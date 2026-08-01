//! TLS certificate collector
//!
//! Inventories the certificates a host serves and the health of whatever
//! renews them. An expired certificate takes a site down, and the expiry date
//! is written inside the certificate — it is the most predictable outage there
//! is.
//!
//! Not implemented yet: this is the passthrough that keeps the contract, the
//! transport and the UI wiring in place so the collection itself lands as a
//! self-contained change.

use anyhow::Result;
use shared::types::TlsSnapshot;

/// Collector for TLS certificates
pub struct TlsCollector {
    /// Directories holding certificate lineages, e.g. `/etc/letsencrypt/live`
    #[allow(dead_code)]
    certificate_dirs: Vec<String>,
}

impl TlsCollector {
    pub fn new(certificate_dirs: Vec<String>) -> Self {
        Self { certificate_dirs }
    }

    /// Collect the certificate inventory.
    ///
    /// Returns an empty snapshot until the collection is implemented. Callers
    /// must treat that as "nothing known", never as "nothing expiring".
    pub async fn collect(&self) -> Result<TlsSnapshot> {
        Ok(TlsSnapshot::default())
    }
}

impl Default for TlsCollector {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tls_collector_returns_empty_snapshot() {
        let collector = TlsCollector::default();
        let snapshot = collector.collect().await.unwrap();

        assert!(snapshot.certificates.is_empty());
        assert!(snapshot.renewal_unit_name.is_none());
    }

    #[tokio::test]
    async fn test_tls_collector_never_errors_on_missing_dirs() {
        let collector = TlsCollector::new(vec!["/nonexistent/letsencrypt/live".to_string()]);
        assert!(collector.collect().await.is_ok());
    }
}
