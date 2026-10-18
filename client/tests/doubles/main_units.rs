//! CLI behavior with owned configuration files and scripted user/transport input.
use super::*;
use crate::{unit_console as console, unit_grpc as sdk};
use shared::proto::monitoring as wire;
use std::sync::{Arc, Mutex};
fn manager() -> (ClientConfigManager, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    (
        ClientConfigManager::new(&dir.path().join("client.toml")).unwrap(),
        dir,
    )
}
fn server() -> ServerEndpoint {
    ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: "Unit".into(),
        address: "fixture.invalid".into(),
        port: 1,
        description: None,
        access_token: None,
    }
}
fn connection(reply: bool) {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    if reply {
        state.lock().unwrap().respond(
            "get_system_info",
            wire::SystemInfoResponse {
                hostname: "unit-host".into(),
                os: "UnitOS".into(),
                ..Default::default()
            },
        );
    } else {
        state.lock().unwrap().reject("get_system_info");
    }
    sdk::connect_next(state);
}
#[test]
fn prompts_preserve_defaults_validate_and_propagate_io_failures() {
    console::lines(&["", " text "]);
    assert_eq!(prompt_string("Name", "default").unwrap(), "default");
    assert_eq!(prompt_string("Name", "").unwrap(), "text");
    console::lines(&["", " ", "unit"]);
    assert_eq!(prompt_required_string("Required").unwrap(), "unit");
    for (answers, default, expected) in [
        (&["invalid", "Y"][..], false, true),
        (&["no"][..], true, false),
        (&[""][..], true, true),
        (&[""][..], false, false),
    ] {
        console::lines(answers);
        assert_eq!(prompt_bool("Enabled", default).unwrap(), expected);
    }
    console::lines(&["invalid", "8"]);
    assert_eq!(prompt_u64("Count", 3).unwrap(), 8);
    console::lines(&["999999", "50051"]);
    assert_eq!(prompt_u16("Port", 1).unwrap(), 50051);
    console::script(vec![], Some(1));
    assert!(prompt_string("Name", "")
        .unwrap_err()
        .to_string()
        .contains("flush refused"));
    console::script(vec![Err(std::io::Error::other("unit stdin refused"))], None);
    assert!(prompt_string("Name", "")
        .unwrap_err()
        .to_string()
        .contains("stdin refused"));
    assert_eq!(blank_to_none(" \n".into()), None);
    assert_eq!(blank_to_none(" token ".into()), Some("token".into()));
}
#[tokio::test]
async fn prompt_server_and_connection_test_cover_optional_values_and_rpc_failures() {
    console::lines(&[
        "Unit",
        "fixture.invalid",
        "50051",
        " token ",
        " description ",
    ]);
    let endpoint = prompt_server().await.unwrap();
    assert_eq!(endpoint.access_token.as_deref(), Some("token"));
    assert_eq!(endpoint.description.as_deref(), Some("description"));
    for success in [false, true] {
        connection(success);
        test_server_connection(&endpoint, None).await;
    }
    sdk::reject_next();
    test_server_connection(&endpoint, None).await;
    let tls = ClientTlsConfig {
        ca_cert_path: "/unit-missing-ca".into(),
        client_cert_path: None,
        client_key_path: None,
        danger_skip_verify: false,
    };
    test_server_connection(&endpoint, Some(&tls)).await;
    console::lines(&["Unit", "fixture.invalid", "", "", ""]);
    let endpoint = prompt_server().await.unwrap();
    assert!(endpoint.access_token.is_none());
    assert!(endpoint.description.is_none());
}
#[tokio::test]
async fn setup_replaces_endpoint_preserves_others_and_clears_only_when_forced() {
    let (mut manager, _dir) = manager();
    let existing = server();
    manager.add_server(existing.clone()).unwrap();
    console::lines(&["7", "no", "4", "no"]);
    init_client(&mut manager, false).await.unwrap();
    let config = manager.load_config().unwrap();
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.update_interval_seconds, 7);
    assert!(!config.auto_reconnect);
    console::lines(&[
        "",
        "",
        "",
        "yes",
        "Changed",
        "fixture.invalid",
        "1",
        "",
        "",
        "no",
    ]);
    init_client(&mut manager, false).await.unwrap();
    let config = manager.load_config().unwrap();
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.servers[0].name, "Changed");
    console::lines(&["", "", "", "Forced", "fixture.invalid", "2", "", "", "yes"]);
    connection(true);
    init_client(&mut manager, true).await.unwrap();
    let config = manager.load_config().unwrap();
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.servers[0].port, 2);
}
#[tokio::test]
async fn quick_connect_saves_only_after_success_or_explicit_yes() {
    for (connect_ok, rpc_ok, save) in [
        (true, true, false),
        (true, false, true),
        (true, false, false),
        (false, false, true),
        (false, false, false),
    ] {
        let (mut manager, _dir) = manager();
        console::lines(&["", "", if save { "y" } else { "n" }]);
        if connect_ok {
            connection(rpc_ok);
        } else {
            sdk::reject_next();
        }
        quick_connect(&mut manager, "fixture.invalid".into(), 1)
            .await
            .unwrap();
        let config = manager.load_config().unwrap();
        assert_eq!(
            config.servers.len(),
            usize::from((connect_ok && rpc_ok) || save)
        );
        if !config.servers.is_empty() {
            assert_eq!(config.servers[0].name, "Server-fixture.invalid");
        }
    }
    let (mut manager, _dir) = manager();
    console::lines(&["Named", "token"]);
    connection(true);
    quick_connect(&mut manager, "fixture.invalid".into(), 1)
        .await
        .unwrap();
    assert_eq!(
        manager.load_config().unwrap().servers[0]
            .access_token
            .as_deref(),
        Some("token")
    );
}
#[test]
fn entry_point_dispatches_real_commands_and_maps_all_log_levels() {
    let (manager, dir) = manager();
    let path = manager.config_path().to_str().unwrap().to_owned();
    for (level, expected) in [
        ("trace", tracing::Level::TRACE),
        ("debug", tracing::Level::DEBUG),
        ("info", tracing::Level::INFO),
        ("warn", tracing::Level::WARN),
        ("error", tracing::Level::ERROR),
        ("other", tracing::Level::INFO),
    ] {
        crate::unit_bootstrap::next(
            CliArgs::try_parse_from([
                "monitor-client",
                "--config",
                &path,
                "--log-level",
                level,
                "list",
            ])
            .unwrap(),
        );
        main().unwrap();
        crate::unit_bootstrap::logging::LEVELS
            .with(|l| assert_eq!(l.borrow().last(), Some(&expected)));
    }
    // Invalid config fails before dispatch, and is preserved for diagnosis.
    let broken = dir.path().join("broken.toml");
    std::fs::write(&broken, "invalid = [").unwrap();
    crate::unit_bootstrap::next(
        CliArgs::try_parse_from([
            "monitor-client",
            "--config",
            broken.to_str().unwrap(),
            "list",
        ])
        .unwrap(),
    );
    assert!(main().is_err());
    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "invalid = [");
}
#[tokio::test]
async fn service_and_process_listing_cover_selection_rows_limits_and_failures() {
    let (mut manager, _dir) = manager();
    let endpoint = server();
    let id = endpoint.id;
    list_services(&manager, None, 2).await.unwrap();
    list_processes(&manager, Some(id), 2, None).await.unwrap();
    manager.add_server(endpoint).unwrap();
    for specific in [false, true] {
        let state = Arc::new(Mutex::new(sdk::State::default()));
        state.lock().unwrap().respond(
            "get_services",
            wire::ServicesResponse {
                services: ["Running", "Stopped", "Failed", "other"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, status)| wire::ServiceInfo {
                        name: format!("unit-{i}"),
                        status: status.into(),
                        pid: if i == 0 { 7 } else { 0 },
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
        );
        sdk::connect_next(state);
        list_services(
            &manager,
            if specific { Some(id) } else { None },
            if specific { 2 } else { 10 },
        )
        .await
        .unwrap();
        let state = Arc::new(Mutex::new(sdk::State::default()));
        state.lock().unwrap().respond(
            "get_processes",
            wire::ProcessesResponse {
                processes: vec![wire::ProcessInfo {
                    pid: 7,
                    name: "unit-process".into(),
                    status: "Running".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        sdk::connect_next(state.clone());
        list_processes(
            &manager,
            if specific { Some(id) } else { None },
            5,
            if specific { Some("unit".into()) } else { None },
        )
        .await
        .unwrap();
        assert_eq!(state.lock().unwrap().calls[0].method, "get_processes");
    }
    for method in ["get_services", "get_processes"] {
        let state = Arc::new(Mutex::new(sdk::State::default()));
        state.lock().unwrap().reject(method);
        sdk::connect_next(state);
        let result = if method == "get_services" {
            list_services(&manager, None, 1).await
        } else {
            list_processes(&manager, None, 1, None).await
        };
        assert!(result.is_err());
    }
    sdk::reject_next();
    assert!(list_services(&manager, None, 1).await.is_err());
    sdk::reject_next();
    assert!(list_processes(&manager, None, 1, None).await.is_err());
    list_services(&manager, Some(uuid::Uuid::new_v4()), 1)
        .await
        .unwrap();
    list_processes(&manager, Some(uuid::Uuid::new_v4()), 1, None)
        .await
        .unwrap();
}
fn queue_storage() -> crate::unit_sqlite::SharedState {
    use crate::unit_sqlite::{self as sql, Expected, Reply};
    let state = Arc::new(Mutex::new(sql::State::default()));
    for query in crate::storage::campaign_storage_units::SETUP {
        state.lock().unwrap().expected.push_back(Expected {
            sql: query,
            parameters: vec![],
            reply: Reply::Execute(0),
        });
    }
    sql::next_open(Ok(state.clone()));
    state
}
fn history(state: &crate::unit_sqlite::SharedState, id: uuid::Uuid, rows: bool) {
    use crate::unit_sqlite::{Expected, Reply};
    use rusqlite::types::Value;
    state.lock().unwrap().expected.push_back(Expected {
        sql: crate::storage::campaign_storage_units::HISTORY,
        parameters: vec![
            Value::Text(id.to_string()),
            Value::Integer(1_700_000_000 - 24 * 3600),
        ],
        reply: Reply::Rows(if rows {
            vec![vec![
                Value::Integer(1),
                Value::Text(id.to_string()),
                Value::Integer(1_700_000_000),
                Value::Real(25.),
                Value::Integer(50),
                Value::Integer(100),
            ]]
        } else {
            vec![]
        }),
    });
}
#[tokio::test]
async fn export_commands_write_real_csv_json_and_preserve_provider_errors() {
    let (mut manager, dir) = manager();
    let endpoint = server();
    let id = endpoint.id;
    manager.add_server(endpoint).unwrap();
    for format in ["csv", "json"] {
        let state = queue_storage();
        history(&state, id, true);
        let output = dir.path().join(format!("unit.{format}"));
        export_metrics(&manager, None, format.into(), output.clone(), 24)
            .await
            .unwrap();
        let content = std::fs::read_to_string(output).unwrap();
        assert!(content.contains(&id.to_string()));
        assert!(content.contains("25"));
        assert!(state.lock().unwrap().expected.is_empty());
    }
    let state = queue_storage();
    export_metrics(
        &manager,
        Some(id),
        "other".into(),
        dir.path().join("unused"),
        24,
    )
    .await
    .unwrap();
    assert!(state.lock().unwrap().expected.is_empty());
    for fault in [
        crate::unit_files::Fault::CsvHeader,
        crate::unit_files::Fault::CsvRow,
        crate::unit_files::Fault::CsvFlush,
        crate::unit_files::Fault::JsonWrite,
    ] {
        let _fault = crate::unit_files::Guard::arm(fault);
        let state = queue_storage();
        if fault != crate::unit_files::Fault::CsvHeader {
            history(&state, id, true);
        }
        let format = if fault == crate::unit_files::Fault::JsonWrite {
            "json"
        } else {
            "csv"
        };
        assert!(export_metrics(
            &manager,
            Some(id),
            format.into(),
            dir.path().join("failure"),
            24
        )
        .await
        .is_err());
    }
    let _state = queue_storage();
    assert!(export_metrics(
        &manager,
        None,
        "csv".into(),
        dir.path().join("missing/failed.csv"),
        24
    )
    .await
    .is_err());
    manager.remove_server(id).unwrap();
    let state = queue_storage();
    export_metrics(&manager, None, "csv".into(), dir.path().join("empty"), 24)
        .await
        .unwrap();
    assert!(state.lock().unwrap().expected.is_empty());
    assert!(
        export_metrics(&manager, None, "csv".into(), dir.path().join("never"), 24)
            .await
            .is_err(),
        "unconfigured SQLite open fails closed"
    );
}
#[tokio::test]
async fn statistics_and_purge_use_scripted_sql_and_surface_failures() {
    use crate::unit_sqlite::{Expected, Reply};
    use rusqlite::types::Value;
    for present in [false, true] {
        let state = queue_storage();
        for (i, query) in crate::storage::campaign_storage_units::STATS
            .into_iter()
            .enumerate()
        {
            state.lock().unwrap().expected.push_back(Expected {
                sql: query,
                parameters: vec![],
                reply: Reply::Rows(vec![vec![if i == 1 || i == 2 {
                    if present {
                        Value::Integer(1_700_000_000)
                    } else {
                        Value::Null
                    }
                } else {
                    Value::Integer(3)
                }]]),
            });
        }
        show_storage_stats().await.unwrap();
        assert!(state.lock().unwrap().expected.is_empty());
    }
    let state = queue_storage();
    state.lock().unwrap().expected.push_back(Expected {
        sql: "DELETE FROM metrics WHERE timestamp < ?1",
        parameters: vec![Value::Integer(1_700_000_000 - 7 * 86400)],
        reply: Reply::Execute(9),
    });
    state.lock().unwrap().expected.push_back(Expected {
        sql: "VACUUM",
        parameters: vec![],
        reply: Reply::Execute(0),
    });
    purge_storage(7).await.unwrap();
    assert!(state.lock().unwrap().expected.is_empty());
    assert!(show_storage_stats().await.is_err());
    assert!(purge_storage(7).await.is_err());
}
#[tokio::test]
async fn monitoring_loop_samples_formats_and_reconnects_without_timers() {
    use std::{
        future::Future,
        task::{Context, Poll},
    };
    let (mut manager, _dir) = manager();
    let endpoint = server();
    let id = endpoint.id;
    manager.add_server(endpoint.clone()).unwrap();
    for format in [cli::MonitorFormat::Text, cli::MonitorFormat::Json] {
        let state = Arc::new(Mutex::new(sdk::State::default()));
        for _ in 0..2 {
            state.lock().unwrap().respond(
                "get_system_info",
                wire::SystemInfoResponse {
                    hostname: "unit-host".into(),
                    memory_total_bytes: 100,
                    memory_used_bytes: 50,
                    disk_info: vec![wire::DiskInfo {
                        device: "unit-disk".into(),
                        total_bytes: 100,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            );
        }
        sdk::connect_next(state.clone());
        crate::unit_bootstrap::timer(1);
        {
            let mut future = std::pin::pin!(monitor_server(&manager, Some(id), 3, format));
            assert!(matches!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(std::task::Waker::noop())),
                Poll::Pending
            ));
        }
        assert_eq!(state.lock().unwrap().calls.len(), 2);
        crate::unit_bootstrap::SLEEPS.with(|s| {
            assert_eq!(
                *s.borrow(),
                [Duration::from_secs(3), Duration::from_secs(3)]
            )
        });
    }
    for (auto, reconnected) in [(false, false), (true, false), (true, true)] {
        let state = Arc::new(Mutex::new(sdk::State::default()));
        state.lock().unwrap().reject("get_system_info");
        sdk::connect_next(state.clone());
        let client = MonitorClient::connect("http://fixture.invalid:1", 3, auto)
            .await
            .unwrap();
        if auto {
            if reconnected {
                sdk::connect_next(Arc::new(Mutex::new(sdk::State::default())));
            } else {
                sdk::reject_next();
            }
        }
        crate::unit_bootstrap::timer(0);
        let clients = HashMap::from([(id, (endpoint.clone(), client))]);
        let mut future =
            std::pin::pin!(start_monitoring_loop(clients, 3, cli::MonitorFormat::Text));
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(std::task::Waker::noop())),
            Poll::Pending
        ));
        assert_eq!(state.lock().unwrap().calls.len(), 1);
    }
    monitor_server(
        &manager,
        Some(uuid::Uuid::new_v4()),
        3,
        cli::MonitorFormat::Text,
    )
    .await
    .unwrap();
    sdk::reject_next();
    assert!(monitor_server(&manager, None, 3, cli::MonitorFormat::Text)
        .await
        .is_err());
}
#[test]
fn bootstrap_dispatches_each_command_with_owned_config_and_providers() {
    use crate::unit_sqlite::{Expected, Reply};
    use rusqlite::types::Value;
    let (manager, dir) = manager();
    let path = manager.config_path().to_owned();
    let invoke = |command| {
        crate::unit_bootstrap::next(CliArgs {
            config: path.clone(),
            log_level: "info".into(),
            command,
        });
        main()
    };
    invoke(Some(cli::Commands::Monitor {
        server: None,
        interval: 5,
        format: cli::MonitorFormat::Text,
    }))
    .unwrap();
    invoke(Some(cli::Commands::Serve {
        address: "fixture.invalid".into(),
        port: 1,
    }))
    .unwrap();
    invoke(Some(cli::Commands::Services {
        server: None,
        limit: 2,
    }))
    .unwrap();
    invoke(Some(cli::Commands::Processes {
        server: None,
        limit: 2,
        filter: None,
    }))
    .unwrap();
    let state = queue_storage();
    invoke(Some(cli::Commands::Export {
        server: None,
        format: "csv".into(),
        output: dir.path().join("empty.csv"),
        hours: 24,
    }))
    .unwrap();
    assert!(state.lock().unwrap().expected.is_empty());
    for command in [None, Some(cli::Commands::Dashboard)] {
        dashboard::unit_terminal::reset(None);
        dashboard::unit_events::script(vec![(
            100,
            Ok(Some(crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('q'),
                    crossterm::event::KeyModifiers::NONE,
                ),
            ))),
        )]);
        invoke(command).unwrap();
    }
    console::lines(&["", "", "", "Initial", "fixture.invalid", "1", "", "", "no"]);
    invoke(Some(cli::Commands::Init { force: false })).unwrap();
    let id = manager.load_config().unwrap().servers[0].id;
    invoke(Some(cli::Commands::SetToken {
        id,
        token: "unit-token".into(),
    }))
    .unwrap();
    assert_eq!(
        manager.load_config().unwrap().servers[0]
            .access_token
            .as_deref(),
        Some("unit-token")
    );
    invoke(Some(cli::Commands::Add {
        name: "Second".into(),
        address: "other.invalid".into(),
        port: 2,
        token: None,
        description: None,
    }))
    .unwrap();
    invoke(Some(cli::Commands::Remove { id })).unwrap();
    assert_eq!(manager.load_config().unwrap().servers.len(), 1);
    console::lines(&["Quick", "unit-token"]);
    connection(true);
    invoke(Some(cli::Commands::Connect {
        address: "quick.invalid".into(),
        port: 3,
    }))
    .unwrap();
    assert_eq!(manager.load_config().unwrap().servers.len(), 2);
    let state = queue_storage();
    for query in crate::storage::campaign_storage_units::STATS {
        state.lock().unwrap().expected.push_back(Expected {
            sql: query,
            parameters: vec![],
            reply: Reply::Rows(vec![vec![Value::Integer(1_700_000_000)]]),
        });
    }
    invoke(Some(cli::Commands::StorageStats)).unwrap();
    assert!(state.lock().unwrap().expected.is_empty());
    let state = queue_storage();
    state.lock().unwrap().expected.push_back(Expected {
        sql: "DELETE FROM metrics WHERE timestamp < ?1",
        parameters: vec![Value::Integer(1_700_000_000 - 7 * 86400)],
        reply: Reply::Execute(2),
    });
    state.lock().unwrap().expected.push_back(Expected {
        sql: "VACUUM",
        parameters: vec![],
        reply: Reply::Execute(0),
    });
    invoke(Some(cli::Commands::Purge { days: 7 })).unwrap();
    assert!(state.lock().unwrap().expected.is_empty());
}
#[tokio::test]
async fn every_interactive_input_stage_propagates_its_io_failure() {
    for stage in 1..=5 {
        console::script(
            ["Unit", "fixture.invalid", "1", "token", "description"]
                .into_iter()
                .map(|s| Ok(s.into()))
                .collect(),
            Some(stage),
        );
        assert!(
            prompt_server().await.is_err(),
            "server prompt stage {stage}"
        );
    }
    for stage in 1..=10 {
        let (mut manager, _dir) = manager();
        console::script(
            [
                "5",
                "yes",
                "5",
                "Unit",
                "fixture.invalid",
                "1",
                "token",
                "description",
                "no",
            ]
            .into_iter()
            .map(|s| Ok(s.into()))
            .collect(),
            Some(stage),
        );
        assert!(
            init_client(&mut manager, false).await.is_err(),
            "setup stage {stage}"
        );
    }
    for kind in 0..4 {
        console::script(
            vec![Err(std::io::Error::other("unit stdin stage refused"))],
            None,
        );
        let result = match kind {
            0 => prompt_required_string("required").map(|_| ()),
            1 => prompt_bool("boolean", true).map(|_| ()),
            2 => prompt_u64("number", 5).map(|_| ()),
            _ => prompt_u16("port", 1).map(|_| ()),
        };
        assert!(result.is_err());
    }
    for connected in [false, true] {
        for stage in 1..=3 {
            let (mut manager, _dir) = manager();
            console::script(
                ["Unit", "token", "y"]
                    .into_iter()
                    .map(|s| Ok(s.into()))
                    .collect(),
                Some(stage),
            );
            if stage == 3 {
                if connected {
                    connection(false);
                } else {
                    sdk::reject_next();
                }
            }
            assert!(quick_connect(&mut manager, "fixture.invalid".into(), 1)
                .await
                .is_err());
        }
        for stage in 0..3 {
            let (mut manager, _dir) = manager();
            let mut input = vec![Ok("Unit".into()), Ok("token".into()), Ok("y".into())];
            input[stage] = Err(std::io::Error::other("unit read failed"));
            console::script(input, None);
            if stage == 2 {
                if connected {
                    connection(false);
                } else {
                    sdk::reject_next();
                }
            }
            assert!(quick_connect(&mut manager, "fixture.invalid".into(), 1)
                .await
                .is_err());
        }
    }
}
#[tokio::test]
async fn save_errors_and_invalid_configuration_preserve_existing_files() {
    use crate::config::campaign_path_units::{Fault, FaultGuard};
    let (mut manager, _dir) = manager();
    let endpoint = server();
    let id = endpoint.id;
    manager.add_server(endpoint).unwrap();
    let original = std::fs::read(manager.config_path()).unwrap();
    {
        let _fault = FaultGuard::arm(Fault::Write);
        assert!(set_server_token(&mut manager, id, "changed".into())
            .await
            .is_err());
        console::lines(&["", "", "", "no"]);
        assert!(init_client(&mut manager, false).await.is_err());
    }
    assert_eq!(std::fs::read(manager.config_path()).unwrap(), original);
    for mode in 0..3 {
        console::lines(&["New", "token", "y"]);
        if mode < 2 {
            connection(mode == 0);
        } else {
            sdk::reject_next();
        }
        let _fault = FaultGuard::arm(Fault::Write);
        assert!(quick_connect(&mut manager, "other.invalid".into(), 2)
            .await
            .is_err());
    }
    std::fs::write(manager.config_path(), "invalid = [").unwrap();
    assert!(init_client(&mut manager, false).await.is_err());
    assert!(list_servers(&manager).await.is_err());
    assert!(set_server_token(&mut manager, id, "unit".into())
        .await
        .is_err());
    assert!(list_services(&manager, None, 1).await.is_err());
    assert!(list_processes(&manager, None, 1, None).await.is_err());
    assert!(monitor_server(&manager, None, 5, cli::MonitorFormat::Text)
        .await
        .is_err());
    assert!(run_dashboard(&manager).await.is_err());
    let _state = queue_storage();
    assert!(export_metrics(
        &manager,
        None,
        "csv".into(),
        manager.config_path().with_extension("csv"),
        24
    )
    .await
    .is_err());
}
#[tokio::test]
async fn export_history_and_statistics_query_failures_propagate() {
    use crate::unit_sqlite::{Expected, Reply};
    use rusqlite::types::Value;
    let (mut manager, dir) = manager();
    let endpoint = server();
    let id = endpoint.id;
    manager.add_server(endpoint).unwrap();
    for format in ["csv", "json"] {
        let state = queue_storage();
        state.lock().unwrap().expected.push_back(Expected {
            sql: crate::storage::campaign_storage_units::HISTORY,
            parameters: vec![
                Value::Text(id.to_string()),
                Value::Integer(1_700_000_000 - 24 * 3600),
            ],
            reply: Reply::Error,
        });
        assert!(export_metrics(
            &manager,
            Some(id),
            format.into(),
            dir.path().join("failed-export"),
            24
        )
        .await
        .is_err());
        assert!(state.lock().unwrap().expected.is_empty());
    }
    let state = queue_storage();
    history(&state, id, true);
    assert!(export_metrics(
        &manager,
        Some(id),
        "json".into(),
        dir.path().join("missing/failed.json"),
        24
    )
    .await
    .is_err());
    let state = queue_storage();
    state.lock().unwrap().expected.push_back(Expected {
        sql: crate::storage::campaign_storage_units::STATS[0],
        parameters: vec![],
        reply: Reply::Error,
    });
    assert!(show_storage_stats().await.is_err());
    let state = queue_storage();
    state.lock().unwrap().expected.push_back(Expected {
        sql: "DELETE FROM metrics WHERE timestamp < ?1",
        parameters: vec![Value::Integer(1_700_000_000 - 7 * 86400)],
        reply: Reply::Error,
    });
    assert!(purge_storage(7).await.is_err());
}
#[tokio::test]
async fn list_reports_configured_token_and_description_without_mutating_config() {
    let (mut manager, _dir) = manager();
    let mut endpoint = server();
    endpoint.access_token = Some("unit-token".into());
    endpoint.description = Some("unit description".into());
    manager.add_server(endpoint).unwrap();
    let before = std::fs::read(manager.config_path()).unwrap();
    list_servers(&manager).await.unwrap();
    assert_eq!(std::fs::read(manager.config_path()).unwrap(), before);
}
#[test]
fn bootstrap_propagates_errors_from_every_fallible_command() {
    let (manager, dir) = manager();
    let path = manager.config_path().to_owned();
    std::fs::write(&path, "invalid = [").unwrap();
    let id = uuid::Uuid::new_v4();
    let commands = vec![
        cli::Commands::Init { force: false },
        cli::Commands::Add {
            name: "Unit".into(),
            address: "fixture.invalid".into(),
            port: 1,
            token: None,
            description: None,
        },
        cli::Commands::Remove { id },
        cli::Commands::List,
        cli::Commands::Monitor {
            server: None,
            interval: 1,
            format: cli::MonitorFormat::Text,
        },
        cli::Commands::Dashboard,
        cli::Commands::Services {
            server: None,
            limit: 1,
        },
        cli::Commands::Processes {
            server: None,
            limit: 1,
            filter: None,
        },
        cli::Commands::SetToken {
            id,
            token: "unit-token".into(),
        },
        cli::Commands::Connect {
            address: "fixture.invalid".into(),
            port: 1,
        },
        cli::Commands::Export {
            server: None,
            format: "csv".into(),
            output: dir.path().join("unused"),
            hours: 24,
        },
        cli::Commands::StorageStats,
        cli::Commands::Purge { days: 7 },
    ];
    for command in commands {
        if matches!(command, cli::Commands::Connect { .. }) {
            console::script(vec![Err(std::io::Error::other("unit input refused"))], None);
        }
        if matches!(command, cli::Commands::Export { .. }) {
            queue_storage();
        }
        crate::unit_bootstrap::next(CliArgs {
            config: path.clone(),
            log_level: "info".into(),
            command: Some(command),
        });
        assert!(main().is_err());
    }
    crate::unit_bootstrap::next(CliArgs {
        config: dir.path().join("missing/never.toml"),
        log_level: "info".into(),
        command: Some(cli::Commands::List),
    });
    assert!(main().is_err());
}
#[tokio::test]
async fn existing_setup_confirmation_and_terminal_start_errors_propagate() {
    let (mut manager, _dir) = manager();
    manager.add_server(server()).unwrap();
    console::script(
        ["5", "yes", "5"]
            .into_iter()
            .map(|s| Ok(s.into()))
            .collect(),
        Some(4),
    );
    assert!(init_client(&mut manager, false).await.is_err());
    dashboard::unit_terminal::reset(Some(("enable", 1)));
    assert!(run_dashboard(&manager)
        .await
        .unwrap_err()
        .to_string()
        .contains("enable refused"));
}
