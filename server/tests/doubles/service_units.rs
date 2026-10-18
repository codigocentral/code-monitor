//! Direct gRPC methods with scripted monitor data; no server or socket.
use super::*;
use crate::unit_service_monitor as sdk;
use serde::de::DeserializeOwned;
use serde_json::json;
use shared::types::*;
use std::sync::Mutex;
use tokio_stream::StreamExt;
fn fixture() -> (MonitorServiceImpl, sdk::SharedState) {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    let service = MonitorServiceImpl::from_arc(
        Arc::new(sdk::SystemMonitor(state.clone())),
        Config {
            enable_authentication: false,
            ..Default::default()
        },
    )
    .unwrap();
    (service, state)
}
fn dto<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn system() -> SystemInfo {
    dto(
        json!({"hostname":"unit-host","os":"FixtureOS","kernel_version":"1","uptime_seconds":42,"cpu_count":2,"cpu_usage_percent":30.5,"memory_total_bytes":1000,"memory_used_bytes":400,"memory_available_bytes":600,"disk_info":[{"device":"unit-disk","mount_point":"/unit","filesystem_type":"fixture","total_bytes":100,"used_bytes":40,"available_bytes":60,"usage_percent":40.0}],"timestamp":"2026-01-01T00:00:00.123Z","swap":{"total_bytes":10,"used_bytes":5,"in_pages_per_sec":1.5,"out_pages_per_sec":2.5},"memory_pressure":{"some_avg60":3.0,"full_avg60":4.0,"full_total_seconds":5.0}}),
    )
}
#[tokio::test]
async fn constructors_start_only_owned_monitor_and_shutdown_stops_it() {
    for with_config in [false, true] {
        let state = Arc::new(Mutex::new(sdk::State::default()));
        let monitor = sdk::SystemMonitor(state.clone());
        let service = if with_config {
            MonitorServiceImpl::new_with_config(monitor, Config::default())
                .await
                .unwrap()
        } else {
            MonitorServiceImpl::new(monitor).await.unwrap()
        };
        assert_eq!(state.lock().unwrap().starts, 1);
        service.shutdown();
        assert_eq!(state.lock().unwrap().stops, 1);
    }
}
#[tokio::test]
async fn system_process_service_network_mapping_and_filter_validation() {
    let (service, state) = fixture();
    state.lock().unwrap().respond("system", system());
    let value = service
        .get_system_info(Request::new(()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(value.hostname, "unit-host");
    assert_eq!(value.disk_info[0].device, "unit-disk");
    assert_eq!(value.timestamp.unwrap().nanos, 123_000_000);
    assert_eq!(value.psi_memory_full_avg60, Some(4.));
    assert_eq!(value.swap_in_pages_per_sec, 1.5);
    let mut no_pressure = system();
    no_pressure.memory_pressure = None;
    state.lock().unwrap().respond("system", no_pressure);
    assert_eq!(
        service
            .get_system_info(Request::new(()))
            .await
            .unwrap()
            .into_inner()
            .psi_memory_full_avg60,
        None
    );
    for filter in ["", "unit"] {
        let processes: Vec<ProcessInfo> = dto(
            json!([{"pid":7,"name":"unit","user":"u","cpu_usage_percent":4.,"memory_usage_bytes":90,"command_line":"unit --x","start_time":"2026-01-01T00:00:00Z","status":"Running","swap_bytes":11}]),
        );
        state.lock().unwrap().respond("processes", processes);
        let result = service
            .get_processes(Request::new(ProcessesRequest {
                limit: 3,
                filter: filter.into(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(result.processes[0].pid, 7);
        assert_eq!(result.processes[0].swap_bytes, Some(11));
    }
    assert_eq!(
        state.lock().unwrap().process_requests,
        [(3, None), (3, Some("unit".into()))]
    );
    assert_eq!(
        service
            .get_processes(Request::new(ProcessesRequest {
                limit: 1,
                filter: "x".repeat(257)
            }))
            .await
            .unwrap_err()
            .code(),
        tonic::Code::InvalidArgument
    );
    let services: Vec<ServiceInfo> = dto(
        json!([{"name":"unit","status":"Running","pid":7,"cpu_usage_percent":4.,"memory_usage_bytes":8,"user":"u","uptime_seconds":9},{"name":"empty","status":"Stopped","pid":null,"cpu_usage_percent":0.,"memory_usage_bytes":0,"user":"","uptime_seconds":null}]),
    );
    state.lock().unwrap().respond("services", services);
    let values = service
        .get_services(Request::new(()))
        .await
        .unwrap()
        .into_inner()
        .services;
    assert_eq!(values[0].status, "Running");
    assert_eq!(values[0].uptime_seconds, 9);
    assert_eq!(values[1].pid, 0);
    let networks: Vec<NetworkInfo> = dto(
        json!([{"interface":"fixture0","ip_address":"192.0.2.1","mac_address":"00:00:00:00:00:01","is_up":true,"bytes_sent":1,"bytes_received":2,"packets_sent":3,"packets_received":4}]),
    );
    state.lock().unwrap().respond("networks", networks);
    let value = service
        .get_network_info(Request::new(()))
        .await
        .unwrap()
        .into_inner()
        .interfaces
        .remove(0);
    assert_eq!(value.interface, "fixture0");
    assert_eq!(
        (
            value.bytes_sent,
            value.bytes_received,
            value.packets_sent,
            value.packets_received
        ),
        (1, 2, 3, 4)
    );
}
#[tokio::test]
async fn containers_preserve_health_and_every_image_version_status() {
    let (service, state) = fixture();
    let mut containers = vec![];
    for status in ["UpToDate", "Drifted", "LocalBuild", "Unknown"] {
        let value: ContainerInfo = dto(
            json!({"id":status,"name":"unit","image":"unit:tag","status":"Up","state":"running","health":"healthy","cpu_percent":5.,"memory_usage_bytes":50,"memory_limit_bytes":100,"memory_percent":50.,"restart_count":3,"network_rx_bytes":4,"network_tx_bytes":5,"networks":["unit-net"],"memory_limit_set":true,"health_detail":{"failing_streak":6,"last_output":"check","last_exit_code":7,"last_checked_at":"2026-01-01T00:00:00Z"},"swap_bytes":8,"image_version":{"image_ref":"unit:tag","registry":"fixture.invalid","repository":"unit","tag":"tag","local_digest":"sha256:1","remote_digest":"sha256:2","status":status,"checked_at":"2026-01-01T00:00:00Z","error":"unit detail"}}),
        );
        containers.push(value);
    }
    let mut absent = containers[0].clone();
    absent.health_detail = None;
    absent.image_version = None;
    containers.push(absent);
    state.lock().unwrap().respond("containers", containers);
    let values = service
        .get_containers(Request::new(ContainersRequest::default()))
        .await
        .unwrap()
        .into_inner()
        .containers;
    for (item, status) in values.iter().zip([
        ProtoImageVersionStatus::UpToDate,
        ProtoImageVersionStatus::Drifted,
        ProtoImageVersionStatus::LocalBuild,
        ProtoImageVersionStatus::Unknown,
    ]) {
        assert_eq!(item.image_version.as_ref().unwrap().status, status as i32);
        assert_eq!(item.failing_streak, 6);
        assert_eq!(item.last_health_exit_code, 7);
        assert_eq!(item.last_health_output, "check");
        assert!(item.last_health_checked_at.is_some());
    }
    assert!(values[4].image_version.is_none());
    assert_eq!(values[4].failing_streak, 0);
    assert!(values[4].last_health_output.is_empty());
}
#[tokio::test]
async fn database_mapping_keeps_optional_metadata_and_nested_rows() {
    let (service, state) = fixture();
    let postgres: Vec<PostgresClusterInfo> = dto(
        json!([{"name":"pg","host":"fixture.invalid","port":5432,"databases":[{"name":"app","size_bytes":10,"num_backends":2,"cache_hit_ratio":99.,"transactions":33,"stats_reset_at":"2026-01-01T00:00:00Z"}],"connections_total":5,"connections_by_state":[{"state":"active","count":2}],"cache_hit_ratio":98.,"top_queries":[{"query":"SELECT 1","calls":2,"total_exec_time_ms":3.,"mean_exec_time_ms":1.5}],"timestamp":"2026-01-01T00:00:00Z","settings":[{"name":"work_mem","value":"8","unit":"MB","source":"configuration file","source_file":"unit.conf","source_line":9},{"name":"max_connections","value":"100","unit":null,"source":"default","source_file":null,"source_line":null}]}]),
    );
    state.lock().unwrap().respond("postgres", postgres);
    let pg = service
        .get_postgres_info(Request::new(()))
        .await
        .unwrap()
        .into_inner()
        .clusters
        .remove(0);
    assert_eq!(pg.databases[0].transactions, 33);
    assert!(pg.databases[0].stats_reset_at.is_some());
    assert_eq!(pg.settings[0].source_line, 9);
    assert_eq!(pg.settings[1].source_line, 0);
    assert!(pg.settings[1].unit.is_empty());
    assert_eq!(pg.top_queries[0].calls, 2);
    assert_eq!(pg.connections_by_state[0].count, 2);
    let maria: Vec<MariaDBClusterInfo> = dto(
        json!([{"name":"maria","host":"fixture.invalid","port":3306,"schemas":[{"name":"app","size_bytes":10,"table_count":2,"last_write_at":"2026-01-01T00:00:00Z","write_time_available":true}],"connections_active":2,"connections_total":5,"innodb_status":"unit","processes":[{"id":7,"user":"u","host":"fixture.invalid","db":"app","command":"Query","time_seconds":8,"state":"executing","info":"SELECT 1"},{"id":8,"user":"u","host":"fixture.invalid","db":null,"command":"Sleep","time_seconds":9,"state":"idle","info":null}],"timestamp":"2026-01-01T00:00:00Z"}]),
    );
    state.lock().unwrap().respond("mariadb", maria);
    let db = service
        .get_maria_db_info(Request::new(()))
        .await
        .unwrap()
        .into_inner()
        .clusters
        .remove(0);
    assert!(db.schemas[0].write_time_available);
    assert!(db.schemas[0].last_write_at.is_some());
    assert_eq!(db.processes[0].db, "app");
    assert!(db.processes[1].info.is_empty());
    assert_eq!(db.innodb_status, "unit");
}
#[tokio::test]
async fn systemd_certificates_and_port_mapping_preserve_timestamps_and_defaults() {
    let (service, state) = fixture();
    let units: Vec<SystemdUnitInfo> = dto(
        json!([{"name":"unit.service","status":"active","is_active":true,"pid":3,"memory_current_bytes":10,"started_at":"2026-01-01T00:00:00Z"}]),
    );
    let failed: Vec<SystemdFailedUnit> = dto(
        json!([{"name":"failed.service","description":"unit failure","since":"2026-01-01T00:00:00Z"}]),
    );
    state.lock().unwrap().respond("systemd", units);
    state.lock().unwrap().respond("failed", failed);
    let value = service
        .get_systemd_info(Request::new(()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(value.units[0].pid, 3);
    assert!(value.units[0].started_at.is_some());
    assert_eq!(value.failed_units[0].description, "unit failure");
    assert!(value.failed_units[0].since.is_some());
    let snapshot: TlsSnapshot = dto(
        json!({"certificates":[{"name":"unit","domains":["fixture.invalid"],"issuer":"Unit","not_after":"2026-01-01T00:00:00Z","days_until_expiry":-1,"source":"unit.pem","orphaned":true}],"renewal_unit_name":"unit.timer","renewal_unit_status":"active"}),
    );
    state.lock().unwrap().respond("tls", snapshot);
    let value = service
        .get_tls_info(Request::new(()))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(value.certificates[0].days_until_expiry, -1);
    assert!(value.certificates[0].orphaned);
    assert_eq!(value.renewal_unit_status.as_deref(), Some("active"));
    let ports: Vec<ListeningPortInfo> = dto(
        json!([{"address":"0.0.0.0","port":5432,"protocol":"tcp","bind_scope":"AllInterfaces","pid":5,"process_name":"unit","sensitive":true},{"address":"127.0.0.1","port":1,"protocol":"tcp","bind_scope":"Loopback","pid":null,"process_name":"","sensitive":false}]),
    );
    state.lock().unwrap().respond("ports", ports);
    let value = service
        .get_listening_ports(Request::new(()))
        .await
        .unwrap()
        .into_inner()
        .ports;
    assert_eq!(value[0].pid, 5);
    assert_eq!(value[1].pid, 0);
    assert!(value[0].sensitive);
    assert_ne!(value[0].bind_scope, value[1].bind_scope);
}
#[tokio::test]
async fn monitor_errors_are_mapped_to_internal_status() {
    let (_guard, _logs) = crate::unit_logs::capture();
    let (service, state) = fixture();
    macro_rules! reject {
        ($name:literal,$method:ident,$request:expr) => {{
            state.lock().unwrap().reject($name);
            let error = service.$method(Request::new($request)).await.unwrap_err();
            assert_eq!(error.code(), tonic::Code::Internal);
            assert!(error.message().starts_with("Failed to"));
        }};
    }
    reject!("system", get_system_info, ());
    reject!("processes", get_processes, ProcessesRequest::default());
    reject!("services", get_services, ());
    reject!("networks", get_network_info, ());
    reject!("containers", get_containers, ContainersRequest::default());
    reject!("postgres", get_postgres_info, ());
    reject!("mariadb", get_maria_db_info, ());
    reject!("systemd", get_systemd_info, ());
    state
        .lock()
        .unwrap()
        .respond("systemd", Vec::<SystemdUnitInfo>::new());
    reject!("failed", get_systemd_info, ());
    reject!("tls", get_tls_info, ());
    reject!("ports", get_listening_ports, ());
}
#[tokio::test]
async fn authentication_format_precedence_and_poisoned_config() {
    let (service, _state) = fixture();
    {
        let mut config = service.config.write().unwrap();
        config.enable_authentication = true;
        config.access_token = "unit-secret".into();
    }
    for header in ["x-access-token", "authorization"] {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            header,
            tonic::metadata::MetadataValue::try_from(&b"\xff"[..]).unwrap(),
        );
        assert_eq!(
            service.validate_request(&request).unwrap_err().message(),
            if header == "x-access-token" {
                "Invalid token format"
            } else {
                "Invalid auth format"
            }
        );
    }
    let mut invalid = Request::new(());
    invalid
        .metadata_mut()
        .insert("authorization", "Bearer wrong".parse().unwrap());
    assert_eq!(
        service.validate_request(&invalid).unwrap_err().code(),
        tonic::Code::Unauthenticated
    );
    let mut request = Request::new(());
    request
        .metadata_mut()
        .insert("x-access-token", "wrong".parse().unwrap());
    request
        .metadata_mut()
        .insert("authorization", "Bearer unit-secret".parse().unwrap());
    assert!(service.validate_request(&request).is_ok());
    let lock = service.config.clone();
    assert!(std::thread::spawn(move || {
        let _guard = lock.write().unwrap();
        panic!("unit config poisoned");
    })
    .join()
    .is_err());
    assert_eq!(
        service.max_clients().unwrap_err().message(),
        "Config lock error"
    );
    assert_eq!(
        service
            .validate_request(&Request::new(()))
            .unwrap_err()
            .message(),
        "Config lock error"
    );
}
#[tokio::test]
async fn all_rpc_methods_reject_missing_auth_before_touching_monitor() {
    let (service, state) = fixture();
    service.config.write().unwrap().enable_authentication = true;
    macro_rules! reject {
        ($method:ident,$request:expr) => {
            assert_eq!(
                service
                    .$method(Request::new($request))
                    .await
                    .err()
                    .unwrap()
                    .code(),
                tonic::Code::Unauthenticated
            );
        };
    }
    reject!(get_system_info, ());
    reject!(get_processes, ProcessesRequest::default());
    reject!(get_services, ());
    reject!(get_network_info, ());
    reject!(get_containers, ContainersRequest::default());
    reject!(get_postgres_info, ());
    reject!(get_maria_db_info, ());
    reject!(get_systemd_info, ());
    reject!(get_tls_info, ());
    reject!(get_listening_ports, ());
    reject!(stream_system_updates, UpdatesRequest::default());
    assert!(state.lock().unwrap().calls.is_empty());
}
#[tokio::test]
async fn streaming_limit_counter_drop_and_scripted_success_or_error_ticks() {
    let (_guard, _logs) = crate::unit_logs::capture();
    for fail in [false, true] {
        let (service, state) = fixture();
        service.config.write().unwrap().max_clients = 1;
        let mut ticks = vec![];
        for _ in 0..3 {
            ticks.push(Box::new(|| {}) as crate::unit_runtime::Hook);
        }
        let _timer = crate::unit_runtime::plan(std::time::Duration::from_secs(1), ticks);
        {
            let mut state = state.lock().unwrap();
            for _ in 0..3 {
                if fail {
                    state.reject("system");
                } else {
                    state.respond("system", system());
                }
            }
        }
        let before = metrics::get(&metrics::GRPC_ACTIVE_STREAMS);
        let rejected_before = metrics::get(&metrics::GRPC_STREAMS_REJECTED_TOTAL);
        let mut response = service
            .stream_system_updates(Request::new(UpdatesRequest {
                update_interval_seconds: 0,
                ..Default::default()
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(service.active_streams.load(Ordering::SeqCst), 1);
        assert_eq!(metrics::get(&metrics::GRPC_ACTIVE_STREAMS), before + 1);
        assert_eq!(
            service
                .stream_system_updates(Request::new(UpdatesRequest::default()))
                .await
                .err()
                .unwrap()
                .code(),
            tonic::Code::ResourceExhausted
        );
        assert_eq!(
            metrics::get(&metrics::GRPC_STREAMS_REJECTED_TOTAL),
            rejected_before + 1
        );
        let item = response.next().await.unwrap().unwrap();
        assert_eq!(item.update_type.is_none(), fail);
        assert!(item.timestamp.is_some());
        if let Some(UpdateType::SystemInfo(info)) = item.update_type {
            assert_eq!(info.hostname, "unit-host");
            assert_eq!(info.disk_info[0].mount_point, "/unit");
            assert_eq!(info.psi_memory_full_total_seconds, Some(5.));
        }
        drop(response);
        assert_eq!(service.active_streams.load(Ordering::SeqCst), 0);
        assert_eq!(metrics::get(&metrics::GRPC_ACTIVE_STREAMS), before);
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert!(
            state.lock().unwrap().calls.len() >= 2,
            "send to dropped receiver must terminate worker"
        );
        assert!(state.lock().unwrap().calls.len() <= 3);
    }
}
