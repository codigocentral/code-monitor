use super::*;
use crate::unit_sqlite::{self as sdk, Expected, Reply, State};
use rusqlite::types::Value;
use std::sync::{Arc, Mutex};
pub(crate) const SETUP: [&str;4]=[
"CREATE TABLE IF NOT EXISTS metrics (id INTEGER PRIMARY KEY AUTOINCREMENT,server_id TEXT NOT NULL,timestamp INTEGER NOT NULL,cpu_usage REAL,memory_used INTEGER,memory_total INTEGER)",
"CREATE INDEX IF NOT EXISTS idx_metrics_server_time ON metrics(server_id, timestamp)",
"CREATE TABLE IF NOT EXISTS storage_meta (key TEXT PRIMARY KEY,value TEXT)",
"INSERT OR REPLACE INTO storage_meta (key, value) VALUES ('schema_version', '1')",
];
pub(crate) const HISTORY:&str="SELECT id, server_id, timestamp, cpu_usage, memory_used, memory_total FROM metrics WHERE server_id = ?1 AND timestamp > ?2 ORDER BY timestamp ASC";
const LATEST:&str="SELECT id, server_id, timestamp, cpu_usage, memory_used, memory_total FROM metrics WHERE server_id = ?1 ORDER BY timestamp DESC LIMIT 1";
pub(crate) const STATS: [&str; 4] = [
    "SELECT COUNT(*) FROM metrics",
    "SELECT MIN(timestamp) FROM metrics",
    "SELECT MAX(timestamp) FROM metrics",
    "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
];
fn sql(sql: &'static str, parameters: Vec<Value>, reply: Reply) -> Expected {
    Expected {
        sql,
        parameters,
        reply,
    }
}
fn text(value: &str) -> Value {
    Value::Text(value.into())
}
pub(crate) fn fixture() -> (MetricsStorage, sdk::SharedState) {
    let state = Arc::new(Mutex::new(State::default()));
    for query in SETUP {
        state
            .lock()
            .unwrap()
            .expected
            .push_back(sql(query, vec![], Reply::Execute(0)));
    }
    sdk::next_open(Ok(state.clone()));
    let storage = MetricsStorage::new("unit-database-never-opened").unwrap();
    assert_eq!(state.lock().unwrap().opened.len(), 1);
    (storage, state)
}
fn row() -> Vec<Value> {
    vec![
        Value::Integer(1),
        text("server-unit"),
        Value::Integer(1_700_000_000),
        Value::Real(50.),
        Value::Integer(60),
        Value::Integer(100),
    ]
}
fn point() -> SystemInfo {
    SystemInfo {
        hostname: "unit".into(),
        os: "unit".into(),
        kernel_version: "unit".into(),
        uptime_seconds: 0,
        cpu_count: 1,
        cpu_usage_percent: 50.,
        memory_total_bytes: 100,
        memory_used_bytes: 60,
        memory_available_bytes: 40,
        disk_info: vec![],
        timestamp: sdk::now(),
        swap: Default::default(),
        memory_pressure: None,
    }
}
fn enqueue(state: &sdk::SharedState, sql_text: &'static str, parameters: Vec<Value>, reply: Reply) {
    state
        .lock()
        .unwrap()
        .expected
        .push_back(sql(sql_text, parameters, reply));
}
fn history(state: &sdk::SharedState, hours: i64, reply: Reply) {
    enqueue(
        state,
        HISTORY,
        vec![
            text("server-unit"),
            Value::Integer(1_700_000_000 - hours * 3600),
        ],
        reply,
    );
}
fn latest(state: &sdk::SharedState, reply: Reply) {
    enqueue(state, LATEST, vec![text("server-unit")], reply);
}
fn drained(state: &sdk::SharedState) {
    assert!(state.lock().unwrap().expected.is_empty());
}

#[test]
fn initialization_is_ordered_and_every_provider_error_propagates() {
    sdk::next_open(Err(sdk::failure()));
    assert!(MetricsStorage::new("never-created").is_err());
    for failed in 0..4 {
        let state = Arc::new(Mutex::new(State::default()));
        for (index, query) in SETUP.into_iter().enumerate().take(failed + 1) {
            state.lock().unwrap().expected.push_back(sql(
                query,
                vec![],
                if index == failed {
                    Reply::Error
                } else {
                    Reply::Execute(0)
                },
            ));
        }
        sdk::next_open(Ok(state.clone()));
        assert!(MetricsStorage::new("never-created").is_err());
        drained(&state);
    }
    let (_storage, state) = fixture();
    drained(&state);
}
#[test]
fn storing_captures_each_metric_parameter_and_propagates_errors() {
    let (storage, state) = fixture();
    let query="INSERT INTO metrics (server_id, timestamp, cpu_usage, memory_used, memory_total) VALUES (?1, ?2, ?3, ?4, ?5)";
    let parameters = vec![
        text("server-unit"),
        Value::Integer(1_700_000_000),
        Value::Real(50.),
        Value::Integer(60),
        Value::Integer(100),
    ];
    enqueue(&state, query, parameters.clone(), Reply::Execute(1));
    storage.store("server-unit", &point()).unwrap();
    enqueue(&state, query, parameters, Reply::Error);
    assert!(storage.store("server-unit", &point()).is_err());
    drained(&state);
}
#[test]
fn history_maps_rows_handles_clock_fallback_and_propagates_every_read_error() {
    let (storage, state) = fixture();
    history(&state, 1, Reply::Rows(vec![row()]));
    let result = storage.get_history("server-unit", 1).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].server_id, "server-unit");
    assert_eq!(result[0].timestamp, sdk::now());
    assert_eq!(result[0].id, 1);
    assert_eq!(result[0].cpu_usage, 50.);
    assert_eq!(result[0].memory_used, 60);
    assert_eq!(result[0].memory_total, 100);
    let mut invalid = row();
    invalid[2] = Value::Integer(i64::MAX);
    history(&state, 1, Reply::Rows(vec![invalid]));
    assert_eq!(
        storage.get_history("server-unit", 1).unwrap()[0].timestamp,
        sdk::now()
    );
    history(&state, 1, Reply::Rows(vec![]));
    assert!(storage.get_history("server-unit", 1).unwrap().is_empty());
    enqueue(&state, "PREPARE ERROR", vec![], Reply::Error);
    assert!(storage.get_history("server-unit", 1).is_err());
    history(&state, 1, Reply::Error);
    assert!(storage.get_history("server-unit", 1).is_err());
    for index in 0..6 {
        let mut invalid = row();
        invalid[index] = Value::Null;
        history(&state, 1, Reply::Rows(vec![invalid]));
        assert!(storage.get_history("server-unit", 1).is_err());
    }
    drained(&state);
}
#[test]
fn latest_handles_absence_conversion_failures_and_out_of_range_time() {
    let (storage, state) = fixture();
    latest(&state, Reply::Rows(vec![row()]));
    let result = storage.get_latest("server-unit").unwrap().unwrap();
    assert_eq!(result.id, 1);
    assert_eq!(result.server_id, "server-unit");
    assert_eq!(result.timestamp, sdk::now());
    assert_eq!(result.cpu_usage, 50.);
    assert_eq!(result.memory_used, 60);
    assert_eq!(result.memory_total, 100);
    latest(&state, Reply::Rows(vec![]));
    assert!(storage.get_latest("server-unit").unwrap().is_none());
    let mut invalid = row();
    invalid[2] = Value::Integer(i64::MAX);
    latest(&state, Reply::Rows(vec![invalid]));
    assert_eq!(
        storage
            .get_latest("server-unit")
            .unwrap()
            .unwrap()
            .timestamp,
        sdk::now()
    );
    latest(&state, Reply::Error);
    assert!(storage.get_latest("server-unit").is_err());
    for index in 0..6 {
        let mut invalid = row();
        invalid[index] = Value::Null;
        latest(&state, Reply::Rows(vec![invalid]));
        assert!(storage.get_latest("server-unit").is_err());
    }
    drained(&state);
}
#[test]
fn purge_uses_the_requested_retention_and_vacuums_only_after_delete() {
    let (storage, state) = fixture();
    for days in [0, 2] {
        let (query, parameters) = if days == 0 {
            ("DELETE FROM metrics", vec![])
        } else {
            (
                "DELETE FROM metrics WHERE timestamp < ?1",
                vec![Value::Integer(1_700_000_000 - days * 86400)],
            )
        };
        enqueue(&state, query, parameters.clone(), Reply::Execute(7));
        enqueue(&state, "VACUUM", vec![], Reply::Execute(0));
        assert_eq!(storage.purge_old(days).unwrap(), 7);
        enqueue(&state, query, parameters.clone(), Reply::Error);
        assert!(storage.purge_old(days).is_err());
        enqueue(&state, query, parameters, Reply::Execute(1));
        enqueue(&state, "VACUUM", vec![], Reply::Error);
        assert!(storage.purge_old(days).is_err());
    }
    drained(&state);
}
#[test]
fn statistics_preserve_optional_ranges_and_propagate_all_query_errors() {
    let (storage, state) = fixture();
    for timestamp in [
        Value::Integer(1_700_000_000),
        Value::Null,
        Value::Integer(i64::MAX),
    ] {
        let rows = [
            Value::Integer(2),
            timestamp.clone(),
            timestamp.clone(),
            Value::Integer(4096),
        ];
        for (query, value) in STATS.into_iter().zip(rows) {
            enqueue(&state, query, vec![], Reply::Rows(vec![vec![value]]));
        }
        let stats = storage.get_stats().unwrap();
        assert_eq!(stats.total_records, 2);
        assert_eq!(stats.size_bytes, 4096);
        assert_eq!(
            stats.oldest_timestamp,
            if timestamp == Value::Integer(1_700_000_000) {
                Some(sdk::now())
            } else {
                None
            }
        );
        assert_eq!(stats.oldest_timestamp, stats.newest_timestamp);
    }
    for failed in 0..4 {
        for (index, query) in STATS.into_iter().enumerate().take(failed + 1) {
            enqueue(
                &state,
                query,
                vec![],
                if index == failed {
                    Reply::Error
                } else {
                    Reply::Rows(vec![vec![Value::Integer(0)]])
                },
            );
        }
        assert!(storage.get_stats().is_err());
    }
    for failed in 0..4 {
        for (index, query) in STATS.into_iter().enumerate().take(failed + 1) {
            enqueue(
                &state,
                query,
                vec![],
                Reply::Rows(vec![vec![if index == failed {
                    text("wrong-type")
                } else {
                    Value::Integer(0)
                }]]),
            );
        }
        assert!(storage.get_stats().is_err());
    }
    enqueue(
        &state,
        STATS[0],
        vec![],
        Reply::Rows(vec![vec![Value::Integer(0)]]),
    );
    enqueue(&state, STATS[1], vec![], Reply::Rows(vec![]));
    enqueue(&state, STATS[2], vec![], Reply::Rows(vec![]));
    enqueue(
        &state,
        STATS[3],
        vec![],
        Reply::Rows(vec![vec![Value::Integer(4096)]]),
    );
    assert!(storage.get_stats().unwrap().oldest_timestamp.is_none());
    drained(&state);
}
#[test]
fn exports_use_owned_files_and_propagate_query_and_creation_errors() {
    let (_guard, logs) = crate::unit_logs::capture();
    let (storage, state) = fixture();
    let temp = tempfile::tempdir().unwrap();
    history(&state, 720, Reply::Rows(vec![row()]));
    let csv = temp.path().join("unit.csv");
    assert_eq!(storage.export_csv("server-unit", &csv).unwrap(), 1);
    let output = std::fs::read_to_string(csv).unwrap();
    assert!(output.starts_with("timestamp,cpu_usage,memory_used,memory_total"));
    assert!(output.contains(",50,60,100"));
    history(&state, 720, Reply::Rows(vec![row()]));
    let json = temp.path().join("unit.json");
    assert_eq!(storage.export_json("server-unit", &json).unwrap(), 1);
    let output: serde_json::Value = serde_json::from_slice(&std::fs::read(json).unwrap()).unwrap();
    assert_eq!(output[0]["server_id"], "server-unit");
    assert_eq!(output[0]["cpu_usage"], 50.);
    let bad = temp.path().join("missing/output");
    assert!(storage.export_csv("server-unit", &bad).is_err());
    history(&state, 720, Reply::Rows(vec![]));
    assert!(storage.export_json("server-unit", &bad).is_err());
    for kind in ["csv", "json"] {
        history(&state, 720, Reply::Error);
        let path = temp.path().join(kind);
        assert!(if kind == "csv" {
            storage.export_csv("server-unit", &path)
        } else {
            storage.export_json("server-unit", &path)
        }
        .is_err());
    }
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|entry| entry.contains("Exported 1 records")));
    drained(&state);
}

#[test]
fn exports_propagate_header_row_flush_and_json_writer_failures() {
    use crate::unit_files::{Fault, Guard};
    let (storage, state) = fixture();
    let temp = tempfile::tempdir().unwrap();
    for fault in [Fault::CsvHeader, Fault::CsvRow, Fault::CsvFlush] {
        if fault != Fault::CsvHeader {
            history(&state, 720, Reply::Rows(vec![row()]));
        }
        let _guard = Guard::arm(fault);
        assert!(storage
            .export_csv("server-unit", &temp.path().join("unit.csv"))
            .err()
            .unwrap()
            .to_string()
            .contains("unit output provider failed"));
    }
    history(&state, 720, Reply::Rows(vec![row()]));
    let _guard = Guard::arm(Fault::JsonWrite);
    assert!(storage
        .export_json("server-unit", &temp.path().join("unit.json"))
        .err()
        .unwrap()
        .to_string()
        .contains("unit output provider failed"));
    drained(&state);
}
