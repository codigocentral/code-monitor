//! Direct handler units and transport lifecycle with no HTTP request or socket.
use super::*;
#[tokio::test]
async fn handlers_report_health_readiness_and_atomic_metrics() {
    let Json(status) = health_check().await;
    assert_eq!(status.status, "healthy");
    assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(status.timestamp, crate::unit_sysinfo::now().to_rfc3339());
    for ready in [true, false] {
        let state = Arc::new(HealthState {
            start_time: std::time::Instant::now(),
            ready,
        });
        let (code, Json(result)) = readiness_check(State(state.clone())).await;
        assert_eq!(
            code,
            if ready {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        );
        assert_eq!(result.ready, ready);
        assert_eq!(result.checks.len(), 3);
        assert_eq!(result.checks[0].name, "uptime");
        assert_eq!(result.checks[0].message.as_deref(), Some("123s"));
        assert_eq!(result.checks[1].name, "grpc_service");
        assert_eq!(result.checks[1].message.as_deref(), Some("running"));
        assert_eq!(result.checks[2].status, if ready { "ok" } else { "fail" });
        assert_eq!(result.checks[2].message, None);
        let text = metrics_check(State(state)).await.unwrap();
        assert!(text.contains("code_monitor_uptime_seconds 123\n"));
        for (name, counter) in [
            ("requests_total", &metrics::GRPC_REQUESTS_TOTAL),
            ("auth_failures_total", &metrics::GRPC_AUTH_FAILURES_TOTAL),
            (
                "streams_rejected_total",
                &metrics::GRPC_STREAMS_REJECTED_TOTAL,
            ),
            ("active_streams", &metrics::GRPC_ACTIVE_STREAMS),
        ] {
            assert!(text.contains(&format!(
                "code_monitor_grpc_{name} {}\n",
                metrics::get(counter)
            )));
        }
        assert!(text.contains(&format!(
            "code_monitor_build_info{{version=\"{}\"}} 1\n",
            env!("CARGO_PKG_VERSION")
        )));
    }
}
pub(crate) async fn exercise_log_fallback() {
    *unit_http::STATE.lock().unwrap() = unit_http::State::default();
    start_health_server(8123).await.unwrap();
}
#[tokio::test]
async fn start_propagates_bind_and_serve_errors_and_builds_routes() {
    let (_guard, logs) = crate::unit_logs::capture();
    for (bind_error, serve_error, error) in [
        (true, false, Some("unit bind refused")),
        (false, true, Some("unit serve refused")),
        (false, false, None),
    ] {
        *unit_http::STATE.lock().unwrap() = unit_http::State {
            bind_error,
            serve_error,
            ..Default::default()
        };
        let result = start_health_server(8124).await;
        assert_eq!(result.err().map(|e| e.to_string()).as_deref(), error);
        let state = unit_http::STATE.lock().unwrap();
        assert_eq!(state.addresses, ["0.0.0.0:8124".parse().unwrap()]);
        assert_eq!(state.served, usize::from(!bind_error));
    }
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("/metrics")));
}
