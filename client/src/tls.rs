//! TLS configuration for the gRPC client
//!
//! Supports TLS and optional mTLS (client certificate authentication).
//! When `danger_skip_verify` is true, a custom rustls connector bypasses
//! server certificate verification (insecure, for testing only).

use anyhow::{Context, Result};
use hyper::client::connect::{Connected, Connection, HttpConnector};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{
    rustls::{ClientConfig, ServerName},
    TlsConnector as TokioTlsConnector,
};
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity};
use tower::Service;
use tracing::{info, warn};

use shared::types::ClientTlsConfig as TlsConfig;

/// Custom TLS stream that implements hyper's `Connection`.
pub struct SkipVerifyStream(tokio_rustls::client::TlsStream<tokio::net::TcpStream>);

impl AsyncRead for SkipVerifyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for SkipVerifyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl Connection for SkipVerifyStream {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

/// Connector that skips TLS certificate verification.
#[derive(Clone)]
pub struct SkipVerifyConnector {
    http: HttpConnector,
    tls: TokioTlsConnector,
    domain: String,
}

impl std::fmt::Debug for SkipVerifyConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkipVerifyConnector")
            .field("domain", &self.domain)
            .finish_non_exhaustive()
    }
}

impl Service<http::Uri> for SkipVerifyConnector {
    type Response = SkipVerifyStream;
    type Error = std::io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
        self.http.poll_ready(cx).map_err(std::io::Error::other)
    }

    fn call(&mut self, uri: http::Uri) -> Self::Future {
        let mut http = self.http.clone();
        let tls = self.tls.clone();
        let domain = self.domain.clone();
        Box::pin(async move {
            let tcp = Service::call(&mut http, uri)
                .await
                .map_err(std::io::Error::other)?;
            let server_name = ServerName::try_from(domain.as_str())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            let tls_stream = tls
                .connect(server_name, tcp)
                .await
                .map_err(std::io::Error::other)?;
            Ok(SkipVerifyStream(tls_stream))
        })
    }
}

/// Result of TLS configuration: either a plain endpoint or one with a custom connector.
#[derive(Debug)]
pub struct TlsSetup {
    pub endpoint: Endpoint,
    pub connector: Option<SkipVerifyConnector>,
}

/// Configure TLS for a gRPC endpoint.
///
/// Returns a `TlsSetup` that should be used to connect:
/// - If `connector` is `Some`, use `endpoint.connect_with_connector(connector)`.
/// - Otherwise, use `endpoint.connect()`.
pub fn configure_tls(endpoint: Endpoint, tls_config: &TlsConfig) -> Result<TlsSetup> {
    if tls_config.danger_skip_verify {
        warn!("TLS certificate verification disabled (danger_skip_verify=true)");

        let builder = ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification));

        let mut config = builder.with_no_client_auth();
        config.alpn_protocols.push(b"h2".to_vec());

        let tls = TokioTlsConnector::from(Arc::new(config));
        let domain = endpoint.uri().host().unwrap_or("localhost").to_string();

        let mut http = HttpConnector::new();
        http.enforce_http(false);

        let connector = SkipVerifyConnector { http, tls, domain };

        return Ok(TlsSetup {
            endpoint,
            connector: Some(connector),
        });
    }

    let ca_cert = std::fs::read(&tls_config.ca_cert_path).with_context(|| {
        format!(
            "Failed to read CA certificate from {}",
            tls_config.ca_cert_path
        )
    })?;
    let ca = Certificate::from_pem(ca_cert);

    let mut client_tls = ClientTlsConfig::new().ca_certificate(ca);

    // Optional mTLS: client certificate
    if let (Some(ref cert_path), Some(ref key_path)) =
        (&tls_config.client_cert_path, &tls_config.client_key_path)
    {
        let cert = std::fs::read(cert_path)
            .with_context(|| format!("Failed to read client certificate from {}", cert_path))?;
        let key = std::fs::read(key_path)
            .with_context(|| format!("Failed to read client key from {}", key_path))?;
        let identity = Identity::from_pem(cert, key);
        client_tls = client_tls.identity(identity);
        info!("mTLS configured with client certificate");
    }

    let endpoint = endpoint
        .tls_config(client_tls)
        .context("Failed to apply TLS configuration")?;

    Ok(TlsSetup {
        endpoint,
        connector: None,
    })
}

struct SkipServerVerification;

impl tokio_rustls::rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &tokio_rustls::rustls::Certificate,
        _intermediates: &[tokio_rustls::rustls::Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<tokio_rustls::rustls::client::ServerCertVerified, tokio_rustls::rustls::Error> {
        Ok(tokio_rustls::rustls::client::ServerCertVerified::assertion())
    }
}

/// Check if TLS config is valid (CA cert exists)
pub fn is_tls_valid(config: Option<&TlsConfig>) -> bool {
    match config {
        Some(tls) => {
            if tls.danger_skip_verify {
                return true;
            }
            let exists = std::path::Path::new(&tls.ca_cert_path).exists();
            if !exists {
                warn!("TLS CA certificate not found at {}", tls.ca_cert_path);
            }
            exists
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::Service;

    #[test]
    fn test_is_tls_valid_none() {
        assert!(!is_tls_valid(None));
    }

    #[test]
    fn test_is_tls_valid_danger_skip_verify() {
        let tls = TlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: true,
        };
        assert!(is_tls_valid(Some(&tls)));
    }

    #[test]
    fn test_is_tls_valid_missing_ca() {
        let tls = TlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: false,
        };
        assert!(!is_tls_valid(Some(&tls)));
    }

    #[test]
    fn test_is_tls_valid_ca_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ca_path = temp_dir.path().join("ca.pem");
        std::fs::write(&ca_path, "dummy ca").unwrap();

        let tls = TlsConfig {
            ca_cert_path: ca_path.to_string_lossy().to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: false,
        };
        assert!(is_tls_valid(Some(&tls)));
    }

    #[test]
    fn test_configure_tls_missing_ca() {
        let tls = TlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: false,
        };
        let endpoint = Endpoint::from_static("https://localhost:50051");
        let result = configure_tls(endpoint, &tls);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read CA certificate"));
    }

    #[test]
    fn test_configure_tls_missing_client_cert() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ca_path = temp_dir.path().join("ca.pem");
        std::fs::write(&ca_path, "dummy ca").unwrap();

        let tls = TlsConfig {
            ca_cert_path: ca_path.to_string_lossy().to_string(),
            client_cert_path: Some("/nonexistent/client.crt".to_string()),
            client_key_path: Some("/nonexistent/client.key".to_string()),
            danger_skip_verify: false,
        };
        let endpoint = Endpoint::from_static("https://localhost:50051");
        let result = configure_tls(endpoint, &tls);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read client certificate"));
    }

    #[test]
    fn test_configure_tls_skip_verify() {
        let tls = TlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: true,
        };
        let endpoint = Endpoint::from_static("https://localhost:50051");
        let result = configure_tls(endpoint, &tls);
        assert!(result.is_ok());
        let setup = result.unwrap();
        assert!(setup.connector.is_some());
    }

    #[test]
    fn test_skip_verify_connector_fmt() {
        let tls = TlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: true,
        };
        let endpoint = Endpoint::from_static("https://localhost:50051");
        let setup = configure_tls(endpoint, &tls).unwrap();
        let connector = setup.connector.unwrap();
        let debug = format!("{:?}", connector);
        assert!(debug.contains("SkipVerifyConnector"));
        assert!(debug.contains("localhost"));
    }

    #[test]
    fn test_skip_verify_connector_poll_ready() {
        use std::sync::Arc;
        use std::task::{Context, Wake, Waker};

        struct DummyWaker;
        impl Wake for DummyWaker {
            fn wake(self: Arc<Self>) {}
        }

        let tls = TlsConfig {
            ca_cert_path: "/nonexistent/ca.pem".to_string(),
            client_cert_path: None,
            client_key_path: None,
            danger_skip_verify: true,
        };
        let endpoint = Endpoint::from_static("https://localhost:50051");
        let setup = configure_tls(endpoint, &tls).unwrap();
        let mut connector = setup.connector.unwrap();

        let waker = Waker::from(Arc::new(DummyWaker));
        let mut cx = Context::from_waker(&waker);
        let result = connector.poll_ready(&mut cx);
        assert!(matches!(result, Poll::Ready(Ok(()))));
    }

    #[test]
    fn test_skip_server_verification() {
        use tokio_rustls::rustls::client::ServerCertVerifier;
        let verifier = SkipServerVerification;
        let result = verifier.verify_server_cert(
            &tokio_rustls::rustls::Certificate(vec![]),
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &mut std::iter::empty(),
            &[],
            std::time::SystemTime::now(),
        );
        assert!(result.is_ok());
    }

}
