use crate::{
    storage::campaign_storage_units as fixture,
    unit_log_compat,
    unit_sqlite::{Expected, Reply},
};
use rusqlite::types::Value;
#[test]
fn native_log_compatibility_formats_both_export_paths_without_a_subscriber() {
    unit_log_compat::install();
    let (storage, state) = fixture::fixture();
    let temp = tempfile::tempdir().unwrap();
    for format in ["csv", "json"] {
        state.lock().unwrap().expected.push_back(Expected {sql:"SELECT id, server_id, timestamp, cpu_usage, memory_used, memory_total FROM metrics WHERE server_id = ?1 AND timestamp > ?2 ORDER BY timestamp ASC",parameters:vec![Value::Text("server-unit".into()),Value::Integer(1_700_000_000-720*3600)],reply:Reply::Rows(vec![])});
        let path = temp.path().join(format!("unit.{format}"));
        let count = if format == "csv" {
            storage.export_csv("server-unit", &path)
        } else {
            storage.export_json("server-unit", &path)
        }
        .unwrap();
        assert_eq!(count, 0);
    }
    let records = unit_log_compat::records();
    for extension in ["unit.csv", "unit.json"] {
        assert!(records
            .iter()
            .any(|s| s.contains("Exported 0 records") && s.contains(extension)));
    }
    assert!(state.lock().unwrap().expected.is_empty());
}
