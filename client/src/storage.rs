//! Local storage for metrics history using SQLite
//!
//! This module provides persistent storage for historical metrics data,
//! allowing users to view trends and export data.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use shared::types::SystemInfo;
use std::path::Path;
use tracing::{debug, info};

/// A single metrics data point stored in the database
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsPoint {
    pub id: i64,
    pub server_id: String,
    pub timestamp: DateTime<Utc>,
    pub cpu_usage: f64,
    pub memory_used: u64,
    pub memory_total: u64,
}

/// Storage manager for metrics history
pub struct MetricsStorage {
    conn: Connection,
}

impl MetricsStorage {
    /// Create a new storage instance, initializing the database if needed
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server_id TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                cpu_usage REAL,
                memory_used INTEGER,
                memory_total INTEGER
            )",
            [],
        )?;

        // Create indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_metrics_server_time
             ON metrics(server_id, timestamp)",
            [],
        )?;

        // Create metadata table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS storage_meta (
                key TEXT PRIMARY KEY,
                value TEXT
            )",
            [],
        )?;

        // Set schema version
        conn.execute(
            "INSERT OR REPLACE INTO storage_meta (key, value) VALUES ('schema_version', '1')",
            [],
        )?;

        info!("Metrics storage initialized");

        Ok(Self { conn })
    }

    /// Store a metrics snapshot for a server
    pub fn store(&self, server_id: &str, metrics: &SystemInfo) -> Result<()> {
        let timestamp = metrics.timestamp.timestamp();

        self.conn.execute(
            "INSERT INTO metrics (server_id, timestamp, cpu_usage, memory_used, memory_total)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                server_id,
                timestamp,
                metrics.cpu_usage_percent,
                metrics.memory_used_bytes,
                metrics.memory_total_bytes,
            ],
        )?;

        debug!(
            "Stored metrics for server {} at {}",
            server_id, metrics.timestamp
        );

        Ok(())
    }

    /// Get metrics history for a server
    pub fn get_history(&self, server_id: &str, hours: i64) -> Result<Vec<MetricsPoint>> {
        let since = Utc::now() - chrono::Duration::hours(hours);
        let since_timestamp = since.timestamp();

        let mut stmt = self.conn.prepare(
            "SELECT id, server_id, timestamp, cpu_usage, memory_used, memory_total
             FROM metrics
             WHERE server_id = ?1 AND timestamp > ?2
             ORDER BY timestamp ASC",
        )?;

        let points = stmt.query_map(params![server_id, since_timestamp], |row| {
            let timestamp: i64 = row.get(2)?;
            Ok(MetricsPoint {
                id: row.get(0)?,
                server_id: row.get(1)?,
                timestamp: DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now),
                cpu_usage: row.get(3)?,
                memory_used: row.get(4)?,
                memory_total: row.get(5)?,
            })
        })?;

        let mut result = Vec::new();
        for point in points {
            result.push(point?);
        }

        Ok(result)
    }

    /// Get the latest metrics for a server
    #[allow(dead_code)]
    pub fn get_latest(&self, server_id: &str) -> Result<Option<MetricsPoint>> {
        let point = self
            .conn
            .query_row(
                "SELECT id, server_id, timestamp, cpu_usage, memory_used, memory_total
             FROM metrics
             WHERE server_id = ?1
             ORDER BY timestamp DESC
             LIMIT 1",
                params![server_id],
                |row| {
                    let timestamp: i64 = row.get(2)?;
                    Ok(MetricsPoint {
                        id: row.get(0)?,
                        server_id: row.get(1)?,
                        timestamp: DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now),
                        cpu_usage: row.get(3)?,
                        memory_used: row.get(4)?,
                        memory_total: row.get(5)?,
                    })
                },
            )
            .optional()?;

        Ok(point)
    }

    /// Purge old metrics (retention policy)
    pub fn purge_old(&self, days: i64) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        let cutoff_timestamp = cutoff.timestamp();

        let deleted = self.conn.execute(
            "DELETE FROM metrics WHERE timestamp < ?1",
            params![cutoff_timestamp],
        )?;

        info!("Purged {} old metrics records", deleted);

        // Vacuum to reclaim space
        self.conn.execute("VACUUM", [])?;

        Ok(deleted)
    }

    /// Get storage statistics
    pub fn get_stats(&self) -> Result<StorageStats> {
        let total_records: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))?;

        let oldest: Option<i64> = self
            .conn
            .query_row("SELECT MIN(timestamp) FROM metrics", [], |row| {
                let val: Option<i64> = row.get(0)?;
                Ok(val)
            })
            .optional()?
            .flatten();

        let newest: Option<i64> = self
            .conn
            .query_row("SELECT MAX(timestamp) FROM metrics", [], |row| {
                let val: Option<i64> = row.get(0)?;
                Ok(val)
            })
            .optional()?
            .flatten();

        let size_bytes: i64 = self.conn.query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |row| row.get(0),
        )?;

        Ok(StorageStats {
            total_records: total_records as u64,
            oldest_timestamp: oldest.and_then(|ts| DateTime::from_timestamp(ts, 0)),
            newest_timestamp: newest.and_then(|ts| DateTime::from_timestamp(ts, 0)),
            size_bytes: size_bytes as u64,
        })
    }

    /// Export metrics to CSV
    #[allow(dead_code)]
    pub fn export_csv(&self, server_id: &str, output_path: &Path) -> Result<usize> {
        let mut wtr = csv::Writer::from_path(output_path)?;

        // Write header
        wtr.write_record(["timestamp", "cpu_usage", "memory_used", "memory_total"])?;

        let points = self.get_history(server_id, 24 * 30)?; // Last 30 days

        for point in &points {
            wtr.write_record(&[
                point.timestamp.to_rfc3339(),
                point.cpu_usage.to_string(),
                point.memory_used.to_string(),
                point.memory_total.to_string(),
            ])?;
        }

        wtr.flush()?;

        info!(
            "Exported {} records to {}",
            points.len(),
            output_path.display()
        );

        Ok(points.len())
    }

    /// Export metrics to JSON
    #[allow(dead_code)]
    pub fn export_json(&self, server_id: &str, output_path: &Path) -> Result<usize> {
        let points = self.get_history(server_id, 24 * 30)?; // Last 30 days

        let file = std::fs::File::create(output_path)?;
        serde_json::to_writer_pretty(file, &points)?;

        info!(
            "Exported {} records to {}",
            points.len(),
            output_path.display()
        );

        Ok(points.len())
    }
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_records: u64,
    pub oldest_timestamp: Option<DateTime<Utc>>,
    pub newest_timestamp: Option<DateTime<Utc>>,
    pub size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn create_test_system_info() -> SystemInfo {
        SystemInfo {
            hostname: "test".to_string(),
            os: "Linux".to_string(),
            kernel_version: "5.0".to_string(),
            uptime_seconds: 100,
            cpu_count: 4,
            cpu_usage_percent: 50.0,
            memory_total_bytes: 16_000_000_000,
            memory_used_bytes: 8_000_000_000,
            memory_available_bytes: 8_000_000_000,
            disk_info: vec![],
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_storage_create_and_store() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let metrics = create_test_system_info();
        storage.store("server-1", &metrics).unwrap();

        let history = storage.get_history("server-1", 24).unwrap();
        assert_eq!(history.len(), 1);
        assert!((history[0].cpu_usage - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_storage_get_latest() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let metrics = create_test_system_info();
        storage.store("server-1", &metrics).unwrap();

        let latest = storage.get_latest("server-1").unwrap();
        assert!(latest.is_some());
        let point = latest.unwrap();
        assert!((point.cpu_usage - 50.0).abs() < 0.01);
        assert_eq!(point.memory_used, 8_000_000_000);
        assert_eq!(point.memory_total, 16_000_000_000);
        assert_eq!(point.server_id, "server-1");
    }

    #[test]
    fn test_storage_multiple_servers() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let mut metrics1 = create_test_system_info();
        metrics1.cpu_usage_percent = 10.0;
        let mut metrics2 = create_test_system_info();
        metrics2.cpu_usage_percent = 20.0;

        storage.store("server-a", &metrics1).unwrap();
        storage.store("server-b", &metrics2).unwrap();

        let history_a = storage.get_history("server-a", 24).unwrap();
        let history_b = storage.get_history("server-b", 24).unwrap();
        assert_eq!(history_a.len(), 1);
        assert_eq!(history_b.len(), 1);
        assert!((history_a[0].cpu_usage - 10.0).abs() < 0.01);
        assert!((history_b[0].cpu_usage - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_storage_get_latest_none() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let latest = storage.get_latest("nonexistent").unwrap();
        assert!(latest.is_none());
    }

    #[test]
    fn test_storage_purge_old() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let mut old_metrics = create_test_system_info();
        old_metrics.timestamp = Utc::now() - chrono::Duration::days(10);
        let new_metrics = create_test_system_info();

        storage.store("server-1", &old_metrics).unwrap();
        storage.store("server-1", &new_metrics).unwrap();

        let deleted = storage.purge_old(7).unwrap();
        assert_eq!(deleted, 1);

        let history = storage.get_history("server-1", 24).unwrap();
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_storage_stats() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let metrics = create_test_system_info();
        storage.store("server-1", &metrics).unwrap();
        storage.store("server-1", &metrics).unwrap();

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.total_records, 2);
        assert!(stats.size_bytes > 0);
        assert!(stats.newest_timestamp.is_some());
        assert!(stats.oldest_timestamp.is_some());
    }

    #[test]
    fn test_storage_stats_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.total_records, 0);
        assert!(stats.oldest_timestamp.is_none());
        assert!(stats.newest_timestamp.is_none());
    }

    #[test]
    fn test_storage_get_history_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let history = storage.get_history("server-1", 24).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn test_storage_purge_old_zero_days() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let metrics = create_test_system_info();
        storage.store("server-1", &metrics).unwrap();
        storage.store("server-1", &metrics).unwrap();

        let deleted = storage.purge_old(0).unwrap();
        assert_eq!(deleted, 2);

        let history = storage.get_history("server-1", 24).unwrap();
        assert!(history.is_empty());

        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.total_records, 0);
    }

    #[test]
    fn test_storage_export_csv() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let mut metrics = create_test_system_info();
        metrics.cpu_usage_percent = 42.5;
        metrics.memory_used_bytes = 4_000_000_000;
        metrics.memory_total_bytes = 16_000_000_000;
        storage.store("server-1", &metrics).unwrap();

        let csv_file = NamedTempFile::new().unwrap();
        let exported = storage.export_csv("server-1", csv_file.path()).unwrap();
        assert_eq!(exported, 1);

        let content = std::fs::read_to_string(csv_file.path()).unwrap();
        assert!(content.contains("timestamp"));
        assert!(content.contains("cpu_usage"));
        assert!(content.contains("memory_used"));
        assert!(content.contains("memory_total"));
        assert!(content.contains("42.5"));
        assert!(content.contains("4000000000"));
        assert!(content.contains("16000000000"));
    }

    #[test]
    fn test_storage_export_json() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let mut metrics = create_test_system_info();
        metrics.cpu_usage_percent = 33.3;
        metrics.memory_used_bytes = 2_000_000_000;
        metrics.memory_total_bytes = 8_000_000_000;
        storage.store("server-1", &metrics).unwrap();

        let json_file = NamedTempFile::new().unwrap();
        let exported = storage.export_json("server-1", json_file.path()).unwrap();
        assert_eq!(exported, 1);

        let content = std::fs::read_to_string(json_file.path()).unwrap();
        assert!(content.contains("cpu_usage"));
        assert!(content.contains("33.3"));
        assert!(content.contains("server-1"));
    }

    #[test]
    fn test_storage_export_csv_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let csv_file = NamedTempFile::new().unwrap();
        let exported = storage.export_csv("server-1", csv_file.path()).unwrap();
        assert_eq!(exported, 0);

        let content = std::fs::read_to_string(csv_file.path()).unwrap();
        assert!(content.contains("timestamp"));
    }

    #[test]
    fn test_storage_export_json_empty() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = MetricsStorage::new(temp_file.path()).unwrap();

        let json_file = NamedTempFile::new().unwrap();
        let exported = storage.export_json("server-1", json_file.path()).unwrap();
        assert_eq!(exported, 0);

        let content = std::fs::read_to_string(json_file.path()).unwrap();
        assert!(content.contains("[]"));
    }
}
