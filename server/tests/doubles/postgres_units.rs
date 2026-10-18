//! Actual PostgreSQL collector orchestration and decoding with scripted SDK rows.
use super::*;
use crate::unit_pg as sdk;
use std::{io, sync::Mutex};
fn config() -> PostgresClusterConfig {
    toml::from_str("name = 'Unit'\nhost = 'fixture.invalid'\npassword = 'unit-password'").unwrap()
}
fn fixture() -> (PostgresCollector, sdk::Client, sdk::SharedState) {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    (
        PostgresCollector::new(config()),
        sdk::Client(state.clone()),
        state,
    )
}
fn row(cells: Vec<sdk::Cell>) -> sdk::Row {
    sdk::Row(cells)
}
fn queue(
    state: &sdk::SharedState,
    method: &'static str,
    contains: &'static str,
    reply: io::Result<Vec<sdk::Row>>,
) {
    state.lock().unwrap().expected.push_back(sdk::Expected {
        method,
        contains,
        reply,
    });
}
fn error() -> io::Error {
    io::Error::other("unit PostgreSQL refused")
}
#[tokio::test]
async fn connection_is_reused_checked_reconnected_and_driver_errors_are_logged() {
    let (_guard, logs) = crate::unit_logs::capture();
    let (collector, _client, state) = fixture();
    sdk::next(Ok((state.clone(), Err(error()))));
    let first = collector.ensure_connected().await.unwrap();
    queue(&state, "one", "SELECT 1", Ok(vec![row(vec![])]));
    assert!(Arc::ptr_eq(
        &first,
        &collector.ensure_connected().await.unwrap()
    ));
    queue(&state, "one", "SELECT 1", Err(error()));
    let next = Arc::new(Mutex::new(sdk::State::default()));
    sdk::next(Ok((next.clone(), Ok(()))));
    let reconnect = collector.ensure_connected().await.unwrap();
    assert!(!Arc::ptr_eq(&first, &reconnect));
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.contains("Postgres connection error")));
    sdk::CONFIGS.with(|c| {
        let c = c.borrow();
        let config = c.last().unwrap();
        assert_eq!(config.get_ports(), [5432]);
        assert_eq!(config.get_user(), Some("code-monitor"));
        assert_eq!(config.get_options(), Some("-c statement_timeout=10s"));
    });
    let (collector, _, _) = fixture();
    sdk::next(Err(error()));
    assert!(collector
        .connect()
        .await
        .unwrap_err()
        .to_string()
        .contains("Failed to connect to Postgres cluster 'Unit' at fixture.invalid:5432"));
}
#[tokio::test]
async fn rows_decode_database_activity_connections_cache_and_query_fallbacks() {
    use sdk::cell as c;
    let (collector, client, state) = fixture();
    queue(
        &state,
        "query",
        "FROM pg_stat_database",
        Ok(vec![
            row(vec![
                c("app".to_string()),
                c(20i64),
                c(3i32),
                c(98f64),
                c(7i64),
                c(Some(crate::unit_sysinfo::now())),
            ]),
            row(vec![None; 6]),
        ]),
    );
    let values = collector.collect_databases(&client).await.unwrap();
    assert_eq!(values[0].name, "app");
    assert_eq!(values[0].transactions, 7);
    assert!(values[0].stats_reset_at.is_some());
    assert_eq!(values[1].cache_hit_ratio, 100.);
    queue(
        &state,
        "query",
        "GROUP BY state",
        Ok(vec![
            row(vec![c("active".to_string()), c(2i64)]),
            row(vec![None, None]),
        ]),
    );
    let (total, values) = collector.collect_connections(&client).await.unwrap();
    assert_eq!(total, 2);
    assert_eq!(values[0].state, "active");
    assert_eq!(values[1].count, 0);
    for cell in [c(98f64), None] {
        queue(
            &state,
            "one",
            "FROM pg_stat_database",
            Ok(vec![row(vec![cell])]),
        );
        let value = collector.collect_cache_hit_ratio(&client).await.unwrap();
        assert!([98., 100.].contains(&value));
    }
    for reply in [
        Ok(vec![row(vec![c(false)])]),
        Ok(vec![row(vec![None])]),
        Err(error()),
    ] {
        queue(&state, "one", "FROM pg_extension", reply);
        assert!(collector
            .collect_top_queries(&client)
            .await
            .unwrap()
            .is_empty());
    }
    queue(
        &state,
        "one",
        "FROM pg_extension",
        Ok(vec![row(vec![c(true)])]),
    );
    queue(
        &state,
        "query",
        "FROM pg_stat_statements",
        Ok(vec![
            row(vec![c("SELECT 1".to_string()), c(4i64), c(8f64), c(2f64)]),
            row(vec![None; 4]),
        ]),
    );
    let values = collector.collect_top_queries(&client).await.unwrap();
    assert_eq!(values[0].calls, 4);
    assert_eq!(values[0].mean_exec_time_ms, 2.);
    assert_eq!(values[1].query, "");
}
#[tokio::test]
async fn settings_filter_empty_metadata_and_query_errors_are_best_effort() {
    use sdk::cell as c;
    let (collector, client, state) = fixture();
    queue(
        &state,
        "query",
        "FROM pg_settings",
        Ok(vec![
            row(vec![
                c("work_mem".to_string()),
                c("8".to_string()),
                c(Some("MB".to_string())),
                c("configuration file".to_string()),
                c(Some("unit.conf".to_string())),
                c(Some(3i32)),
            ]),
            row(vec![
                c("max_connections".to_string()),
                c("100".to_string()),
                c(Some("".to_string())),
                c("default".to_string()),
                c(Some("".to_string())),
                c(None::<i32>),
            ]),
        ]),
    );
    let settings = collector.collect_settings(&client).await;
    assert_eq!(settings[0].unit.as_deref(), Some("MB"));
    assert_eq!(settings[0].source_file.as_deref(), Some("unit.conf"));
    assert!(settings[1].unit.is_none());
    assert!(settings[1].source_file.is_none());
    let params = state.lock().unwrap().calls[0].1.clone();
    for name in TRACKED_SETTINGS {
        assert!(params[0].contains(name));
    }
    queue(&state, "query", "FROM pg_settings", Err(error()));
    assert!(collector.collect_settings(&client).await.is_empty());
}
#[tokio::test]
async fn collection_keeps_metadata_and_propagates_each_required_query_failure() {
    use sdk::cell as c;
    for fail in 0..5 {
        let (collector, client, state) = fixture();
        *collector.client.lock().await = Some(Arc::new(client));
        queue(&state, "one", "SELECT 1", Ok(vec![row(vec![])]));
        let queries = [
            ("query", "FROM pg_stat_database", vec![]),
            ("query", "GROUP BY state", vec![]),
            ("one", "FROM pg_stat_database", vec![row(vec![c(99f64)])]),
            ("one", "FROM pg_extension", vec![row(vec![c(false)])]),
            ("query", "FROM pg_settings", vec![]),
        ];
        for (i, (method, sql, rows)) in queries.into_iter().enumerate() {
            if i == fail && i < 3 {
                queue(&state, method, sql, Err(error()));
                break;
            }
            queue(&state, method, sql, Ok(rows));
        }
        let result = collector.collect().await;
        if fail < 3 {
            assert!(result.is_err());
        } else {
            let value = result.unwrap();
            assert_eq!(value.name, "Unit");
            assert_eq!(value.host, "fixture.invalid");
            assert_eq!(value.port, 5432);
            assert_eq!(value.cache_hit_ratio, 99.);
        }
        assert!(state.lock().unwrap().expected.is_empty());
    }
    let (collector, client, state) = fixture();
    queue(
        &state,
        "one",
        "FROM pg_extension",
        Ok(vec![row(vec![c(true)])]),
    );
    queue(&state, "query", "FROM pg_stat_statements", Err(error()));
    assert_eq!(
        collector
            .collect_top_queries(&client)
            .await
            .unwrap_err()
            .to_string(),
        "Failed to query top statements"
    );
    let (collector, _, _) = fixture();
    sdk::next(Err(error()));
    assert!(collector.collect().await.is_err());
}

#[tokio::test]
async fn failed_top_query_collection_propagates_through_cluster_collection() {
    use sdk::cell as c;
    let (collector, client, state) = fixture();
    *collector.client.lock().await = Some(Arc::new(client));
    queue(&state, "one", "SELECT 1", Ok(vec![row(vec![])]));
    queue(&state, "query", "FROM pg_stat_database", Ok(vec![]));
    queue(&state, "query", "GROUP BY state", Ok(vec![]));
    queue(
        &state,
        "one",
        "FROM pg_stat_database",
        Ok(vec![row(vec![c(99f64)])]),
    );
    queue(
        &state,
        "one",
        "FROM pg_extension",
        Ok(vec![row(vec![c(true)])]),
    );
    queue(&state, "query", "FROM pg_stat_statements", Err(error()));
    assert_eq!(
        collector.collect().await.unwrap_err().to_string(),
        "Failed to query top statements"
    );
    assert!(state.lock().unwrap().expected.is_empty());
}
