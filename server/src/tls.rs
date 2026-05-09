//! TLS configuration for the gRPC server
//!
//! Supports TLS and optional mTLS (client certificate verification).

use anyhow::{Context, Result};
use tonic::transport::{Certificate, Identity, ServerTlsConfig};
use tracing::{info, warn};

use crate::config::TlsConfig;

/// Load TLS configuration for the gRPC server
pub fn load_tls_config(config: &TlsConfig) -> Result<ServerTlsConfig> {
    let cert = std::fs::read(&config.cert_path)
        .with_context(|| format!("Failed to read TLS certificate from {}", config.cert_path))?;
    let key = std::fs::read(&config.key_path)
        .with_context(|| format!("Failed to read TLS key from {}", config.key_path))?;

    let identity = Identity::from_pem(cert, key);

    let mut tls_config = ServerTlsConfig::new().identity(identity);

    if let Some(ref ca_path) = config.ca_path {
        let ca_cert = std::fs::read(ca_path)
            .with_context(|| format!("Failed to read CA certificate from {}", ca_path))?;
        let ca = Certificate::from_pem(ca_cert);
        tls_config = tls_config.client_ca_root(ca);
        info!("mTLS enabled: client certificates will be verified");
    }

    Ok(tls_config)
}

/// Check if TLS is configured and certs exist
pub fn is_tls_available(config: &Option<TlsConfig>) -> bool {
    match config {
        Some(tls) => {
            let cert_exists = std::path::Path::new(&tls.cert_path).exists();
            let key_exists = std::path::Path::new(&tls.key_path).exists();
            if !cert_exists {
                warn!("TLS certificate not found at {}", tls.cert_path);
            }
            if !key_exists {
                warn!("TLS key not found at {}", tls.key_path);
            }
            cert_exists && key_exists
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tls_available_none() {
        assert!(!is_tls_available(&None));
    }

    #[test]
    fn test_is_tls_available_missing_files() {
        let tls = TlsConfig {
            cert_path: "/nonexistent/cert.pem".to_string(),
            key_path: "/nonexistent/key.pem".to_string(),
            ca_path: None,
        };
        assert!(!is_tls_available(&Some(tls)));
    }

    #[test]
    fn test_is_tls_available_cert_only() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        std::fs::write(&cert_path, "dummy cert").unwrap();

        let tls = TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: "/nonexistent/key.pem".to_string(),
            ca_path: None,
        };
        assert!(!is_tls_available(&Some(tls)));
    }

    #[test]
    fn test_is_tls_available_both_exist() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");
        std::fs::write(&cert_path, "dummy cert").unwrap();
        std::fs::write(&key_path, "dummy key").unwrap();

        let tls = TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: key_path.to_string_lossy().to_string(),
            ca_path: None,
        };
        assert!(is_tls_available(&Some(tls)));
    }

    #[test]
    fn test_load_tls_config_missing_cert() {
        let tls = TlsConfig {
            cert_path: "/nonexistent/cert.pem".to_string(),
            key_path: "/nonexistent/key.pem".to_string(),
            ca_path: None,
        };
        let result = load_tls_config(&tls);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read TLS certificate"));
    }

    #[test]
    fn test_load_tls_config_missing_key() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        std::fs::write(&cert_path, "dummy cert").unwrap();

        let tls = TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: "/nonexistent/key.pem".to_string(),
            ca_path: None,
        };
        let result = load_tls_config(&tls);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to read TLS key"));
    }

    #[test]
    fn test_load_tls_config_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");
        std::fs::write(&cert_path, "dummy cert").unwrap();
        std::fs::write(&key_path, "dummy key").unwrap();

        let tls = TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: key_path.to_string_lossy().to_string(),
            ca_path: None,
        };
        let result = load_tls_config(&tls);
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_tls_config_with_ca() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");
        let ca_path = temp_dir.path().join("ca.pem");
        std::fs::write(&cert_path, "dummy cert").unwrap();
        std::fs::write(&key_path, "dummy key").unwrap();
        std::fs::write(&ca_path, "dummy ca").unwrap();

        let tls = TlsConfig {
            cert_path: cert_path.to_string_lossy().to_string(),
            key_path: key_path.to_string_lossy().to_string(),
            ca_path: Some(ca_path.to_string_lossy().to_string()),
        };
        let result = load_tls_config(&tls);
        assert!(result.is_ok());
    }
}
