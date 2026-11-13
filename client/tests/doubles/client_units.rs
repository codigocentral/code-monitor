use super::*;
use crate::unit_grpc::{self as sdk, State};
use shared::proto::monitoring as wire;
use std::sync::{Arc, Mutex};
fn timestamp() -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp {
        seconds: 1_700_000_000,
        nanos: 0,
    })
}
fn fixture() -> (MonitorClient, sdk::SharedState) {
    let state = Arc::new(Mutex::new(State::default()));
    (
        MonitorClient {
            channel: Channel(state.clone()),
            update_interval: 7,
            auto_reconnect: false,
            access_token: Some("unit-token".into()),
        },
        state,
    )
}
#[tokio::test]
async fn connection_and_token_paths_use_only_the_configured_transport() {
    assert!(MonitorClient::connect("\0", 1, false)
        .await
        .unwrap_err_display()
        .contains("Invalid server address"));
    sdk::reject_next();
    assert!(MonitorClient::connect("http://fixture.invalid:1", 1, false)
        .await
        .is_err());
    let state = Arc::new(Mutex::new(State::default()));
    sdk::connect_next(state.clone());
    let mut client =
        MonitorClient::connect_with_token("http://fixture.invalid:1", 3, true, Some("first"), None)
            .await
            .unwrap();
    assert!(client.is_auto_reconnect());
    assert_eq!(client.update_interval, 3);
    assert_eq!(client.access_token.as_deref(), Some("first"));
    client.set_access_token(Some("second".into()));
    assert_eq!(
        client
            .create_request(())
            .metadata()
            .get("x-access-token")
            .unwrap(),
        "second"
    );
    client.set_access_token(None);
    assert!(!client
        .create_request(())
        .metadata()
        .contains_key("x-access-token"));
    assert_eq!(state.lock().unwrap().connection_kind, Some("plain"));
    let mut tls = shared::types::ClientTlsConfig {
        ca_cert_path: "/nonexistent/unit-ca".into(),
        client_cert_path: None,
        client_key_path: None,
        danger_skip_verify: false,
    };
    sdk::connect_next(state.clone());
    MonitorClient::connect_with_token("http://fixture.invalid:1", 1, false, None, Some(&tls))
        .await
        .unwrap();
    tls.danger_skip_verify = true;
    sdk::connect_next(state.clone());
    MonitorClient::connect_with_token("https://fixture.invalid:1", 1, false, None, Some(&tls))
        .await
        .unwrap();
    assert_eq!(state.lock().unwrap().connection_kind, Some("custom"));
    sdk::reject_next();
    assert!(MonitorClient::connect_with_token(
        "https://fixture.invalid:1",
        1,
        false,
        None,
        Some(&tls)
    )
    .await
    .is_err());
    tls.danger_skip_verify = false;
    let temp = tempfile::tempdir().unwrap();
    let ca = temp.path().join("ca.pem");
    std::fs::write(&ca, "invalid certificate").unwrap();
    tls.ca_cert_path = ca.display().to_string();
    assert!(MonitorClient::connect_with_token(
        "https://fixture.invalid:1",
        1,
        false,
        None,
        Some(&tls)
    )
    .await
    .is_err());
}
#[tokio::test]
async fn existing_ca_with_missing_client_identity_propagates_configuration_error() {
    let temp = tempfile::tempdir().unwrap();
    let ca = temp.path().join("ca.pem");
    std::fs::write(&ca, "unit CA fixture").unwrap();
    let tls = shared::types::ClientTlsConfig {
        ca_cert_path: ca.display().to_string(),
        client_cert_path: Some(temp.path().join("absent-cert").display().to_string()),
        client_key_path: Some(temp.path().join("absent-key").display().to_string()),
        danger_skip_verify: false,
    };
    let error =
        MonitorClient::connect_with_token("https://fixture.invalid:1", 1, false, None, Some(&tls))
            .await
            .err()
            .unwrap();
    assert!(error
        .to_string()
        .contains("Failed to read client certificate"));
}
// Keep errors inspectable without requiring the client to expose Debug.
trait ErrorDisplay {
    fn unwrap_err_display(self) -> String;
}
impl ErrorDisplay for Result<MonitorClient> {
    fn unwrap_err_display(self) -> String {
        self.err().expect("expected error").to_string()
    }
}

#[tokio::test]
async fn rpc_errors_preserve_context_and_request_contracts() {
    let (mut client, state) = fixture();
    macro_rules! failure { ($method:ident, $rpc:literal, $text:literal $(,$arg:expr)*) => {
        state.lock().unwrap().reject($rpc);
        assert!(client.$method($($arg),*).await.err().unwrap().to_string().contains($text));
    }; }
    failure!(
        get_system_info,
        "get_system_info",
        "Failed to get system info"
    );
    failure!(
        get_processes,
        "get_processes",
        "Failed to get processes",
        9,
        Some("needle".into())
    );
    failure!(get_services, "get_services", "Failed to get services");
    failure!(
        get_network_info,
        "get_network_info",
        "Failed to get network info"
    );
    failure!(
        get_containers,
        "get_containers",
        "Failed to get containers",
        Some("image".into())
    );
    failure!(
        get_postgres_info,
        "get_postgres_info",
        "Failed to get postgres info"
    );
    failure!(
        get_mariadb_info,
        "get_maria_db_info",
        "Failed to get MariaDB info"
    );
    failure!(
        get_systemd_info,
        "get_systemd_info",
        "Failed to get systemd info"
    );
    failure!(get_tls_info, "get_tls_info", "Failed to get TLS info");
    failure!(
        get_listening_ports,
        "get_listening_ports",
        "Failed to get listening ports"
    );
    failure!(
        stream_system_updates,
        "stream_system_updates",
        "Failed to start system updates stream"
    );
    let state = state.lock().unwrap();
    assert_eq!(state.calls.len(), 11);
    assert!(state.responses.is_empty());
    assert!(state
        .calls
        .iter()
        .all(|call| call.token.as_deref() == Some("unit-token")));
    let call = state
        .calls
        .iter()
        .find(|c| c.method == "get_processes")
        .unwrap();
    let request = call.payload.downcast_ref::<ProcessesRequest>().unwrap();
    assert_eq!(request.limit, 9);
    assert_eq!(request.filter, "needle");
    let call = state
        .calls
        .iter()
        .find(|c| c.method == "get_containers")
        .unwrap();
    assert_eq!(
        call.payload
            .downcast_ref::<wire::ContainersRequest>()
            .unwrap()
            .filter,
        "image"
    );
    let call = state
        .calls
        .iter()
        .find(|c| c.method == "stream_system_updates")
        .unwrap();
    assert_eq!(
        call.payload
            .downcast_ref::<UpdatesRequest>()
            .unwrap()
            .update_interval_seconds,
        7
    );
}

#[tokio::test]
async fn system_process_service_and_network_payloads_are_converted() {
    let (mut client, state) = fixture();
    let source = wire::SystemInfoResponse {
        hostname: "unit-host".into(),
        os: "unit-os".into(),
        kernel_version: "1.2".into(),
        uptime_seconds: 17,
        cpu_count: 4,
        cpu_usage_percent: 25.,
        memory_total_bytes: 100,
        memory_used_bytes: 60,
        memory_available_bytes: 40,
        disk_info: vec![wire::DiskInfo {
            device: "unit-disk".into(),
            mount_point: "/fixture".into(),
            filesystem_type: "unitfs".into(),
            total_bytes: 200,
            used_bytes: 50,
            available_bytes: 150,
            usage_percent: 25.,
        }],
        timestamp: timestamp(),
        swap_total_bytes: 500,
        swap_used_bytes: 10,
        swap_in_pages_per_sec: 1.5,
        swap_out_pages_per_sec: 2.5,
        psi_memory_some_avg60: Some(1.),
        psi_memory_full_avg60: Some(2.),
        psi_memory_full_total_seconds: Some(3.),
    };
    state.lock().unwrap().respond("get_system_info", source);
    let info = client.get_system_info().await.unwrap();
    assert_eq!(info.hostname, "unit-host");
    assert_eq!(info.disk_info[0].mount_point, "/fixture");
    assert_eq!(info.swap.activity_pages_per_sec(), 4.);
    assert_eq!(info.memory_pressure.unwrap().full_avg60, 2.);
    assert_eq!(info.timestamp.timestamp(), 1_700_000_000);
    state
        .lock()
        .unwrap()
        .respond("get_system_info", wire::SystemInfoResponse::default());
    assert!(client
        .get_system_info()
        .await
        .unwrap()
        .memory_pressure
        .is_none());
    state.lock().unwrap().respond(
        "get_processes",
        wire::ProcessesResponse {
            processes: vec![
                wire::ProcessInfo {
                    pid: 42,
                    name: "worker".into(),
                    user: "unit".into(),
                    cpu_usage_percent: 1.,
                    memory_usage_bytes: 123,
                    command_line: "unit-worker".into(),
                    start_time: timestamp(),
                    status: "running".into(),
                    swap_bytes: Some(9),
                },
                wire::ProcessInfo::default(),
            ],
            ..Default::default()
        },
    );
    let processes = client.get_processes(10, None).await.unwrap();
    assert_eq!(processes.len(), 2);
    assert_eq!(processes[0].pid, 42);
    assert_eq!(processes[0].swap_bytes, Some(9));
    assert_eq!(processes[0].start_time.timestamp(), 1_700_000_000);
    let services = ["Running", "Stopped", "Failed", "new-state"]
        .iter()
        .enumerate()
        .map(|(i, status)| wire::ServiceInfo {
            name: format!("service-{i}"),
            status: (*status).into(),
            pid: i as u32,
            uptime_seconds: i as u64,
            ..Default::default()
        })
        .collect();
    state.lock().unwrap().respond(
        "get_services",
        wire::ServicesResponse {
            services,
            ..Default::default()
        },
    );
    let services = client.get_services().await.unwrap();
    assert!(matches!(services[0].status, ServiceStatus::Running));
    assert!(matches!(services[1].status, ServiceStatus::Stopped));
    assert!(matches!(services[2].status, ServiceStatus::Failed));
    assert!(matches!(services[3].status, ServiceStatus::Unknown));
    assert_eq!(services[0].pid, None);
    assert_eq!(services[1].pid, Some(1));
    assert_eq!(services[0].uptime_seconds, None);
    assert_eq!(services[1].uptime_seconds, Some(1));
    state.lock().unwrap().respond(
        "get_network_info",
        wire::NetworkInfoResponse {
            interfaces: vec![wire::NetworkInfo {
                interface: "eth-unit".into(),
                ip_address: "192.0.2.1".into(),
                mac_address: "00:11:22:33:44:55".into(),
                is_up: true,
                bytes_sent: 1,
                bytes_received: 2,
                packets_sent: 3,
                packets_received: 4,
            }],
            ..Default::default()
        },
    );
    let networks = client.get_network_info().await.unwrap();
    assert_eq!(networks[0].interface, "eth-unit");
    assert_eq!(networks[0].packets_received, 4);
    assert!(state.lock().unwrap().responses.is_empty());
}

#[tokio::test]
async fn container_optional_health_and_image_status_are_preserved() {
    let (mut client, state) = fixture();
    let containers = (0..5)
        .map(|status| wire::ContainerInfo {
            id: format!("id-{status}"),
            name: "unit-container".into(),
            image: "unit/image:tag".into(),
            failing_streak: if status == 1 { 1 } else { 0 },
            last_health_output: if status == 2 {
                "probe".into()
            } else {
                String::new()
            },
            last_health_checked_at: timestamp(),
            last_health_exit_code: 2,
            image_version: Some(wire::ImageVersionInfo {
                status,
                checked_at: timestamp(),
                ..Default::default()
            }),
            ..Default::default()
        })
        .chain(std::iter::once(wire::ContainerInfo::default()))
        .collect();
    state.lock().unwrap().respond(
        "get_containers",
        wire::ContainersResponse {
            containers,
            ..Default::default()
        },
    );
    let result = client.get_containers(None).await.unwrap();
    assert_eq!(result.len(), 6);
    assert_eq!(result[0].id, "id-0");
    assert!(result[0].health_detail.is_none());
    assert_eq!(result[1].health_detail.as_ref().unwrap().failing_streak, 1);
    assert_eq!(
        result[2].health_detail.as_ref().unwrap().last_output,
        "probe"
    );
    assert!(result[5].image_version.is_none());
    for (index, expected) in [
        shared::types::VersionStatus::Unknown,
        shared::types::VersionStatus::UpToDate,
        shared::types::VersionStatus::Drifted,
        shared::types::VersionStatus::LocalBuild,
        shared::types::VersionStatus::Unknown,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            result[index].image_version.as_ref().unwrap().status,
            expected
        );
    }
}

#[tokio::test]
async fn database_nested_rows_and_missing_values_are_preserved() {
    let (mut client, state) = fixture();
    state.lock().unwrap().respond(
        "get_postgres_info",
        wire::PostgresInfoResponse {
            clusters: vec![
                wire::PostgresClusterInfo {
                    name: "pg-unit".into(),
                    host: "db.invalid".into(),
                    port: 5432,
                    databases: vec![wire::PostgresDatabaseInfo {
                        name: "app".into(),
                        transactions: 7,
                        stats_reset_at: timestamp(),
                        ..Default::default()
                    }],
                    connections_by_state: vec![wire::ConnectionStateCount {
                        state: "active".into(),
                        count: 3,
                    }],
                    top_queries: vec![wire::TopQuery {
                        query: "select unit".into(),
                        calls: 4,
                        total_exec_time_ms: 10.,
                        mean_exec_time_ms: 2.5,
                    }],
                    timestamp: timestamp(),
                    settings: vec![
                        wire::PostgresSetting {
                            name: "work_mem".into(),
                            value: "8".into(),
                            unit: "MB".into(),
                            source: "unit".into(),
                            source_file: "fixture.conf".into(),
                            source_line: 9,
                        },
                        wire::PostgresSetting::default(),
                    ],
                    ..Default::default()
                },
                wire::PostgresClusterInfo::default(),
            ],
            ..Default::default()
        },
    );
    let result = client.get_postgres_info().await.unwrap();
    assert_eq!(result[0].port, 5432);
    assert_eq!(result[0].databases[0].transactions, 7);
    assert_eq!(
        result[0].databases[0].stats_reset_at.unwrap().timestamp(),
        1_700_000_000
    );
    assert_eq!(result[0].connections_by_state[0].count, 3);
    assert_eq!(result[0].top_queries[0].query, "select unit");
    assert_eq!(result[0].settings[0].source_line, Some(9));
    assert_eq!(result[0].settings[1].source_line, None);
    assert_eq!(result[0].settings[1].unit, None);
    assert_eq!(result[0].settings[1].source_file, None);
    state.lock().unwrap().respond(
        "get_maria_db_info",
        wire::MariaDbInfoResponse {
            clusters: vec![
                wire::MariaDbClusterInfo {
                    name: "maria-unit".into(),
                    host: "db.invalid".into(),
                    port: 3306,
                    schemas: vec![wire::MariaDbSchemaInfo {
                        name: "app".into(),
                        size_bytes: 99,
                        table_count: 2,
                        last_write_at: timestamp(),
                        write_time_available: true,
                    }],
                    innodb_status: "healthy".into(),
                    processes: vec![
                        wire::MariaDbProcessInfo {
                            id: 5,
                            db: "app".into(),
                            info: "select unit".into(),
                            ..Default::default()
                        },
                        wire::MariaDbProcessInfo::default(),
                    ],
                    timestamp: timestamp(),
                    ..Default::default()
                },
                wire::MariaDbClusterInfo::default(),
            ],
            ..Default::default()
        },
    );
    let result = client.get_mariadb_info().await.unwrap();
    assert_eq!(result[0].schemas[0].name, "app");
    assert_eq!(
        result[0].schemas[0].last_write_at.unwrap().timestamp(),
        1_700_000_000
    );
    assert_eq!(result[0].innodb_status.as_deref(), Some("healthy"));
    assert_eq!(result[1].innodb_status, None);
    assert_eq!(result[0].processes[0].db.as_deref(), Some("app"));
    assert_eq!(result[0].processes[0].info.as_deref(), Some("select unit"));
    assert_eq!(result[0].processes[1].db, None);
    assert_eq!(result[0].processes[1].info, None);
}

#[tokio::test]
async fn systemd_certificate_ports_and_stream_responses_are_preserved() {
    let (mut client, state) = fixture();
    state.lock().unwrap().respond(
        "get_systemd_info",
        wire::SystemdInfoResponse {
            units: vec![
                wire::SystemdUnitInfo {
                    name: "unit.service".into(),
                    pid: 42,
                    started_at: timestamp(),
                    ..Default::default()
                },
                wire::SystemdUnitInfo::default(),
            ],
            failed_units: vec![wire::SystemdFailedUnit {
                name: "failed.service".into(),
                description: "unit failure".into(),
                since: timestamp(),
            }],
            ..Default::default()
        },
    );
    let result = client.get_systemd_info().await.unwrap();
    assert_eq!(result.units[0].pid, Some(42));
    assert_eq!(result.units[1].pid, None);
    assert_eq!(
        result.units[0].started_at.unwrap().timestamp(),
        1_700_000_000
    );
    assert_eq!(result.failed_units[0].name, "failed.service");
    assert!(result.failed_units[0].since.is_some());
    state.lock().unwrap().respond(
        "get_tls_info",
        wire::TlsInfoResponse {
            certificates: vec![wire::TlsCertificateInfo {
                name: "unit-cert".into(),
                domains: vec!["unit.invalid".into()],
                not_after: timestamp(),
                days_until_expiry: -2,
                orphaned: true,
                ..Default::default()
            }],
            renewal_unit_name: Some("certbot.timer".into()),
            renewal_unit_status: Some("active".into()),
            ..Default::default()
        },
    );
    let result = client.get_tls_info().await.unwrap();
    assert_eq!(result.certificates[0].days_until_expiry, -2);
    assert!(result.certificates[0].orphaned);
    assert!(result.certificates[0].not_after.is_some());
    assert_eq!(result.renewal_unit_name.as_deref(), Some("certbot.timer"));
    state.lock().unwrap().respond(
        "get_listening_ports",
        wire::ListeningPortsResponse {
            ports: vec![
                wire::ListeningPortInfo {
                    address: "0.0.0.0".into(),
                    port: 5432,
                    pid: 42,
                    bind_scope: 3,
                    protocol: "tcp".into(),
                    sensitive: true,
                    ..Default::default()
                },
                wire::ListeningPortInfo::default(),
            ],
            ..Default::default()
        },
    );
    let ports = client.get_listening_ports().await.unwrap();
    assert_eq!(ports[0].pid, Some(42));
    assert_eq!(ports[1].pid, None);
    assert_eq!(ports[0].bind_scope, BindScope::AllInterfaces);
    assert!(ports[0].sensitive);
    state.lock().unwrap().respond(
        "stream_system_updates",
        sdk::Streaming(std::collections::VecDeque::from([Ok(
            SystemUpdate::default(),
        )])),
    );
    let mut stream = client.stream_system_updates().await.unwrap();
    assert!(stream.message().await.unwrap().is_some());
    assert!(stream.message().await.unwrap().is_none());
    assert!(state.lock().unwrap().responses.is_empty());
}
