//! Postgres cluster metrics collector
//!
//! Connects to Postgres via TCP or Unix socket and collects
//! database sizes, connection counts, cache hit ratios, and top queries.

use anyhow::{Context, Result};
use shared::types::{ConnectionStateCount, PostgresClusterInfo, PostgresDatabaseInfo, TopQuery};
use std::sync::Arc;
use tokio_postgres::{Client, NoTls};
use tracing::{error, info};

use crate::config::PostgresClusterConfig;

/// Collector for a single Postgres cluster
pub struct PostgresCollector {
    config: PostgresClusterConfig,
    client: tokio::sync::Mutex<Option<Arc<Client>>>,
}

impl PostgresCollector {
    pub fn new(config: PostgresClusterConfig) -> Self {
        Self {
            config,
            client: tokio::sync::Mutex::new(None),
        }
    }

    /// Ensure we have a connected client
    async fn ensure_connected(&self) -> Result<Arc<Client>> {
        {
            let guard = self.client.lock().await;
            if let Some(ref client) = *guard {
                if client.query_one("SELECT 1", &[]).await.is_ok() {
                    return Ok(Arc::clone(client));
                }
            }
        }
        // Need to reconnect
        let client = Arc::new(self.connect().await?);
        let mut guard = self.client.lock().await;
        *guard = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn connect(&self) -> Result<Client> {
        let mut connect_config = tokio_postgres::Config::new();
        connect_config
            .user(&self.config.user)
            .dbname(&self.config.database)
            .application_name("code-monitor-agent")
            .options("-c statement_timeout=10s");

        if let Some(ref socket) = self.config.socket_path {
            connect_config.host(socket);
        } else {
            connect_config.host(&self.config.host);
            connect_config.port(self.config.port);
        }

        if let Some(ref password) = self.config.password {
            connect_config.password(password);
        }

        let (client, connection) = connect_config.connect(NoTls).await.with_context(|| {
            format!(
                "Failed to connect to Postgres cluster '{}' at {}:{}",
                self.config.name, self.config.host, self.config.port
            )
        })?;

        // Spawn the connection driver
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                error!("Postgres connection error: {}", e);
            }
        });

        info!(
            "Connected to Postgres cluster '{}' (host={}, db={})",
            self.config.name, self.config.host, self.config.database
        );

        Ok(client)
    }

    /// Collect metrics from this Postgres cluster
    pub async fn collect(&self) -> Result<PostgresClusterInfo> {
        let client = self.ensure_connected().await?;

        let databases = self.collect_databases(&client).await?;
        let (connections_total, connections_by_state) = self.collect_connections(&client).await?;
        let cache_hit_ratio = self.collect_cache_hit_ratio(&client).await?;
        let top_queries = self.collect_top_queries(&client).await?;

        Ok(PostgresClusterInfo {
            name: self.config.name.clone(),
            host: self.config.host.clone(),
            port: self.config.port,
            databases,
            connections_total,
            connections_by_state,
            cache_hit_ratio,
            top_queries,
            timestamp: chrono::Utc::now(),
        })
    }

    fn parse_database_info(
        name: String,
        size_bytes: i64,
        num_backends: i32,
        cache_hit_ratio: f64,
    ) -> PostgresDatabaseInfo {
        PostgresDatabaseInfo {
            name,
            size_bytes: size_bytes as u64,
            num_backends: num_backends as u32,
            cache_hit_ratio,
        }
    }

    async fn collect_databases(
        &self,
        client: &tokio_postgres::Client,
    ) -> Result<Vec<PostgresDatabaseInfo>> {
        let rows = client
            .query(
                "SELECT datname, pg_database_size(datname), numbackends,
                        CASE WHEN blks_hit + blks_read > 0
                             THEN blks_hit * 100.0 / (blks_hit + blks_read)
                             ELSE 100.0
                        END as cache_hit
                 FROM pg_stat_database
                 WHERE datname NOT LIKE 'template%' AND datname <> 'postgres'",
                &[],
            )
            .await
            .context("Failed to query database stats")?;

        let mut databases = Vec::with_capacity(rows.len());
        for row in rows {
            databases.push(Self::parse_database_info(
                row.try_get(0).unwrap_or_default(),
                row.try_get::<_, i64>(1).unwrap_or(0),
                row.try_get::<_, i32>(2).unwrap_or(0),
                row.try_get::<_, f64>(3).unwrap_or(100.0),
            ));
        }

        Ok(databases)
    }

    async fn collect_connections(
        &self,
        client: &tokio_postgres::Client,
    ) -> Result<(u32, Vec<ConnectionStateCount>)> {
        let rows = client
            .query(
                "SELECT COALESCE(state, 'unknown') as state, count(*)
                 FROM pg_stat_activity
                 GROUP BY state",
                &[],
            )
            .await
            .context("Failed to query connection states")?;

        let mut total = 0u32;
        let mut counts = Vec::with_capacity(rows.len());
        for row in rows {
            let state: String = row.try_get(0).unwrap_or_default();
            let count: i64 = row.try_get(1).unwrap_or(0);
            total += count as u32;
            counts.push(ConnectionStateCount {
                state,
                count: count as u32,
            });
        }

        Ok((total, counts))
    }

    async fn collect_cache_hit_ratio(&self, client: &tokio_postgres::Client) -> Result<f64> {
        let row = client
            .query_one(
                "SELECT CASE WHEN sum(blks_hit) + sum(blks_read) > 0
                             THEN sum(blks_hit) * 100.0 / (sum(blks_hit) + sum(blks_read))
                             ELSE 100.0
                        END as cache_hit
                 FROM pg_stat_database
                 WHERE datname NOT LIKE 'template%'",
                &[],
            )
            .await
            .context("Failed to query cache hit ratio")?;

        Ok(row.try_get::<_, f64>(0).unwrap_or(100.0))
    }

    async fn collect_top_queries(&self, client: &tokio_postgres::Client) -> Result<Vec<TopQuery>> {
        // Check if pg_stat_statements extension is installed
        let ext_exists = client
            .query_one(
                "SELECT EXISTS (
                    SELECT 1 FROM pg_extension WHERE extname = 'pg_stat_statements'
                )",
                &[],
            )
            .await;

        let has_extension = match ext_exists {
            Ok(row) => row.try_get::<_, bool>(0).unwrap_or(false),
            Err(_) => false,
        };

        if !has_extension {
            return Ok(Vec::new());
        }

        let rows = client
            .query(
                "SELECT query, calls, total_exec_time, mean_exec_time
                 FROM pg_stat_statements
                 ORDER BY total_exec_time DESC
                 LIMIT 5",
                &[],
            )
            .await
            .context("Failed to query top statements")?;

        let mut queries = Vec::with_capacity(rows.len());
        for row in rows {
            queries.push(TopQuery {
                query: row.try_get(0).unwrap_or_default(),
                calls: row.try_get::<_, i64>(1).unwrap_or(0) as u64,
                total_exec_time_ms: row.try_get::<_, f64>(2).unwrap_or(0.0),
                mean_exec_time_ms: row.try_get::<_, f64>(3).unwrap_or(0.0),
            });
        }

        Ok(queries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_collector_new() {
        let config = PostgresClusterConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: true,
        };
        let collector = PostgresCollector::new(config);
        assert_eq!(collector.config.name, "test");
    }

    #[test]
    fn test_postgres_collector_with_socket() {
        let config = PostgresClusterConfig {
            name: "test-socket".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "postgres".to_string(),
            password: None,
            socket_path: Some("/run/postgresql/.s.PGSQL.5432".to_string()),
            enabled: true,
        };
        let collector = PostgresCollector::new(config);
        assert_eq!(collector.config.socket_path.as_ref().unwrap(), "/run/postgresql/.s.PGSQL.5432");
    }

    #[test]
    fn test_postgres_collector_disabled() {
        let config = PostgresClusterConfig {
            name: "disabled".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "postgres".to_string(),
            password: None,
            socket_path: None,
            enabled: false,
        };
        let collector = PostgresCollector::new(config);
        assert!(!collector.config.enabled);
    }

    #[test]
    fn test_parse_database_info() {
        let info = PostgresCollector::parse_database_info(
            "mydb".to_string(),
            1_048_576,
            5,
            99.5,
        );
        assert_eq!(info.name, "mydb");
        assert_eq!(info.size_bytes, 1_048_576);
        assert_eq!(info.num_backends, 5);
        assert!((info.cache_hit_ratio - 99.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_database_info_negative_size() {
        // Edge case: negative size from DB should be cast to large u64
        let info = PostgresCollector::parse_database_info(
            "test".to_string(),
            -1,
            0,
            100.0,
        );
        assert_eq!(info.size_bytes, u64::MAX); // -1 as i64 cast to u64
    }

    #[test]
    fn test_parse_database_info_zero() {
        let info = PostgresCollector::parse_database_info(
            "".to_string(),
            0,
            0,
            0.0,
        );
        assert_eq!(info.name, "");
        assert_eq!(info.size_bytes, 0);
        assert_eq!(info.num_backends, 0);
        assert_eq!(info.cache_hit_ratio, 0.0);
    }

    #[test]
    fn test_postgres_collector_with_password() {
        let config = PostgresClusterConfig {
            name: "test-pass".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "postgres".to_string(),
            password: Some("secret123".to_string()),
            socket_path: None,
            enabled: true,
        };
        let collector = PostgresCollector::new(config);
        assert_eq!(collector.config.password.as_ref().unwrap(), "secret123");
    }

    #[test]
    fn test_postgres_collector_full_config() {
        let config = PostgresClusterConfig {
            name: "full".to_string(),
            host: "db.example.com".to_string(),
            port: 5433,
            database: "mydb".to_string(),
            user: "admin".to_string(),
            password: Some("pass".to_string()),
            socket_path: Some("/tmp/.s.PGSQL.5433".to_string()),
            enabled: false,
        };
        let collector = PostgresCollector::new(config);
        assert_eq!(collector.config.name, "full");
        assert_eq!(collector.config.host, "db.example.com");
        assert_eq!(collector.config.port, 5433);
        assert_eq!(collector.config.database, "mydb");
        assert_eq!(collector.config.user, "admin");
        assert!(collector.config.password.is_some());
        assert!(collector.config.socket_path.is_some());
        assert!(!collector.config.enabled);
    }

    #[test]
    fn test_parse_database_info_max_values() {
        let info = PostgresCollector::parse_database_info(
            "maxdb".to_string(),
            i64::MAX,
            i32::MAX,
            f64::MAX,
        );
        assert_eq!(info.name, "maxdb");
        assert_eq!(info.size_bytes, i64::MAX as u64);
        assert_eq!(info.num_backends, i32::MAX as u32);
        assert!(info.cache_hit_ratio.is_finite());
    }

    #[test]
    fn test_parse_database_info_min_values() {
        let info = PostgresCollector::parse_database_info(
            "mindb".to_string(),
            i64::MIN,
            i32::MIN,
            f64::MIN,
        );
        assert_eq!(info.name, "mindb");
        // i64::MIN as u64 wraps to 2^63
        assert_eq!(info.size_bytes, i64::MIN as u64);
        assert_eq!(info.num_backends, i32::MIN as u32);
    }

    #[test]
    fn test_parse_database_info_nan_cache_hit() {
        let info = PostgresCollector::parse_database_info(
            "nandb".to_string(),
            100,
            1,
            f64::NAN,
        );
        assert_eq!(info.name, "nandb");
        assert!(info.cache_hit_ratio.is_nan());
    }

    #[test]
    fn test_parse_database_info_infinite_cache_hit() {
        let info = PostgresCollector::parse_database_info(
            "infdb".to_string(),
            100,
            1,
            f64::INFINITY,
        );
        assert!(info.cache_hit_ratio.is_infinite());
    }
}
