use super::*;
use crate::unit_grpc::{self as sdk, State};
use shared::notifications::NotificationConfig;
use shared::proto::monitoring as wire;
use std::sync::{Arc, Mutex};
pub(crate) fn queue_poll(state: &sdk::SharedState, detailed: bool, errors: bool) {
    let mut state = state.lock().unwrap();
    let restart_count = 9 + (state.calls.len() / 10) as u32 * 4;
    if errors {
        for method in [
            "get_system_info",
            "get_services",
            "get_processes",
            "get_network_info",
            "get_containers",
            "get_postgres_info",
            "get_maria_db_info",
            "get_systemd_info",
            "get_tls_info",
            "get_listening_ports",
        ] {
            state.reject(method);
        }
        return;
    }
    state.respond(
        "get_system_info",
        wire::SystemInfoResponse {
            hostname: "dashboard-unit".into(),
            cpu_usage_percent: 95.,
            memory_total_bytes: 100,
            memory_used_bytes: 95,
            psi_memory_some_avg60: Some(5.),
            psi_memory_full_avg60: Some(10.),
            psi_memory_full_total_seconds: Some(100.),
            swap_in_pages_per_sec: 300.,
            disk_info: if detailed {
                vec![wire::DiskInfo {
                    device: "disk".into(),
                    total_bytes: 100,
                    used_bytes: 99,
                    ..Default::default()
                }]
            } else {
                vec![]
            },
            ..Default::default()
        },
    );
    state.respond(
        "get_services",
        wire::ServicesResponse {
            services: vec![wire::ServiceInfo {
                name: "worker".into(),
                status: "Running".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    state.respond(
        "get_processes",
        wire::ProcessesResponse {
            processes: vec![wire::ProcessInfo {
                name: "unit-process".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    state.respond(
        "get_network_info",
        wire::NetworkInfoResponse {
            interfaces: vec![wire::NetworkInfo {
                interface: "fixture".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    state.respond(
        "get_containers",
        wire::ContainersResponse {
            containers: vec![
                wire::ContainerInfo {
                    id: "container-unit".into(),
                    name: "container-unit".into(),
                    restart_count,
                    memory_limit_bytes: 500,
                    memory_limit_set: true,
                    failing_streak: 3,
                    last_health_output: "executable file not found".into(),
                    last_health_exit_code: 127,
                    ..Default::default()
                },
                wire::ContainerInfo::default(),
            ],
            ..Default::default()
        },
    );
    state.respond(
        "get_postgres_info",
        wire::PostgresInfoResponse {
            clusters: vec![wire::PostgresClusterInfo {
                name: "pg-unit".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    state.respond(
        "get_maria_db_info",
        wire::MariaDbInfoResponse {
            clusters: vec![wire::MariaDbClusterInfo {
                name: "maria-unit".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    state.respond(
        "get_systemd_info",
        wire::SystemdInfoResponse {
            units: vec![wire::SystemdUnitInfo {
                name: "unit.service".into(),
                ..Default::default()
            }],
            failed_units: vec![wire::SystemdFailedUnit {
                name: "broken.service".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    state.respond(
        "get_tls_info",
        wire::TlsInfoResponse {
            certificates: vec![wire::TlsCertificateInfo {
                name: "unit.invalid".into(),
                days_until_expiry: 1,
                ..Default::default()
            }],
            renewal_unit_name: Some("renew.timer".into()),
            renewal_unit_status: Some("failed".into()),
            ..Default::default()
        },
    );
    state.respond(
        "get_listening_ports",
        wire::ListeningPortsResponse {
            ports: vec![
                wire::ListeningPortInfo {
                    address: "0.0.0.0".into(),
                    port: 5432,
                    bind_scope: 3,
                    sensitive: true,
                    process_name: "postgres".into(),
                    ..Default::default()
                },
                wire::ListeningPortInfo {
                    address: "0.0.0.0".into(),
                    port: 3306,
                    bind_scope: 3,
                    sensitive: true,
                    ..Default::default()
                },
                wire::ListeningPortInfo::default(),
            ],
            ..Default::default()
        },
    );
}
#[tokio::test]
async fn successful_connect_populates_every_cache_and_repeated_fetches_evaluate_alerts() {
    let server = ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: "Unit server".into(),
        address: "fixture.invalid".into(),
        port: 1,
        description: None,
        access_token: Some("unit-token".into()),
    };
    let id = server.id;
    let mut app = DashboardApp::new(vec![server], NotificationConfig::default(), None);
    assert!(
        app.storage.is_none(),
        "unconfigured SQLite must fail closed"
    );
    let state = Arc::new(Mutex::new(State::default()));
    queue_poll(&state, true, false);
    sdk::connect_next(state.clone());
    handle_command(&mut app, "connect").await;
    assert_eq!(app.status_message, "Connected to Unit server");
    assert!(app.connected_clients.contains_key(&id));
    assert!(matches!(
        app.connection_status.get(&id),
        Some(ConnectionStatus::Connected)
    ));
    assert_eq!(app.system_info_cache[&id].hostname, "dashboard-unit");
    assert_eq!(app.services_cache[&id][0].name, "worker");
    assert_eq!(app.processes_cache[&id][0].name, "unit-process");
    assert_eq!(app.network_cache[&id][0].interface, "fixture");
    assert_eq!(app.containers_cache[&id][0].name, "container-unit");
    assert_eq!(app.postgres_cache[&id][0].name, "pg-unit");
    assert_eq!(app.mariadb_cache[&id][0].name, "maria-unit");
    assert_eq!(app.systemd_cache[&id][0].name, "unit.service");
    assert_eq!(app.systemd_failed_cache[&id][0].name, "broken.service");
    assert_eq!(app.tls_cache[&id].certificates[0].name, "unit.invalid");
    assert_eq!(app.ports_cache[&id].len(), 3);
    for _ in 0..3 {
        queue_poll(&state, true, false);
        fetch_server_data(&mut app, id).await;
    }
    assert_eq!(app.cpu_history[&id].len(), 4);
    for kind in [
        shared::alerts::AlertType::SwapThrashing,
        shared::alerts::AlertType::MemoryStalled,
        shared::alerts::AlertType::ContainerCrashLoop,
    ] {
        assert!(
            app.alert_manager
                .get_active_alerts()
                .iter()
                .any(|alert| alert.alert_type == kind),
            "missing {kind:?}"
        );
    }
    app.servers.clear();
    queue_poll(&state, false, false);
    fetch_server_data(&mut app, id).await;
    assert!(app.system_info_cache[&id].disk_info.is_empty());
    queue_poll(&state, false, true);
    fetch_server_data(&mut app, id).await;
    assert_eq!(app.system_info_cache[&id].hostname, "dashboard-unit");
    assert!(state.lock().unwrap().responses.is_empty());
}

#[tokio::test]
async fn storage_failures_are_logged_without_losing_the_fetched_snapshot() {
    let (_guard, logs) = crate::unit_logs::capture();
    let server = ServerEndpoint {
        id: uuid::Uuid::new_v4(),
        name: "Stored unit".into(),
        address: "fixture.invalid".into(),
        port: 1,
        description: None,
        access_token: None,
    };
    let id = server.id;
    let mut app = DashboardApp::new(vec![server], NotificationConfig::default(), None);
    let (storage, database) = crate::storage::campaign_storage_units::fixture();
    app.storage = Some(storage);
    let state = Arc::new(Mutex::new(State::default()));
    sdk::connect_next(state.clone());
    let client = MonitorClient::connect("http://fixture.invalid:1", 5, false)
        .await
        .unwrap();
    app.connected_clients.insert(id, client);
    for reply in [
        crate::unit_sqlite::Reply::Error,
        crate::unit_sqlite::Reply::Execute(1),
    ] {
        database.lock().unwrap().expected.push_back(crate::unit_sqlite::Expected {sql:"INSERT INTO metrics (server_id, timestamp, cpu_usage, memory_used, memory_total) VALUES (?1, ?2, ?3, ?4, ?5)",parameters:vec![rusqlite::types::Value::Text(id.to_string()),rusqlite::types::Value::Integer(1_700_000_000),rusqlite::types::Value::Real(95.),rusqlite::types::Value::Integer(95),rusqlite::types::Value::Integer(100)],reply});
        queue_poll(&state, true, false);
        fetch_server_data(&mut app, id).await;
        assert_eq!(app.system_info_cache[&id].hostname, "dashboard-unit");
    }
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("Failed to store metrics")));
    assert!(database.lock().unwrap().expected.is_empty());
}

#[tokio::test]
async fn connection_disconnect_and_refresh_without_selection_leave_status_unchanged() {
    let mut app = DashboardApp::new(vec![], NotificationConfig::default(), None);
    app.status_message = "unit unchanged".into();
    for command in ["connect", "disconnect", "refresh"] {
        handle_command(&mut app, command).await;
        assert_eq!(app.status_message, "unit unchanged");
        assert!(app.connected_clients.is_empty());
    }
}
