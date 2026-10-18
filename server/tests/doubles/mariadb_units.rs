//! Actual MariaDB collector behavior on typed SDK rows, no database.
use super::*;
use crate::unit_maria as sdk;
use std::{
    io,
    sync::{Arc, Mutex},
};
fn config() -> MariaDBClusterConfig {
    toml::from_str("name = 'Unit'\nhost = 'fixture.invalid'").unwrap()
}
fn fixture() -> (MariaDBCollector, Conn, sdk::SharedState) {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    (MariaDBCollector::new(config()), Conn(state.clone()), state)
}
fn queue<T: Send + 'static>(state: &sdk::SharedState, contains: &'static str, rows: Vec<T>) {
    state.lock().unwrap().expected.push_back(sdk::Expected {
        contains,
        reply: Ok(Box::new(rows)),
    });
}
fn reject(state: &sdk::SharedState, contains: &'static str) {
    state.lock().unwrap().expected.push_back(sdk::Expected {
        contains,
        reply: Err(io::Error::other("unit MariaDB refused")),
    });
}
#[tokio::test]
async fn connection_options_preserve_password_socket_and_errors() {
    let (_guard, _logs) = crate::unit_logs::capture();
    for optional in [false, true] {
        let mut config = config();
        if optional {
            config.password = Some("unit-password".into());
            config.socket_path = Some("unit.sock".into());
        }
        let collector = MariaDBCollector::new(config);
        sdk::next(Ok(Arc::new(Mutex::new(sdk::State::default()))));
        collector.connect().await.unwrap();
        sdk::OPTIONS.with(|o| {
            let o = o.borrow();
            let options = o.last().unwrap();
            assert_eq!(options.tcp_port(), 3306);
            assert_eq!(options.user(), Some("code-monitor"));
            assert_eq!(
                options.pass(),
                if optional {
                    Some("unit-password")
                } else {
                    None
                }
            );
            assert_eq!(
                options.socket(),
                if optional { Some("unit.sock") } else { None }
            );
            assert!(!options.prefer_socket());
        });
    }
    let (collector, _, _) = fixture();
    sdk::next(Err(io::Error::other("unit offline")));
    assert!(collector
        .connect()
        .await
        .unwrap_err()
        .to_string()
        .contains("Failed to connect to MariaDB cluster 'Unit' at fixture.invalid:3306"));
}
#[tokio::test]
async fn schemas_processes_connections_and_innodb_are_decoded_and_truncated() {
    let (collector, mut conn, state) = fixture();
    queue(
        &state,
        "FROM information_schema.tables",
        vec![
            (
                "app".to_string(),
                20u64,
                3u64,
                Some("2026-01-01 00:00:00".to_string()),
                1u64,
            ),
            ("empty".into(), 0, 0, None, 0),
        ],
    );
    let schemas = collector.collect_schemas(&mut conn).await.unwrap();
    assert!(schemas[0].last_write_at.is_some());
    assert!(schemas[0].write_time_available);
    assert!(schemas[1].last_write_at.is_none());
    assert!(!schemas[1].write_time_available);
    for present in [false, true] {
        for name in ["Threads_connected", "Max_used_connections"] {
            queue(
                &state,
                name,
                if present {
                    vec![(name.to_string(), 7u64)]
                } else {
                    Vec::<(String, u64)>::new()
                },
            );
        }
        assert_eq!(
            collector.collect_connections(&mut conn).await.unwrap(),
            if present { (7, 7) } else { (0, 0) }
        );
    }
    queue(
        &state,
        "FROM information_schema.processlist",
        vec![(
            7u64,
            "u".to_string(),
            "fixture.invalid".to_string(),
            Some("app".to_string()),
            "Query".to_string(),
            9u64,
            "running".to_string(),
            Some("SELECT 1".to_string()),
        )],
    );
    let processes = collector.collect_processes(&mut conn).await.unwrap();
    assert_eq!(processes[0].id, 7);
    assert_eq!(processes[0].time_seconds, 9);
    assert_eq!(processes[0].info.as_deref(), Some("SELECT 1"));
    for long in [false, true] {
        let text = if long {
            "x".repeat(4001)
        } else {
            "unit innodb".into()
        };
        queue(
            &state,
            "SHOW ENGINE INNODB STATUS",
            vec![("a".to_string(), "b".to_string(), text)],
        );
        let status = collector.collect_innodb_status(&mut conn).await.unwrap();
        if long {
            assert_eq!(status.len(), 4000 + "... (truncated)".len());
            assert!(status.ends_with("... (truncated)"));
        } else {
            assert_eq!(status, "unit innodb");
        }
    }
    assert!(state.lock().unwrap().expected.is_empty());
}
#[tokio::test]
async fn collection_propagates_required_failures_but_keeps_optional_status_absent() {
    for failed in 0..6 {
        let (collector, _conn, state) = fixture();
        sdk::next(Ok(state.clone()));
        if failed == 0 {
            reject(&state, "FROM information_schema.tables");
        } else {
            queue::<(String, u64, u64, Option<String>, u64)>(
                &state,
                "FROM information_schema.tables",
                vec![],
            );
            if failed == 1 {
                reject(&state, "Threads_connected");
            } else {
                queue::<(String, u64)>(&state, "Threads_connected", vec![]);
                if failed == 2 {
                    reject(&state, "Max_used_connections");
                } else {
                    queue::<(String, u64)>(&state, "Max_used_connections", vec![]);
                    if failed == 3 {
                        reject(&state, "FROM information_schema.processlist");
                    } else {
                        queue::<(
                            u64,
                            String,
                            String,
                            Option<String>,
                            String,
                            u64,
                            String,
                            Option<String>,
                        )>(
                            &state, "FROM information_schema.processlist", vec![]
                        );
                        if failed == 4 {
                            reject(&state, "SHOW ENGINE INNODB STATUS");
                        } else {
                            queue::<(String, String, String)>(
                                &state,
                                "SHOW ENGINE INNODB STATUS",
                                vec![],
                            );
                        }
                    }
                }
            }
        }
        let result = collector.collect().await;
        if failed < 4 {
            assert!(result.is_err());
        } else {
            let value = result.unwrap();
            assert_eq!(value.name, "Unit");
            assert_eq!(value.host, "fixture.invalid");
            assert_eq!(value.port, 3306);
            assert_eq!(value.innodb_status.is_none(), failed == 4);
        }
        assert!(state.lock().unwrap().expected.is_empty());
    }
    let (collector, _conn, _state) = fixture();
    sdk::next(Err(io::Error::other("unit connection refused")));
    assert!(collector.collect().await.is_err());
}
