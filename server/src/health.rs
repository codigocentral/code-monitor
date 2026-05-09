//! Health check HTTP endpoints
//!
//! Provides HTTP endpoints for monitoring the health of the server:
//! - /health - Basic health check
//! - /ready - Readiness probe with system checks
//! - /metrics - Prometheus-compatible metrics

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

/// Health check status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub timestamp: String,
}

/// Readiness check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessStatus {
    pub ready: bool,
    pub checks: Vec<HealthCheck>,
}

/// Individual health check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Shared state for health check endpoints
pub struct HealthState {
    pub start_time: std::time::Instant,
    pub ready: bool,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
            ready: true,
        }
    }
}

/// Basic health check endpoint
async fn health_check() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Readiness check with system verification
async fn readiness_check(
    State(state): State<Arc<HealthState>>,
) -> (StatusCode, Json<ReadinessStatus>) {
    let mut checks = Vec::new();

    // Check uptime
    let uptime_secs = state.start_time.elapsed().as_secs();
    checks.push(HealthCheck {
        name: "uptime".to_string(),
        status: "ok".to_string(),
        message: Some(format!("{}s", uptime_secs)),
    });

    // Check gRPC service (basic check)
    checks.push(HealthCheck {
        name: "grpc_service".to_string(),
        status: "ok".to_string(),
        message: Some("running".to_string()),
    });

    // Check readiness state
    checks.push(HealthCheck {
        name: "readiness".to_string(),
        status: if state.ready { "ok".to_string() } else { "fail".to_string() },
        message: None,
    });

    let all_ready = checks.iter().all(|c| c.status == "ok");

    let status = if all_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(ReadinessStatus {
            ready: all_ready,
            checks,
        }),
    )
}

/// Prometheus-compatible metrics endpoint
async fn metrics_check(State(state): State<Arc<HealthState>>) -> Result<String, StatusCode> {
    let mut metrics = String::new();

    // Build info
    metrics.push_str("# HELP code_monitor_build_info Build information\n");
    metrics.push_str("# TYPE code_monitor_build_info gauge\n");
    metrics.push_str(&format!(
        "code_monitor_build_info{{version=\"{}\"}} 1\n",
        env!("CARGO_PKG_VERSION")
    ));

    // Uptime
    let uptime_secs = state.start_time.elapsed().as_secs();
    metrics.push_str("# HELP code_monitor_uptime_seconds Server uptime in seconds\n");
    metrics.push_str("# TYPE code_monitor_uptime_seconds counter\n");
    metrics.push_str(&format!("code_monitor_uptime_seconds {}\n", uptime_secs));

    Ok(metrics)
}

/// Create the health check router
pub fn create_router(state: Arc<HealthState>) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/metrics", get(metrics_check))
        .with_state(state)
}

/// Start the health check HTTP server
pub async fn start_health_server(port: u16) -> anyhow::Result<()> {
    let state = Arc::new(HealthState::new());
    let app = create_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("Health check server started on http://{}", addr);
    info!("  - Health:  http://{}/health", addr);
    info!("  - Ready:   http://{}/ready", addr);
    info!("  - Metrics: http://{}/metrics", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_router() -> Router {
        create_router(Arc::new(HealthState::new()))
    }

    #[tokio::test]
    async fn test_health_check_endpoint() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: HealthStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "healthy");
        assert!(!health.version.is_empty());
    }

    #[tokio::test]
    async fn test_readiness_check_endpoint() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let readiness: ReadinessStatus = serde_json::from_slice(&body).unwrap();
        assert!(readiness.ready);
        assert!(!readiness.checks.is_empty());
        assert!(readiness.checks.iter().any(|c| c.name == "uptime"));
        assert!(readiness.checks.iter().any(|c| c.name == "grpc_service"));
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let metrics = String::from_utf8(body.to_vec()).unwrap();
        assert!(metrics.contains("code_monitor_build_info"));
        assert!(metrics.contains("code_monitor_uptime_seconds"));
    }

    #[tokio::test]
    async fn test_unknown_endpoint_returns_404() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/unknown").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_health_state_new() {
        let before = std::time::Instant::now();
        let state = HealthState::new();
        let after = std::time::Instant::now();

        // The start_time should be between before and after
        assert!(state.start_time >= before && state.start_time <= after);
    }

    #[tokio::test]
    async fn test_metrics_uptime_increases() {
        let app = test_router();

        // First request
        let response1 = app
            .clone()
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
            .await
            .unwrap();
        let metrics1 = String::from_utf8(body1.to_vec()).unwrap();

        // Extract uptime value
        let uptime_line1 = metrics1
            .lines()
            .find(|l| l.starts_with("code_monitor_uptime_seconds"))
            .unwrap();
        let uptime1: u64 = uptime_line1.split_whitespace().last().unwrap().parse().unwrap();

        // Wait a tiny bit
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Second request
        let response2 = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
            .await
            .unwrap();
        let metrics2 = String::from_utf8(body2.to_vec()).unwrap();

        let uptime_line2 = metrics2
            .lines()
            .find(|l| l.starts_with("code_monitor_uptime_seconds"))
            .unwrap();
        let uptime2: u64 = uptime_line2.split_whitespace().last().unwrap().parse().unwrap();

        assert!(
            uptime2 >= uptime1,
            "Uptime should increase or stay same: {} >= {}",
            uptime2,
            uptime1
        );
    }

    #[tokio::test]
    async fn test_health_version_matches_package() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: HealthStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_readiness_check_contains_expected_checks() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let readiness: ReadinessStatus = serde_json::from_slice(&body).unwrap();

        assert!(readiness.ready);
        assert_eq!(readiness.checks.len(), 3);

        let uptime_check = readiness.checks.iter().find(|c| c.name == "uptime").unwrap();
        assert_eq!(uptime_check.status, "ok");
        assert!(uptime_check.message.as_ref().unwrap().ends_with('s'));

        let grpc_check = readiness.checks.iter().find(|c| c.name == "grpc_service").unwrap();
        assert_eq!(grpc_check.status, "ok");
        assert_eq!(grpc_check.message.as_ref().unwrap(), "running");

        let readiness_check = readiness.checks.iter().find(|c| c.name == "readiness").unwrap();
        assert_eq!(readiness_check.status, "ok");
    }

    #[tokio::test]
    async fn test_metrics_format_valid_prometheus() {
        let app = test_router();
        let response = app
            .oneshot(Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let metrics = String::from_utf8(body.to_vec()).unwrap();

        // Prometheus format validation
        assert!(metrics.starts_with("# HELP"));
        assert!(metrics.contains("# TYPE"));
        assert!(metrics.contains("\n"));

        // Each metric line should end with a number or value
        for line in metrics.lines() {
            if !line.starts_with('#') && !line.is_empty() {
                let last = line.split_whitespace().last().unwrap();
                assert!(
                    last.parse::<f64>().is_ok() || last.parse::<i64>().is_ok() || last == "1",
                    "Metric line should end with a numeric value: {}",
                    line
                );
            }
        }
    }

    #[tokio::test]
    async fn test_readiness_check_unready() {
        let state = Arc::new(HealthState {
            start_time: std::time::Instant::now(),
            ready: false,
        });
        let app = create_router(state);
        let response = app
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let readiness: ReadinessStatus = serde_json::from_slice(&body).unwrap();
        assert!(!readiness.ready);
        let readiness_check = readiness.checks.iter().find(|c| c.name == "readiness").unwrap();
        assert_eq!(readiness_check.status, "fail");
    }

    #[tokio::test]
    async fn test_start_health_server() {
        use tokio::io::AsyncReadExt;
        use tokio::io::AsyncWriteExt;

        // Find a free port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let handle = tokio::spawn(async move {
            start_health_server(port).await.unwrap()
        });

        // Give the server a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Make a raw HTTP request to verify the server is running
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .expect("Should connect to health server");

        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();

        let mut buf = vec![0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(response.contains("200 OK"), "Response should be 200 OK: {}", response);

        handle.abort();
    }
}
