//! MariaDB cluster metrics collector
//!
//! Connects to MariaDB/MySQL via TCP or Unix socket and collects
//! schema sizes, process list, and basic InnoDB status.

use anyhow::{Context, Result};
use mysql_async::{prelude::Queryable, Conn, OptsBuilder};
use shared::types::{MariaDBClusterInfo, MariaDBProcessInfo, MariaDBSchemaInfo};
use tracing::info;

use crate::config::MariaDBClusterConfig;

/// Collector for a single MariaDB cluster
pub struct MariaDBCollector {
    config: MariaDBClusterConfig,
}

impl MariaDBCollector {
    pub fn new(config: MariaDBClusterConfig) -> Self {
        Self { config }
    }

    async fn connect(&self) -> Result<Conn> {
        let mut opts = OptsBuilder::default()
            .ip_or_hostname(&self.config.host)
            .tcp_port(self.config.port)
            .user(Some(&self.config.user))
            .prefer_socket(false);

        if let Some(ref password) = self.config.password {
            opts = opts.pass(Some(password));
        }

        if let Some(ref socket) = self.config.socket_path {
            opts = opts.socket(Some(socket));
        }

        let conn = Conn::new(opts).await.with_context(|| {
            format!(
                "Failed to connect to MariaDB cluster '{}' at {}:{}",
                self.config.name, self.config.host, self.config.port
            )
        })?;

        info!(
            "Connected to MariaDB cluster '{}' (host={})",
            self.config.name, self.config.host
        );

        Ok(conn)
    }

    /// Collect metrics from this MariaDB cluster
    pub async fn collect(&self) -> Result<MariaDBClusterInfo> {
        let mut conn = self.connect().await?;

        let schemas = self.collect_schemas(&mut conn).await?;
        let (connections_active, connections_total) = self.collect_connections(&mut conn).await?;
        let processes = self.collect_processes(&mut conn).await?;
        let innodb_status = self.collect_innodb_status(&mut conn).await.ok();

        Ok(MariaDBClusterInfo {
            name: self.config.name.clone(),
            host: self.config.host.clone(),
            port: self.config.port,
            schemas,
            connections_active,
            connections_total,
            innodb_status,
            processes,
            timestamp: chrono::Utc::now(),
        })
    }

    fn parse_schema_info(name: String, size_bytes: u64, table_count: u64) -> MariaDBSchemaInfo {
        MariaDBSchemaInfo {
            name,
            size_bytes,
            table_count: table_count as u32,
        }
    }

    async fn collect_schemas(&self, conn: &mut Conn) -> Result<Vec<MariaDBSchemaInfo>> {
        let rows: Vec<(String, u64, u64)> = conn
            .query(
                "SELECT table_schema,
                        CAST(SUM(data_length + index_length) AS UNSIGNED) AS size_bytes,
                        COUNT(*) AS table_count
                 FROM information_schema.tables
                 WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
                 GROUP BY table_schema
                 ORDER BY size_bytes DESC",
            )
            .await
            .context("Failed to query schema sizes")?;

        Ok(rows
            .into_iter()
            .map(|(name, size_bytes, table_count)| {
                Self::parse_schema_info(name, size_bytes, table_count)
            })
            .collect())
    }

    async fn collect_connections(&self, conn: &mut Conn) -> Result<(u32, u32)> {
        let active_rows: Vec<(String, u64)> = conn
            .query("SHOW STATUS LIKE 'Threads_connected'")
            .await
            .context("Failed to query threads connected")?;

        let active: u32 = active_rows
            .into_iter()
            .next()
            .map(|(_, v)| v as u32)
            .unwrap_or(0);

        let total_rows: Vec<(String, u64)> = conn
            .query("SHOW STATUS LIKE 'Max_used_connections'")
            .await
            .context("Failed to query max connections")?;

        let total: u32 = total_rows
            .into_iter()
            .next()
            .map(|(_, v)| v as u32)
            .unwrap_or(0);

        Ok((active, total))
    }

    async fn collect_processes(&self, conn: &mut Conn) -> Result<Vec<MariaDBProcessInfo>> {
        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            u64,
            String,
            String,
            Option<String>,
            String,
            u64,
            String,
            Option<String>,
        )> = conn
            .query(
                "SELECT ID, USER, HOST, DB, COMMAND, TIME, STATE, INFO
                 FROM information_schema.processlist
                 WHERE COMMAND <> 'Sleep'
                 ORDER BY TIME DESC
                 LIMIT 50",
            )
            .await
            .context("Failed to query processlist")?;

        Ok(rows
            .into_iter()
            .map(
                |(id, user, host, db, command, time, state, info)| MariaDBProcessInfo {
                    id,
                    user,
                    host,
                    db,
                    command,
                    time_seconds: time as u32,
                    state,
                    info,
                },
            )
            .collect())
    }

    async fn collect_innodb_status(&self, conn: &mut Conn) -> Result<String> {
        let rows: Vec<(String, String, String)> = conn
            .query("SHOW ENGINE INNODB STATUS")
            .await
            .context("Failed to query InnoDB status")?;

        let lines: Vec<String> = rows.into_iter().map(|(_, _, line)| line).collect();
        let status = lines.join("\n");
        let truncated = if status.len() > 4000 {
            format!("{}... (truncated)", &status[..4000])
        } else {
            status
        };

        Ok(truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mariadb_collector_new() {
        let config = MariaDBClusterConfig {
            name: "test".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: true,
        };
        let collector = MariaDBCollector::new(config);
        assert_eq!(collector.config.name, "test");
    }

    #[test]
    fn test_mariadb_collector_with_password() {
        let config = MariaDBClusterConfig {
            name: "test-pass".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: Some("secret".to_string()),
            socket_path: None,
            enabled: true,
        };
        let collector = MariaDBCollector::new(config);
        assert_eq!(collector.config.password.as_ref().unwrap(), "secret");
    }

    #[test]
    fn test_mariadb_collector_with_socket() {
        let config = MariaDBClusterConfig {
            name: "test-socket".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "code-monitor".to_string(),
            password: None,
            socket_path: Some("/run/mysqld/mysqld.sock".to_string()),
            enabled: true,
        };
        let collector = MariaDBCollector::new(config);
        assert_eq!(collector.config.socket_path.as_ref().unwrap(), "/run/mysqld/mysqld.sock");
    }

    #[test]
    fn test_mariadb_collector_disabled() {
        let config = MariaDBClusterConfig {
            name: "disabled".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "code-monitor".to_string(),
            password: None,
            socket_path: None,
            enabled: false,
        };
        let collector = MariaDBCollector::new(config);
        assert!(!collector.config.enabled);
    }

    #[test]
    fn test_parse_schema_info() {
        let info = MariaDBCollector::parse_schema_info(
            "app_db".to_string(),
            10_485_760,
            42,
        );
        assert_eq!(info.name, "app_db");
        assert_eq!(info.size_bytes, 10_485_760);
        assert_eq!(info.table_count, 42);
    }

    #[test]
    fn test_parse_schema_info_empty() {
        let info = MariaDBCollector::parse_schema_info(
            "".to_string(),
            0,
            0,
        );
        assert_eq!(info.name, "");
        assert_eq!(info.size_bytes, 0);
        assert_eq!(info.table_count, 0);
    }

    #[test]
    fn test_parse_schema_info_large_table_count() {
        let info = MariaDBCollector::parse_schema_info(
            "big_db".to_string(),
            u64::MAX,
            u64::MAX,
        );
        assert_eq!(info.name, "big_db");
        assert_eq!(info.size_bytes, u64::MAX);
        // u64::MAX as u32 wraps around
        assert_eq!(info.table_count, u64::MAX as u32);
    }

    #[test]
    fn test_mariadb_collector_full_config() {
        let config = MariaDBClusterConfig {
            name: "full".to_string(),
            host: "db.example.com".to_string(),
            port: 3307,
            user: "admin".to_string(),
            password: Some("secret".to_string()),
            socket_path: Some("/run/mysqld/mysqld.sock".to_string()),
            enabled: true,
        };
        let collector = MariaDBCollector::new(config);
        assert_eq!(collector.config.name, "full");
        assert_eq!(collector.config.host, "db.example.com");
        assert_eq!(collector.config.port, 3307);
        assert_eq!(collector.config.user, "admin");
        assert_eq!(collector.config.password.as_ref().unwrap(), "secret");
        assert_eq!(collector.config.socket_path.as_ref().unwrap(), "/run/mysqld/mysqld.sock");
        assert!(collector.config.enabled);
    }

    #[test]
    fn test_mariadb_collector_new_minimal_config() {
        let config = MariaDBClusterConfig {
            name: "minimal".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3306,
            user: "root".to_string(),
            password: None,
            socket_path: None,
            enabled: false,
        };
        let collector = MariaDBCollector::new(config);
        assert_eq!(collector.config.name, "minimal");
        assert_eq!(collector.config.host, "127.0.0.1");
        assert!(!collector.config.enabled);
        assert!(collector.config.password.is_none());
        assert!(collector.config.socket_path.is_none());
    }

    #[test]
    fn test_parse_schema_info_max_size() {
        let info = MariaDBCollector::parse_schema_info(
            "huge".to_string(),
            u64::MAX,
            1,
        );
        assert_eq!(info.size_bytes, u64::MAX);
        assert_eq!(info.table_count, 1);
    }

    #[test]
    fn test_parse_schema_info_zero_table_count() {
        let info = MariaDBCollector::parse_schema_info(
            "empty".to_string(),
            0,
            0,
        );
        assert_eq!(info.size_bytes, 0);
        assert_eq!(info.table_count, 0);
    }

    #[test]
    fn test_mariadb_collector_enabled_true() {
        let config = MariaDBClusterConfig {
            name: "enabled".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            user: "user".to_string(),
            password: None,
            socket_path: None,
            enabled: true,
        };
        let collector = MariaDBCollector::new(config);
        assert!(collector.config.enabled);
    }
}
