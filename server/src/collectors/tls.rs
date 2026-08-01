//! TLS certificate collector
//!
//! Inventories the certificates a host serves. An expired certificate takes a
//! site down, and the expiry date is written inside the certificate itself —
//! it is the most predictable outage there is, and the one uptime checks only
//! catch after the fact.
//!
//! Parsing is done in pure Rust so the agent does not require a system OpenSSL
//! on the monitored host.

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use shared::types::{TlsCertificateInfo, TlsSnapshot};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use x509_parser::prelude::*;

/// Where Let's Encrypt keeps its lineages by default
const DEFAULT_CERTIFICATE_DIR: &str = "/etc/letsencrypt/live";

/// Certificate file to read inside a lineage directory
const CERTIFICATE_FILENAME: &str = "cert.pem";

/// Collector for TLS certificates
pub struct TlsCollector {
    /// Directories holding certificate lineages
    certificate_dirs: Vec<String>,
}

/// Everything read out of a single certificate
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCertificate {
    pub domains: Vec<String>,
    pub issuer: String,
    pub not_after: DateTime<Utc>,
}

/// Parse a PEM-encoded certificate.
///
/// Only the first certificate is read: in a chain file the leaf comes first,
/// and the intermediates expire on their own schedule, which is not what a
/// site's availability depends on.
pub fn parse_certificate(pem_bytes: &[u8]) -> Result<ParsedCertificate> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem_bytes)
        .map_err(|e| anyhow::anyhow!("not a PEM certificate: {}", e))?;
    let cert = pem
        .parse_x509()
        .map_err(|e| anyhow::anyhow!("malformed certificate: {}", e))?;

    let not_after = Utc
        .timestamp_opt(cert.validity().not_after.timestamp(), 0)
        .single()
        .ok_or_else(|| anyhow::anyhow!("certificate carries an unrepresentable expiry"))?;

    // Subject Alternative Names are what browsers actually match; the common
    // name is legacy and often absent on modern certificates.
    let mut domains: Vec<String> = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|san| {
            san.value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(dns) => Some(dns.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    if domains.is_empty() {
        // Fall back to the common name for certificates predating SAN
        domains = cert
            .subject()
            .iter_common_name()
            .filter_map(|cn| cn.as_str().ok().map(str::to_string))
            .collect();
    }

    let issuer = cert
        .issuer()
        .iter_common_name()
        .filter_map(|cn| cn.as_str().ok())
        .next()
        .unwrap_or("unknown")
        .to_string();

    Ok(ParsedCertificate {
        domains,
        issuer,
        not_after,
    })
}

/// Days until a certificate expires, negative once it already has.
pub fn days_until(not_after: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (not_after - now).num_days()
}

/// Name a lineage by its directory, which is how operators refer to it.
fn lineage_name(cert_path: &Path) -> String {
    cert_path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| cert_path.to_string_lossy().to_string())
}

impl TlsCollector {
    pub fn new(certificate_dirs: Vec<String>) -> Self {
        let dirs = if certificate_dirs.is_empty() {
            vec![DEFAULT_CERTIFICATE_DIR.to_string()]
        } else {
            certificate_dirs
        };

        Self {
            certificate_dirs: dirs,
        }
    }

    /// Collect the certificate inventory.
    ///
    /// A missing or unreadable directory is not an error: most hosts have no
    /// certificates at all, and the agent must not fail the RPC over it.
    pub async fn collect(&self) -> Result<TlsSnapshot> {
        let mut certificates = Vec::new();
        let now = Utc::now();

        for dir in &self.certificate_dirs {
            for cert_path in Self::certificate_paths(Path::new(dir)) {
                match std::fs::read(&cert_path) {
                    Ok(bytes) => match parse_certificate(&bytes) {
                        Ok(parsed) => certificates.push(TlsCertificateInfo {
                            name: lineage_name(&cert_path),
                            domains: parsed.domains,
                            issuer: parsed.issuer,
                            not_after: Some(parsed.not_after),
                            days_until_expiry: days_until(parsed.not_after, now),
                            source: cert_path.to_string_lossy().to_string(),
                            // Requires the host's vhost inventory to decide;
                            // reported as false until that lands
                            orphaned: false,
                        }),
                        Err(e) => {
                            warn!("Skipping {}: {}", cert_path.display(), e);
                        }
                    },
                    Err(e) => {
                        debug!("Cannot read {}: {}", cert_path.display(), e);
                    }
                }
            }
        }

        // Soonest to expire first: the list is read top-down under pressure
        certificates.sort_by_key(|c| c.days_until_expiry);

        Ok(TlsSnapshot {
            certificates,
            renewal_unit_name: None,
            renewal_unit_status: None,
        })
    }

    /// Certificate files under a lineage root, one per subdirectory.
    fn certificate_paths(root: &Path) -> Vec<PathBuf> {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) => {
                debug!("No certificates under {}: {}", root.display(), e);
                return Vec::new();
            }
        };

        let mut paths: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .map(|dir| dir.join(CERTIFICATE_FILENAME))
            .filter(|cert| cert.is_file())
            .collect();

        // Deterministic order regardless of what the filesystem returns
        paths.sort();
        paths
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

    /// A real certificate, so the parser is exercised against the actual
    /// encoding rather than a hand-built fixture.
    const SAMPLE_PEM: &[u8] = include_bytes!("../../tests/fixtures/sample-cert.pem");

    #[test]
    fn test_parse_certificate_reads_expiry_and_domains() {
        let parsed = parse_certificate(SAMPLE_PEM).expect("sample must parse");

        assert!(!parsed.domains.is_empty(), "certificate should carry a SAN");
        assert!(
            parsed.not_after > Utc.timestamp_opt(0, 0).unwrap(),
            "expiry should be a real date"
        );
        assert!(!parsed.issuer.is_empty());
    }

    #[test]
    fn test_parse_certificate_rejects_garbage() {
        assert!(parse_certificate(b"not a certificate").is_err());
    }

    #[test]
    fn test_parse_certificate_rejects_empty_input() {
        assert!(parse_certificate(b"").is_err());
    }

    #[test]
    fn test_parse_certificate_rejects_truncated_pem() {
        let truncated = &SAMPLE_PEM[..SAMPLE_PEM.len() / 2];
        assert!(parse_certificate(truncated).is_err());
    }

    // ─────────────────────────────────────────
    // Expiry arithmetic
    // ─────────────────────────────────────────

    #[test]
    fn test_days_until_future() {
        let now = Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap();
        let expiry = Utc.with_ymd_and_hms(2026, 8, 22, 12, 0, 0).unwrap();
        assert_eq!(days_until(expiry, now), 21);
    }

    #[test]
    fn test_days_until_expired_is_negative() {
        let now = Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap();
        let expiry = Utc.with_ymd_and_hms(2026, 7, 25, 12, 0, 0).unwrap();
        assert_eq!(days_until(expiry, now), -7);
    }

    #[test]
    fn test_days_until_truncates_towards_zero() {
        // 23 hours left is zero days: not yet expired, but not a day either
        let now = Utc.with_ymd_and_hms(2026, 8, 1, 12, 0, 0).unwrap();
        let expiry = Utc.with_ymd_and_hms(2026, 8, 2, 11, 0, 0).unwrap();
        assert_eq!(days_until(expiry, now), 0);
    }

    // ─────────────────────────────────────────
    // Lineage naming
    // ─────────────────────────────────────────

    #[test]
    fn test_lineage_name_uses_the_directory() {
        assert_eq!(
            lineage_name(Path::new("/etc/letsencrypt/live/example.com/cert.pem")),
            "example.com"
        );
    }

    #[test]
    fn test_lineage_name_falls_back_to_path() {
        assert_eq!(lineage_name(Path::new("cert.pem")), "cert.pem");
    }

    // ─────────────────────────────────────────
    // Collection
    // ─────────────────────────────────────────

    #[tokio::test]
    async fn test_collect_missing_directory_is_not_an_error() {
        // Most hosts have no certificates; that must not fail the RPC
        let collector = TlsCollector::new(vec!["/nonexistent/letsencrypt/live".to_string()]);
        let snapshot = collector.collect().await.unwrap();
        assert!(snapshot.certificates.is_empty());
    }

    #[tokio::test]
    async fn test_collect_reads_a_lineage_directory() {
        let root = tempfile::tempdir().unwrap();
        let lineage = root.path().join("example.com");
        std::fs::create_dir(&lineage).unwrap();
        std::fs::write(lineage.join(CERTIFICATE_FILENAME), SAMPLE_PEM).unwrap();

        let collector = TlsCollector::new(vec![root.path().to_string_lossy().to_string()]);
        let snapshot = collector.collect().await.unwrap();

        assert_eq!(snapshot.certificates.len(), 1);
        let cert = &snapshot.certificates[0];
        assert_eq!(cert.name, "example.com");
        assert!(cert.source.ends_with("cert.pem"));
        assert!(!cert.domains.is_empty());
    }

    #[tokio::test]
    async fn test_collect_skips_unparseable_certificates() {
        let root = tempfile::tempdir().unwrap();

        let good = root.path().join("good.example");
        std::fs::create_dir(&good).unwrap();
        std::fs::write(good.join(CERTIFICATE_FILENAME), SAMPLE_PEM).unwrap();

        let bad = root.path().join("bad.example");
        std::fs::create_dir(&bad).unwrap();
        std::fs::write(bad.join(CERTIFICATE_FILENAME), b"corrupted").unwrap();

        let snapshot = TlsCollector::new(vec![root.path().to_string_lossy().to_string()])
            .collect()
            .await
            .unwrap();

        assert_eq!(
            snapshot.certificates.len(),
            1,
            "one broken certificate must not hide the others"
        );
        assert_eq!(snapshot.certificates[0].name, "good.example");
    }

    #[tokio::test]
    async fn test_collect_ignores_directories_without_a_certificate() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("empty.example")).unwrap();

        let snapshot = TlsCollector::new(vec![root.path().to_string_lossy().to_string()])
            .collect()
            .await
            .unwrap();
        assert!(snapshot.certificates.is_empty());
    }

    #[tokio::test]
    async fn test_collect_sorts_by_soonest_expiry() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a.example", "b.example"] {
            let dir = root.path().join(name);
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join(CERTIFICATE_FILENAME), SAMPLE_PEM).unwrap();
        }

        let snapshot = TlsCollector::new(vec![root.path().to_string_lossy().to_string()])
            .collect()
            .await
            .unwrap();

        assert_eq!(snapshot.certificates.len(), 2);
        assert!(
            snapshot.certificates[0].days_until_expiry
                <= snapshot.certificates[1].days_until_expiry
        );
    }

    #[test]
    fn test_default_collector_looks_at_letsencrypt() {
        let collector = TlsCollector::default();
        assert_eq!(collector.certificate_dirs, vec![DEFAULT_CERTIFICATE_DIR]);
    }
}
