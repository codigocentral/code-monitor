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

/// Parse a `DATETIME` as MariaDB renders it, `2026-07-31 13:39:13`.
///
/// The server stores these without a zone; they are read as UTC, which is what
/// the fleet's hosts run on. A value that cannot be parsed is reported as
/// unknown rather than guessed at.
fn parse_mysql_datetime(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with("0000-00-00") {
        return None;
    }

    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|naive| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc))
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

    fn parse_schema_info(
        name: String,
        size_bytes: u64,
        table_count: u64,
        last_write_at: Option<chrono::DateTime<chrono::Utc>>,
        write_time_available: bool,
    ) -> MariaDBSchemaInfo {
        MariaDBSchemaInfo {
            name,
            size_bytes,
            table_count: table_count as u32,
            last_write_at,
            write_time_available,
        }
    }

    async fn collect_schemas(&self, conn: &mut Conn) -> Result<Vec<MariaDBSchemaInfo>> {
        // update_time is NULL for every InnoDB table, so MAX over it is only
        // meaningful where some table is on an engine that reports it. The
        // count of non-null values is carried alongside so a missing time can
        // be reported as unknown rather than as "never written" — reading it
        // the other way is how a live database gets deleted.
        let rows: Vec<(String, u64, u64, Option<String>, u64)> = conn
            .query(
                "SELECT table_schema,
                        CAST(SUM(data_length + index_length) AS UNSIGNED) AS size_bytes,
                        COUNT(*) AS table_count,
                        CAST(MAX(update_time) AS CHAR) AS last_write,
                        SUM(update_time IS NOT NULL) AS with_write_time
                 FROM information_schema.tables
                 WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
                 GROUP BY table_schema
                 ORDER BY size_bytes DESC",
            )
            .await
            .context("Failed to query schema sizes")?;

        Ok(rows
            .into_iter()
            .map(
                |(name, size_bytes, table_count, last_write, with_write_time)| {
                    Self::parse_schema_info(
                        name,
                        size_bytes,
                        table_count,
                        last_write.as_deref().and_then(parse_mysql_datetime),
                        with_write_time > 0,
                    )
                },
            )
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
        assert_eq!(
            collector.config.socket_path.as_ref().unwrap(),
            "/run/mysqld/mysqld.sock"
        );
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
    fn test_parse_mysql_datetime() {
        let parsed = parse_mysql_datetime("2026-07-31 13:39:13").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-07-31T13:39:13+00:00");
    }

    #[test]
    fn test_parse_mysql_datetime_rejects_zero_date() {
        // MariaDB's zero date means "no value", not the year zero
        assert!(parse_mysql_datetime("0000-00-00 00:00:00").is_none());
    }

    #[test]
    fn test_parse_mysql_datetime_rejects_garbage() {
        assert!(parse_mysql_datetime("").is_none());
        assert!(parse_mysql_datetime("   ").is_none());
        assert!(parse_mysql_datetime("not a date").is_none());
    }

    #[test]
    fn test_schema_without_write_time_is_not_reported_as_never_written() {
        // Every InnoDB table leaves update_time NULL. Reading that as "never
        // written" is how a live database gets deleted.
        let info = MariaDBCollector::parse_schema_info("app".to_string(), 1024, 5, None, false);

        assert!(info.last_write_at.is_none());
        assert!(
            !info.write_time_available,
            "the absence must be marked as unknown, not as absence of writes"
        );
        assert!(!info.is_empty(), "it still has tables");
    }

    #[test]
    fn test_schema_with_write_time() {
        let when = chrono::Utc::now();
        let info =
            MariaDBCollector::parse_schema_info("legacy".to_string(), 1024, 3, Some(when), true);

        assert!(info.write_time_available);
        assert_eq!(info.last_write_at, Some(when));
    }

    #[test]
    fn test_empty_schema_is_unambiguous() {
        // academiadotenista_com_br and mautic on alemanha8: no tables at all,
        // which needs no engine cooperation to establish
        let info = MariaDBCollector::parse_schema_info("empty_db".to_string(), 0, 0, None, false);
        assert!(info.is_empty());
    }

    #[test]
    fn test_parse_schema_info() {
        let info =
            MariaDBCollector::parse_schema_info("app_db".to_string(), 10_485_760, 42, None, false);
        assert_eq!(info.name, "app_db");
        assert_eq!(info.size_bytes, 10_485_760);
        assert_eq!(info.table_count, 42);
    }

    #[test]
    fn test_parse_schema_info_empty() {
        let info = MariaDBCollector::parse_schema_info("".to_string(), 0, 0, None, false);
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
            None,
            false,
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
        assert_eq!(
            collector.config.socket_path.as_ref().unwrap(),
            "/run/mysqld/mysqld.sock"
        );
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
        let info =
            MariaDBCollector::parse_schema_info("huge".to_string(), u64::MAX, 1, None, false);
        assert_eq!(info.size_bytes, u64::MAX);
        assert_eq!(info.table_count, 1);
    }

    #[test]
    fn test_parse_schema_info_zero_table_count() {
        let info = MariaDBCollector::parse_schema_info("empty".to_string(), 0, 0, None, false);
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
