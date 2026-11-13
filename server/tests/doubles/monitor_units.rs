//! Getter behavior against owned sysinfo/proc fixtures. No host sampling.
use super::*;
use crate::unit_sysinfo as sdk;
use std::sync::atomic::Ordering as AtomicOrdering;
struct Fixture {
    state: crate::unit_collectors::SharedState,
    _scope: crate::unit_collectors::Guard,
}
fn fixture(system: sdk::System) -> (SystemMonitor, Fixture) {
    let state = Arc::new(std::sync::Mutex::new(
        crate::unit_collectors::State::default(),
    ));
    let scope = crate::unit_collectors::scope(state.clone());
    let monitor = SystemMonitor {
        system: Arc::new(RwLock::new(system)),
        update_interval: Duration::from_secs(2),
        last_update: Arc::new(RwLock::new(sdk::now())),
        is_running: Arc::new(AtomicBool::new(false)),
        docker_collector: DockerCollector::new_none(),
        postgres_collectors: vec![],
        mariadb_collectors: vec![],
        systemd_collector: SystemdCollector::new(vec![]),
        tls_collector: TlsCollector::new(vec!["unit-certs".into()]),
        net_ports_collector: NetPortsCollector::with_proc_root("unit-proc"),
        memory_collector: MemoryCollector::with_proc_root("unit-proc"),
        swap_activity: Arc::new(RwLock::new(SwapActivityTracker::new())),
        swap_rate: Arc::new(RwLock::new((2., 3.))),
    };
    (
        monitor,
        Fixture {
            state,
            _scope: scope,
        },
    )
}
fn poison<T: Send + Sync + 'static>(lock: Arc<RwLock<T>>) {
    assert!(std::thread::spawn(move || {
        let _guard = lock.write().unwrap();
        panic!("unit poisoned lock");
    })
    .join()
    .is_err());
}
fn process(name: &str, cpu: f32, memory: u64) -> sdk::Process {
    sdk::Process {
        name: name.into(),
        cpu,
        memory,
        command: vec!["unit-command".into(), "--worker".into()],
        runtime: 17,
        user: Some(1000),
        status: sdk::ProcessStatus::Run,
    }
}
#[test]
fn system_snapshot_maps_metrics_defaults_saturation_and_owned_pressure() {
    let system = sdk::System {
        total_memory: 100,
        used_memory: 60,
        total_swap: 50,
        used_swap: 10,
        host: Some("unit-host".into()),
        os: Some("UnitOS".into()),
        version: Some("42".into()),
        kernel: Some("unit-kernel".into()),
        uptime: 25,
        cores: Some(4),
        cpu: sdk::Cpu { usage: 37.5 },
        disks: vec![
            sdk::Disk {
                total: 100,
                available: 40,
                name: "disk-unit".into(),
                mount: "/fixture".into(),
                filesystem: b"unitfs".to_vec(),
            },
            sdk::Disk::default(),
        ],
        ..Default::default()
    };
    let (monitor, root) = fixture(system);
    let info = monitor.get_system_info().unwrap();
    assert_eq!(info.hostname, "unit-host");
    assert_eq!(info.os, "UnitOS 42");
    assert_eq!(info.kernel_version, "unit-kernel");
    assert_eq!(info.uptime_seconds, 25);
    assert_eq!(info.cpu_count, 4);
    assert_eq!(info.cpu_usage_percent, 37.5);
    assert_eq!(info.memory_available_bytes, 40);
    assert_eq!(info.disk_info[0].usage_percent, 60.);
    assert_eq!(info.disk_info[1].usage_percent, 0.);
    assert_eq!(info.disk_info[0].device, "disk-unit");
    assert_eq!(info.disk_info[0].filesystem_type, "unitfs");
    assert_eq!(info.swap.activity_pages_per_sec(), 5.);
    assert_eq!(info.swap.used_bytes, 10);
    assert!(info.memory_pressure.is_none());
    assert_eq!(info.timestamp, sdk::now());
    root.state.lock().unwrap().pressure = Some(MemoryPressure {
        some_avg60: 2.,
        full_avg60: 1.,
        full_total_seconds: 2.,
    });
    assert_eq!(
        monitor
            .get_system_info()
            .unwrap()
            .memory_pressure
            .unwrap()
            .full_avg60,
        1.
    );
    let (monitor, _root) = fixture(sdk::System {
        total_memory: 1,
        used_memory: 2,
        ..Default::default()
    });
    let info = monitor.get_system_info().unwrap();
    assert_eq!(info.hostname, "unknown");
    assert_eq!(info.os, "Unknown Unknown");
    assert_eq!(info.kernel_version, "Unknown");
    assert_eq!(info.cpu_count, 1);
    assert_eq!(info.memory_available_bytes, 0);
    poison(monitor.swap_rate.clone());
    assert_eq!(
        monitor
            .get_system_info()
            .unwrap()
            .swap
            .activity_pages_per_sec(),
        0.
    );
}
#[test]
fn process_limits_filters_users_swap_and_nan_ordering_are_preserved() {
    let mut system = sdk::System::default();
    system
        .processes
        .insert(sdk::Pid::from_u32(11), process("unit-high", 90., 100));
    system.processes.insert(
        sdk::Pid::from_u32(12),
        sdk::Process {
            user: None,
            ..process("unit-low", 10., 10)
        },
    );
    let (monitor, root) = fixture(system);
    root.state
        .lock()
        .unwrap()
        .process_swap
        .insert(11, 10 * 1024);
    let all = monitor.get_processes(0, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].pid, 11);
    assert_eq!(all[0].swap_bytes, Some(10 * 1024));
    assert_eq!(all[0].command_line, "unit-command --worker");
    assert_eq!(
        all[0].start_time,
        sdk::now() - chrono::Duration::seconds(17)
    );
    assert_eq!(all[0].user, "1000");
    assert_eq!(all[1].user, "unknown");
    assert_eq!(all[1].swap_bytes, None);
    assert_eq!(monitor.get_processes(1, None).unwrap().len(), 1);
    assert_eq!(
        monitor.get_processes(0, Some("HIGH".into())).unwrap()[0].pid,
        11
    );
    assert_eq!(
        monitor
            .get_processes(0, Some("--worker".into()))
            .unwrap()
            .len(),
        2
    );
    assert!(monitor
        .get_processes(0, Some("not-present".into()))
        .unwrap()
        .is_empty());
    monitor
        .system
        .write()
        .unwrap()
        .processes
        .insert(sdk::Pid::from_u32(13), process("unit-nan", f32::NAN, 0));
    assert_eq!(monitor.get_processes(0, None).unwrap().len(), 3);
}
#[test]
fn service_heuristics_sorting_and_fifty_item_limit_are_preserved() {
    let mut system = sdk::System::default();
    for (i, name) in [
        "unitservice",
        "unitsvc",
        "unitd",
        "unitserver",
        "unitdaemon",
        "unitagent",
        "unitmonitor",
        "unitmanager",
    ]
    .into_iter()
    .enumerate()
    {
        let status = [
            sdk::ProcessStatus::Run,
            sdk::ProcessStatus::Sleep,
            sdk::ProcessStatus::Idle,
            sdk::ProcessStatus::Stop,
            sdk::ProcessStatus::Zombie,
            sdk::ProcessStatus::Dead,
            sdk::ProcessStatus::Unknown(1),
            sdk::ProcessStatus::Run,
        ][i];
        system.processes.insert(
            sdk::Pid::from_u32(i as u32 + 1),
            sdk::Process {
                runtime: 0,
                status,
                user: if i == 0 { None } else { Some(1000) },
                ..process(name, 1., i as u64)
            },
        );
    }
    system.processes.insert(
        sdk::Pid::from_u32(20),
        sdk::Process {
            runtime: 61,
            ..process("long-lived", 1., 99)
        },
    );
    system.processes.insert(
        sdk::Pid::from_u32(21),
        sdk::Process {
            runtime: 0,
            ..process("ordinary", 1., 0)
        },
    );
    let (monitor, _root) = fixture(system);
    let services = monitor.get_services().unwrap();
    assert_eq!(services.len(), 9);
    assert_eq!(services[0].name, "long-lived");
    assert!(!services.iter().any(|s| s.name == "ordinary"));
    assert_eq!(
        services
            .iter()
            .find(|s| s.name == "unitservice")
            .unwrap()
            .user,
        "unknown"
    );
    assert!(services
        .iter()
        .any(|s| matches!(s.status, ServiceStatus::Failed)));
    for i in 100..170 {
        monitor.system.write().unwrap().processes.insert(
            sdk::Pid::from_u32(i),
            sdk::Process {
                runtime: 61,
                ..process(&format!("worker-{i}"), 1., i as u64)
            },
        );
    }
    assert_eq!(monitor.get_services().unwrap().len(), 50);
}
#[test]
fn system_updates_and_each_poisoned_lock_report_context() {
    let system = sdk::System::default();
    let refreshes = system.refreshes.clone();
    let (monitor, _root) = fixture(system);
    SystemMonitor::update_system_info(&monitor.system, &monitor.last_update).unwrap();
    assert_eq!(refreshes.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(*monitor.last_update.read().unwrap(), sdk::now());
    poison(monitor.last_update.clone());
    assert!(monitor
        .get_system_info()
        .unwrap_err()
        .to_string()
        .contains("Failed to lock last_update"));
    assert!(
        SystemMonitor::update_system_info(&monitor.system, &monitor.last_update)
            .unwrap_err()
            .to_string()
            .contains("Failed to lock last_update")
    );
    let (monitor, _root) = fixture(sdk::System::default());
    poison(monitor.system.clone());
    assert!(monitor
        .get_system_info()
        .unwrap_err()
        .to_string()
        .contains("Failed to lock system"));
    assert!(monitor
        .get_processes(0, None)
        .unwrap_err()
        .to_string()
        .contains("Failed to lock system"));
    assert!(monitor
        .get_services()
        .unwrap_err()
        .to_string()
        .contains("Failed to lock system"));
    assert!(monitor
        .get_network_info()
        .unwrap_err()
        .to_string()
        .contains("Failed to lock system"));
    assert!(
        SystemMonitor::update_system_info(&monitor.system, &monitor.last_update)
            .unwrap_err()
            .to_string()
            .contains("Failed to lock system")
    );
}

pub(crate) async fn exercise_log_fallback() {
    let state = Arc::new(std::sync::Mutex::new(
        crate::unit_collectors::State::default(),
    ));
    let _scope = crate::unit_collectors::scope(state);
    sdk::next(sdk::System::default());
    let monitor = SystemMonitor::new(2, vec![], vec![], vec!["unit.service".into()])
        .await
        .unwrap();
    assert_eq!(monitor.update_interval, Duration::from_secs(2));
}

#[tokio::test]
async fn construction_filters_disabled_collectors_and_keeps_supplied_directories() {
    let (_guard, logs) = crate::unit_logs::capture();
    let state = Arc::new(std::sync::Mutex::new(
        crate::unit_collectors::State::default(),
    ));
    let _scope = crate::unit_collectors::scope(state.clone());
    let pg = |name: &str, enabled: bool| {
        serde_json::from_value::<PostgresClusterConfig>(serde_json::json!({"name":name,"host":"fixture.invalid","port":5432,"user":"unit","database":"unit","enabled":enabled})).unwrap()
    };
    let maria = |name: &str, enabled: bool| {
        serde_json::from_value::<MariaDBClusterConfig>(serde_json::json!({"name":name,"host":"fixture.invalid","port":3306,"user":"unit","enabled":enabled})).unwrap()
    };
    let sample = sdk::System::default();
    let refreshes = sample.refreshes.clone();
    sdk::next(sample);
    let monitor = SystemMonitor::new(
        7,
        vec![pg("enabled-pg", true), pg("disabled-pg", false)],
        vec![maria("enabled-maria", true), maria("disabled-maria", false)],
        vec!["unit.service".into()],
    )
    .await
    .unwrap()
    .with_tls_dirs(vec!["owned-unit-certs".into()]);
    assert_eq!(monitor.update_interval, Duration::from_secs(7));
    assert_eq!(refreshes.load(AtomicOrdering::Relaxed), 1);
    assert!(!monitor.is_running.load(AtomicOrdering::SeqCst));
    assert_eq!(*monitor.last_update.read().unwrap(), sdk::now());
    let empty = SystemMonitor::new(1, vec![], vec![], vec![]).await.unwrap();
    assert!(empty.postgres_collectors.is_empty());
    assert!(empty.mariadb_collectors.is_empty());
    let state = state.lock().unwrap();
    assert_eq!(state.postgres_names, vec!["enabled-pg"]);
    assert_eq!(state.mariadb_names, vec!["enabled-maria"]);
    assert_eq!(
        state.configured_units,
        vec![vec!["unit.service".to_string()], Vec::<String>::new()]
    );
    assert!(state
        .tls_dirs
        .contains(&vec!["owned-unit-certs".to_owned()]));
    assert!(!logs.lock().unwrap().is_empty());
}

#[test]
fn network_metrics_preserve_addresses_mac_formatting_activity_and_packet_counts() {
    fn ips() -> HashMap<String, String> {
        HashMap::from([("eth-unit".into(), "192.0.2.8".into())])
    }
    let system = sdk::System {
        networks: sdk::Networks(HashMap::from([
            (
                "eth-unit".into(),
                sdk::Network {
                    mac: [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff],
                    received: 2,
                    sent: 3,
                    received_packets: 4,
                    sent_packets: 5,
                },
            ),
            (
                "tx-only".into(),
                sdk::Network {
                    sent: 1,
                    ..Default::default()
                },
            ),
            ("idle".into(), sdk::Network::default()),
        ])),
        ..Default::default()
    };
    let (monitor, _fixture) = fixture(system);
    let networks = monitor.get_network_info_with_provider(ips).unwrap();
    let eth = networks.iter().find(|n| n.interface == "eth-unit").unwrap();
    assert_eq!(eth.ip_address, "192.0.2.8");
    assert_eq!(eth.mac_address, "AA:BB:CC:DD:EE:FF");
    assert_eq!(eth.packets_received, 4);
    assert_eq!(eth.packets_sent, 5);
    assert!(eth.is_up);
    assert!(
        networks
            .iter()
            .find(|n| n.interface == "tx-only")
            .unwrap()
            .is_up
    );
    let idle = networks.iter().find(|n| n.interface == "idle").unwrap();
    assert!(!idle.is_up);
    assert_eq!(idle.ip_address, "N/A");
}

#[tokio::test]
async fn collector_results_are_forwarded_and_database_failures_keep_other_clusters() {
    let (_logs, records) = crate::unit_logs::capture();
    let (mut monitor, fixture) = fixture(sdk::System::default());
    let pg_config = |name: &str| {
        serde_json::from_value::<PostgresClusterConfig>(serde_json::json!({"name":name})).unwrap()
    };
    let maria_config =
        serde_json::from_value::<MariaDBClusterConfig>(serde_json::json!({"name":"maria-unit"}))
            .unwrap();
    monitor.postgres_collectors = vec![
        PostgresCollector::new(pg_config("pg-fails")),
        PostgresCollector::new(pg_config("pg-ok")),
    ];
    monitor.mariadb_collectors = vec![MariaDBCollector::new(maria_config)];
    let pg:shared::types::PostgresClusterInfo=serde_json::from_value(serde_json::json!({"name":"pg-returned","host":"fixture.invalid","port":5432,"databases":[],"connections_total":1,"connections_by_state":[],"cache_hit_ratio":99.,"top_queries":[],"settings":[],"timestamp":sdk::now()})).unwrap();
    let maria:shared::types::MariaDBClusterInfo=serde_json::from_value(serde_json::json!({"name":"maria-returned","host":"fixture.invalid","port":3306,"schemas":[],"connections_active":1,"connections_total":1,"innodb_status":null,"processes":[],"timestamp":sdk::now()})).unwrap();
    {
        let mut state = fixture.state.lock().unwrap();
        state
            .postgres
            .push_back(Err(anyhow::anyhow!("unit postgres refused")));
        state.postgres.push_back(Ok(pg));
        state.mariadb.push_back(Ok(maria));
        state.container_swap.insert("unit-id".into(), 4096);
        state.containers.push_back(Ok(vec![]));
        state
            .containers
            .push_back(Err(anyhow::anyhow!("unit containers refused")));
        state.units.push_back(Ok(vec![]));
        state
            .units
            .push_back(Err(anyhow::anyhow!("unit systemd refused")));
        state.failed.push_back(Ok(vec![]));
        state
            .failed
            .push_back(Err(anyhow::anyhow!("unit failed scan refused")));
        state.ports.push_back(Ok(vec![]));
        state
            .ports
            .push_back(Err(anyhow::anyhow!("unit ports refused")));
    }
    assert_eq!(
        monitor.get_postgres_clusters().await.unwrap()[0].name,
        "pg-returned"
    );
    assert_eq!(
        monitor.get_mariadb_clusters().await.unwrap()[0].name,
        "maria-returned"
    );
    fixture
        .state
        .lock()
        .unwrap()
        .mariadb
        .push_back(Err(anyhow::anyhow!("unit MariaDB refused")));
    assert!(monitor.get_mariadb_clusters().await.unwrap().is_empty());
    assert!(monitor.get_containers().await.unwrap().is_empty());
    assert!(monitor
        .get_containers()
        .await
        .unwrap_err()
        .to_string()
        .contains("unit containers refused"));
    assert_eq!(
        fixture.state.lock().unwrap().seen_container_swap[0].get("unit-id"),
        Some(&4096)
    );
    assert!(monitor.get_systemd_units().await.unwrap().is_empty());
    assert!(monitor.get_systemd_units().await.is_err());
    assert!(monitor.get_systemd_failed_units().await.unwrap().is_empty());
    assert!(monitor.get_systemd_failed_units().await.is_err());
    assert!(monitor.get_listening_ports().await.unwrap().is_empty());
    assert!(monitor.get_listening_ports().await.is_err());
    assert!(records
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("unit postgres refused")));
    assert!(records
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("unit MariaDB refused")));
}
#[tokio::test]
async fn renewal_status_uses_timer_then_service_and_preserves_inventory_errors() {
    let (monitor, fixture) = fixture(sdk::System::default());
    let initial = monitor.get_tls_info().await.unwrap();
    assert!(initial.renewal_unit_name.is_none());
    fixture
        .state
        .lock()
        .unwrap()
        .statuses
        .insert("certbot.service".into(), Some("failed".into()));
    let snapshot = monitor.get_tls_info().await.unwrap();
    assert_eq!(
        snapshot.renewal_unit_name.as_deref(),
        Some("certbot.service")
    );
    assert_eq!(snapshot.renewal_unit_status.as_deref(), Some("failed"));
    fixture
        .state
        .lock()
        .unwrap()
        .statuses
        .insert("certbot.timer".into(), Some("active".into()));
    let snapshot = monitor.get_tls_info().await.unwrap();
    assert_eq!(snapshot.renewal_unit_name.as_deref(), Some("certbot.timer"));
    assert_eq!(snapshot.renewal_unit_status.as_deref(), Some("active"));
    fixture
        .state
        .lock()
        .unwrap()
        .tls
        .push_back(Err(anyhow::anyhow!("unit certificate inventory refused")));
    assert!(monitor
        .get_tls_info()
        .await
        .unwrap_err()
        .to_string()
        .contains("unit certificate inventory refused"));
}
#[test]
fn paging_rate_degrades_on_missing_counters_and_poisoned_locks() {
    let (_guard, logs) = crate::unit_logs::capture();
    let _clock = sdk::clock();
    let (monitor, fixture) = fixture(sdk::System::default());
    SystemMonitor::update_swap_rate(&monitor.swap_activity, &monitor.swap_rate);
    assert_eq!(*monitor.swap_rate.read().unwrap(), (2., 3.));
    for counters in [(10, 20), (15, 27)] {
        fixture.state.lock().unwrap().paging.push_back(Some(
            crate::collectors::memory::PagingCounters {
                pages_in: counters.0,
                pages_out: counters.1,
            },
        ));
        SystemMonitor::update_swap_rate(&monitor.swap_activity, &monitor.swap_rate);
        sdk::advance(1);
    }
    assert_eq!(*monitor.swap_rate.read().unwrap(), (5., 7.));
    poison(monitor.swap_activity.clone());
    fixture.state.lock().unwrap().paging.push_back(Some(
        crate::collectors::memory::PagingCounters {
            pages_in: 16,
            pages_out: 28,
        },
    ));
    SystemMonitor::update_swap_rate(&monitor.swap_activity, &monitor.swap_rate);
    let (other, other_fixture) = self::fixture(sdk::System::default());
    poison(other.swap_rate.clone());
    other_fixture.state.lock().unwrap().paging.push_back(Some(
        crate::collectors::memory::PagingCounters {
            pages_in: 1,
            pages_out: 1,
        },
    ));
    SystemMonitor::update_swap_rate(&other.swap_activity, &other.swap_rate);
    let logs = logs.lock().unwrap();
    assert!(logs
        .iter()
        .any(|s| s.contains("Failed to lock swap activity")));
    assert!(logs.iter().any(|s| s.contains("Failed to lock swap rate")));
}
#[tokio::test]
async fn background_ticks_refresh_metrics_update_paging_and_stop_without_sleep() {
    let (_guard, logs) = crate::unit_logs::capture();
    let _clock = sdk::clock();
    let sample = sdk::System::default();
    let refreshes = sample.refreshes.clone();
    let (monitor, fixture) = fixture(sample);
    let monitor = Arc::new(monitor);
    {
        let mut state = fixture.state.lock().unwrap();
        state
            .paging
            .push_back(Some(crate::collectors::memory::PagingCounters {
                pages_in: 10,
                pages_out: 20,
            }));
        state
            .paging
            .push_back(Some(crate::collectors::memory::PagingCounters {
                pages_in: 15,
                pages_out: 27,
            }));
    }
    let done = Arc::new(AtomicBool::new(false));
    let stop = monitor.clone();
    let completed = done.clone();
    let _plan = crate::unit_runtime::plan(
        Duration::from_secs(2),
        vec![
            Box::new(|| {}),
            Box::new(|| sdk::advance(2)),
            Box::new(move || {
                stop.stop_background_monitoring();
                completed.store(true, AtomicOrdering::SeqCst);
            }),
        ],
    );
    monitor.start_background_monitoring();
    for _ in 0..64 {
        if done.load(AtomicOrdering::SeqCst) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(done.load(AtomicOrdering::SeqCst));
    assert!(!monitor.is_running.load(AtomicOrdering::SeqCst));
    assert_eq!(refreshes.load(AtomicOrdering::Relaxed), 2);
    assert_eq!(*monitor.swap_rate.read().unwrap(), (2.5, 3.5));
    assert_eq!(*monitor.last_update.read().unwrap(), sdk::now());
    assert!(fixture.state.lock().unwrap().paging.is_empty());
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.contains("Background system monitoring stopped")));
}
#[tokio::test]
async fn background_update_error_is_logged_and_the_stop_request_still_works() {
    let (_guard, logs) = crate::unit_logs::capture();
    let (monitor, _fixture) = fixture(sdk::System::default());
    let monitor = Arc::new(monitor);
    let lock = monitor.system.clone();
    let done = Arc::new(AtomicBool::new(false));
    let completed = done.clone();
    let stop = monitor.clone();
    let _plan = crate::unit_runtime::plan(
        Duration::from_secs(2),
        vec![
            Box::new(move || poison(lock)),
            Box::new(move || {
                stop.stop_background_monitoring();
                completed.store(true, AtomicOrdering::SeqCst);
            }),
        ],
    );
    monitor.start_background_monitoring();
    for _ in 0..64 {
        if done.load(AtomicOrdering::SeqCst) {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(done.load(AtomicOrdering::SeqCst));
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|s| s.contains("Failed to update system info")));
}
#[test]
fn malformed_empty_inet_token_is_ignored() {
    assert!(parse_ip_addr_output("1: eth-unit: <UP>\ninet /24\n").is_empty());
}
