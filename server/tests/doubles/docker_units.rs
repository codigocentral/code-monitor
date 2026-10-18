//! Real Docker collector against typed SDK response queues, no daemon.
use super::*;
use crate::{collectors::Collector, unit_bollard as sdk};
use serde_json::json;
use std::{collections::HashMap, sync::Mutex};
fn fixture() -> (DockerCollector, sdk::SharedState) {
    let state = Arc::new(Mutex::new(sdk::State::default()));
    (
        DockerCollector {
            docker: Some(Docker(state.clone())),
            _socket_path: "unit.sock".into(),
            connection_failed: AtomicBool::new(false),
            versions_collector: Arc::new(VersionsCollector::default()),
        },
        state,
    )
}
fn stats(cpu: u64, system: u64, networks: bool) -> sdk::container::Stats {
    let cpu_fields = |usage, system| json!({"cpu_usage":{"total_usage":usage,"usage_in_usermode":0,"usage_in_kernelmode":0},"system_cpu_usage":system,"online_cpus":2,"throttling_data":{"periods":0,"throttled_periods":0,"throttled_time":0}});
    let mut value = json!({"read":"2026-01-01T00:00:00Z","preread":"2026-01-01T00:00:00Z","num_procs":1,"pids_stats":{},"memory_stats":{"usage":50,"limit":1000},"blkio_stats":{},"storage_stats":{},"cpu_stats":cpu_fields(cpu,system),"precpu_stats":cpu_fields(10,100)});
    if networks {
        value["networks"] = json!({"eth0":{"rx_bytes":3,"tx_bytes":4,"rx_dropped":0,"rx_errors":0,"tx_packets":0,"tx_dropped":0,"rx_packets":0,"tx_errors":0},"eth1":{"rx_bytes":5,"tx_bytes":6,"rx_dropped":0,"rx_errors":0,"tx_packets":0,"tx_dropped":0,"rx_packets":0,"tx_errors":0}});
    }
    serde_json::from_value(value).unwrap()
}
#[test]
fn constructors_preserve_socket_defaults_env_and_failures() {
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(value) = &self.0 {
                std::env::set_var("DOCKER_SOCKET", value);
            } else {
                std::env::remove_var("DOCKER_SOCKET");
            }
        }
    }
    let _restore = Restore(std::env::var_os("DOCKER_SOCKET"));
    let (_guard, _logs) = crate::unit_logs::capture();
    for (custom, success) in [(false, false), (false, true), (true, false), (true, true)] {
        if custom {
            std::env::set_var("DOCKER_SOCKET", "unit-custom.sock");
        } else {
            std::env::remove_var("DOCKER_SOCKET");
        }
        sdk::connect_next(if success {
            Ok(Arc::new(Mutex::new(sdk::State::default())))
        } else {
            Err(sdk::error("unit constructor refused"))
        });
        let collector = DockerCollector::new();
        assert_eq!(collector.is_available(), success);
        assert_eq!(collector.is_enabled(), success);
        assert_eq!(
            collector._socket_path,
            if custom {
                "unit-custom.sock"
            } else {
                "/var/run/docker.sock"
            }
        );
        sdk::connect_next(if success {
            Ok(Arc::new(Mutex::new(sdk::State::default())))
        } else {
            Err(sdk::error("unit constructor refused"))
        });
        let collector = DockerCollector::with_socket_path("unit-explicit.sock");
        assert_eq!(collector.is_available(), success);
        assert_eq!(
            sdk::constructor_calls().last(),
            Some(&("unit-explicit.sock".into(), 120))
        );
    }
}
#[tokio::test]
async fn list_failure_disables_connection_once_and_preserves_retryable_errors() {
    let (_guard, _logs) = crate::unit_logs::capture();
    for (connect, racing) in [(false, false), (true, false), (true, true)] {
        let (collector, state) = fixture();
        let collector = Arc::new(collector);
        {
            let mut state = state.lock().unwrap();
            state.lists.push_back(Err(sdk::error(if connect {
                "CONNECT refused"
            } else {
                "permission refused"
            })));
            if racing {
                let own = collector.clone();
                state.on_list = Some(Box::new(move || {
                    own.connection_failed.store(true, Ordering::SeqCst);
                }));
            }
        }
        assert!(collector
            .collect_containers(&HashMap::new())
            .await
            .unwrap()
            .is_empty());
        assert_eq!(collector.is_available(), !connect);
        if connect {
            assert!(collector
                .collect_containers(&HashMap::new())
                .await
                .unwrap()
                .is_empty());
            assert_eq!(state.lock().unwrap().calls.len(), 1);
        }
    }
}
#[tokio::test]
async fn statistics_compute_deltas_network_sums_and_propagate_empty_or_failed_streams() {
    let (collector, state) = fixture();
    let docker = collector.docker.as_ref().unwrap();
    for (cpu, system, expected, networks) in [
        (20, 200, 20., true),
        (10, 200, 0., false),
        (20, 100, 0., false),
    ] {
        state
            .lock()
            .unwrap()
            .stats
            .push_back(("unit".into(), vec![Ok(stats(cpu, system, networks))]));
        let result = collector.get_container_stats(docker, "unit").await.unwrap();
        assert_eq!(
            result,
            (
                expected,
                50,
                1000,
                if networks { 8 } else { 0 },
                if networks { 10 } else { 0 }
            )
        );
    }
    state
        .lock()
        .unwrap()
        .stats
        .push_back(("unit".into(), vec![]));
    assert_eq!(
        collector
            .get_container_stats(docker, "unit")
            .await
            .unwrap_err()
            .to_string(),
        "No stats received"
    );
    state
        .lock()
        .unwrap()
        .stats
        .push_back(("unit".into(), vec![Err(sdk::error("unit stats refused"))]));
    assert!(collector
        .get_container_stats(docker, "unit")
        .await
        .unwrap_err()
        .to_string()
        .contains("stats refused"));
    let mut missing = stats(20, 200, false);
    missing.cpu_stats.online_cpus = None;
    missing.cpu_stats.system_cpu_usage = None;
    missing.precpu_stats.system_cpu_usage = None;
    missing.memory_stats.usage = None;
    missing.memory_stats.limit = None;
    state
        .lock()
        .unwrap()
        .stats
        .push_back(("unit".into(), vec![Ok(missing)]));
    assert_eq!(
        collector.get_container_stats(docker, "unit").await.unwrap(),
        (0., 0, 0, 0, 0)
    );
}
#[tokio::test]
async fn container_inventory_joins_inspect_stats_swap_and_optional_fields() {
    let (_guard, _logs) = crate::unit_logs::capture();
    let (collector, state) = fixture();
    let containers = vec![
        json!({"Id":"running","Names":["/Unit"],"Image":"unit:tag","State":"running","Status":"Up","NetworkSettings":{"Networks":{"unit-net":{}}}}),
        json!({"Id":"unhealthy","Names":[],"State":"running","Status":"Up (unhealthy)"}),
        json!({"Id":"healthy","State":"exited","Status":"Up (healthy)"}),
        json!({"Id":"plain","State":"exited","Status":"Stopped"}),
        json!({"State":"running"}),
    ];
    {
        let mut state = state.lock().unwrap();
        state.lists.push_back(Ok(containers
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect()));
        state
            .stats
            .push_back(("running".into(), vec![Ok(stats(20, 200, true))]));
        state.stats.push_back((
            "unhealthy".into(),
            vec![Err(sdk::error("unit stats unavailable"))],
        ));
        state.inspections.push_back(("running".into(),Ok(serde_json::from_value(json!({"RestartCount":3,"HostConfig":{"Memory":100},"State":{"Health":{"Status":"healthy","FailingStreak":0,"Log":[]}}})).unwrap())));
        for id in ["unhealthy", "healthy", "plain"] {
            state
                .inspections
                .push_back((id.into(), Err(sdk::error("unit inspect refused"))));
        }
    }
    let result = collector
        .collect_containers(&HashMap::from([("running".into(), 7)]))
        .await
        .unwrap();
    assert_eq!(result.len(), 5);
    let first = &result[0];
    assert_eq!(first.name, "Unit");
    assert_eq!(first.memory_limit_bytes, 100);
    assert_eq!(first.memory_percent, Some(50.));
    assert_eq!(first.cpu_percent, 20.);
    assert_eq!(first.networks, ["unit-net"]);
    assert_eq!(first.restart_count, 3);
    assert_eq!(first.swap_bytes, Some(7));
    assert_eq!(
        first.image_version.as_ref().unwrap().status,
        shared::types::VersionStatus::LocalBuild
    );
    assert_eq!(result[1].health, "unhealthy");
    assert_eq!(result[1].memory_usage_bytes, 0);
    assert_eq!(result[2].health, "healthy");
    assert_eq!(result[3].health, "none");
    assert!(result[4].id.is_empty());
    assert_eq!(result[4].name, "unknown");
    assert!(result[4].image_version.is_none());
}
