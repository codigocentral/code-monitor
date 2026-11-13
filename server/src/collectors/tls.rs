//! TLS certificate collector
//!
//! Inventories the certificates a host serves. An expired certificate takes a
//! site down, and the expiry date is written inside the certificate itself —
//! it is the most predictable outage there is, and the one uptime checks only
//! catch after the fact.
//!
//! Parsing is done in pure Rust so the agent does not require a system OpenSSL
//! on the monitored host.
//!
//! A certificate file on disk is not necessarily what the host serves:
//! certbot leaves old lineages behind (`example.com` next to a renewed
//! `example.com-0001`), and the web server may have moved to another ACME
//! client altogether. Only the certificate in use can take a site down, so
//! the collector works out which one that is, in this order:
//!
//! 1. the certificate actually served on `127.0.0.1:443` for the domain (TLS
//!    handshake with SNI, short timeout);
//! 2. the paths referenced by `ssl_certificate` in the configuration of a
//!    running nginx;
//! 3. failing both, the lineage with the latest expiry for each domain.
//!
//! Every other lineage is reported as orphaned, which is informational and
//! never an expiry alert.

use anyhow::Result;
#[cfg(test)]
use chrono::TimeZone;
use chrono::{DateTime, Utc};
use shared::types::{TlsCertificateInfo, TlsSnapshot};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, warn};
use x509_parser::prelude::*;

#[cfg(all(test, feature = "unit-doubles"))]
use campaign_tls_units::read_certificate;
#[cfg(not(all(test, feature = "unit-doubles")))]
use std::fs::read as read_certificate;

/// Where Let's Encrypt keeps its lineages by default
const DEFAULT_CERTIFICATE_DIR: &str = "/etc/letsencrypt/live";

/// Certificate file to read inside a lineage directory
const CERTIFICATE_FILENAME: &str = "cert.pem";

/// Where nginx keeps its configuration on a stock install
const DEFAULT_NGINX_CONFIG_DIR: &str = "/etc/nginx";

/// Where the host's HTTPS listener is probed
const DEFAULT_PROBE_ADDRESS: &str = "127.0.0.1:443";

/// Budget for each local TLS handshake. Loopback answers in milliseconds;
/// anything slower is not worth holding the collection for.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Cap on handshakes per collection, so a host with hundreds of names does
/// not turn every scrape into a scan
const MAX_PROBED_DOMAINS: usize = 64;

/// How deep to walk the nginx configuration tree
const MAX_NGINX_DEPTH: usize = 4;

/// Configuration files larger than this are not nginx configuration
const MAX_NGINX_FILE_BYTES: u64 = 1024 * 1024;

/// Collector for TLS certificates
pub struct TlsCollector {
    /// Directories holding certificate lineages
    certificate_dirs: Vec<String>,
    /// nginx configuration roots; empty disables the nginx lookup
    nginx_config_dirs: Vec<String>,
    /// HTTPS listener to probe; `None` disables the handshake
    probe_address: Option<String>,
}

/// Everything read out of a single certificate
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCertificate {
    pub domains: Vec<String>,
    pub issuer: String,
    pub not_after: DateTime<Utc>,
    /// Serial number in hex, which with the expiry identifies a certificate
    pub serial: String,
}

impl ParsedCertificate {
    /// Whether two parsed certificates are the same certificate
    fn same_certificate(&self, other: &ParsedCertificate) -> bool {
        self.serial == other.serial && self.not_after == other.not_after
    }

    /// Whether this certificate is valid for `domain`, wildcards included
    fn covers(&self, domain: &str) -> bool {
        self.domains.iter().any(|name| name_matches(name, domain))
    }
}

/// A certificate lineage read from disk
#[derive(Debug, Clone)]
pub struct Lineage {
    /// Directory name, which is how operators refer to the lineage
    pub name: String,
    /// The `cert.pem` that was read
    pub source: PathBuf,
    pub certificate: ParsedCertificate,
}

/// What a running nginx says about certificates
#[derive(Debug, Clone, Default)]
pub struct NginxView {
    /// An nginx process exists on the host, containers included
    pub running: bool,
    /// Every `ssl_certificate` path in the active configuration
    pub certificate_paths: Vec<PathBuf>,
    /// Every concrete `server_name`, used to pick the names to probe
    pub server_names: Vec<String>,
}

/// What the local HTTPS listener served
#[derive(Debug, Clone, Default)]
pub struct ServedView {
    /// Something accepted a TCP connection on the probe address
    pub listening: bool,
    /// The address probed, reported as the source of served certificates
    pub address: String,
    /// Leaf certificate served per SNI name; names that failed the handshake
    /// or got a certificate not valid for them are absent
    pub by_domain: BTreeMap<String, ParsedCertificate>,
}

/// Whether a certificate name (possibly `*.example.com`) matches a host name
fn name_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    let host = host.to_ascii_lowercase();
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // A wildcard covers exactly one label
        return host
            .split_once('.')
            .is_some_and(|(label, rest)| !label.is_empty() && rest == suffix);
    }
    pattern == host
}

/// A concrete name worth sending as SNI
fn probeable(name: &str) -> bool {
    name.contains('.')
        && !name.starts_with('*')
        && !name.starts_with('~')
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// Parse a PEM-encoded certificate.
///
/// Only the first certificate is read: in a chain file the leaf comes first,
/// and the intermediates expire on their own schedule, which is not what a
/// site's availability depends on.
pub fn parse_certificate(pem_bytes: &[u8]) -> Result<ParsedCertificate> {
    parse_certificate_with_expiry(pem_bytes, DateTime::<Utc>::from_timestamp)
}

// Keep the defensive expiry conversion observable without manufacturing an
// invalid ASN.1 date; the production decoder is chrono's standard conversion.
fn parse_certificate_with_expiry(
    pem_bytes: &[u8],
    expiry: fn(i64, u32) -> Option<DateTime<Utc>>,
) -> Result<ParsedCertificate> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem_bytes)
        .map_err(|e| anyhow::anyhow!("not a PEM certificate: {}", e))?;
    let cert = pem
        .parse_x509()
        .map_err(|e| anyhow::anyhow!("malformed certificate: {}", e))?;
    read_certificate_fields(&cert, expiry)
}

/// Parse a DER-encoded certificate, as received in a TLS handshake.
pub fn parse_certificate_der(der: &[u8]) -> Result<ParsedCertificate> {
    let (_, cert) = x509_parser::parse_x509_certificate(der)
        .map_err(|e| anyhow::anyhow!("malformed certificate: {}", e))?;
    read_certificate_fields(&cert, DateTime::<Utc>::from_timestamp)
}

fn read_certificate_fields(
    cert: &X509Certificate<'_>,
    expiry: fn(i64, u32) -> Option<DateTime<Utc>>,
) -> Result<ParsedCertificate> {
    let not_after = expiry(cert.validity().not_after.timestamp(), 0)
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
        serial: cert.raw_serial_as_string(),
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
    /// Collector that only reads lineages from disk, without looking at nginx
    /// or the HTTPS listener. Lineages are still resolved against each other:
    /// a superseded lineage is orphaned.
    pub fn new(certificate_dirs: Vec<String>) -> Self {
        let dirs = if certificate_dirs.is_empty() {
            vec![DEFAULT_CERTIFICATE_DIR.to_string()]
        } else {
            certificate_dirs
        };

        Self {
            certificate_dirs: dirs,
            nginx_config_dirs: Vec::new(),
            probe_address: None,
        }
    }

    /// Also consult the host's nginx configuration and probe its HTTPS
    /// listener, to tell the certificate in use from leftovers.
    pub fn with_host_discovery(mut self) -> Self {
        self.nginx_config_dirs = vec![DEFAULT_NGINX_CONFIG_DIR.to_string()];
        self.probe_address = Some(DEFAULT_PROBE_ADDRESS.to_string());
        self
    }

    /// Collect the certificate inventory.
    ///
    /// A missing or unreadable directory is not an error: most hosts have no
    /// certificates at all, and the agent must not fail the RPC over it.
    pub async fn collect(&self) -> Result<TlsSnapshot> {
        let now = Utc::now();
        let lineages = self.read_lineages();

        let nginx = if self.nginx_config_dirs.is_empty() {
            NginxView::default()
        } else {
            let dirs = self.nginx_config_dirs.clone();
            tokio::task::spawn_blocking(move || read_nginx(&dirs))
                .await
                .unwrap_or_default()
        };

        let served = match &self.probe_address {
            Some(address) => probe_served(address, probe_names(&lineages, &nginx)).await,
            None => ServedView::default(),
        };

        let mut certificates = resolve(&lineages, &nginx, &served, now);

        // Soonest to expire first: the list is read top-down under pressure
        certificates.sort_by_key(|c| c.days_until_expiry);

        Ok(TlsSnapshot {
            certificates,
            renewal_unit_name: None,
            renewal_unit_status: None,
        })
    }

    fn read_lineages(&self) -> Vec<Lineage> {
        let mut lineages = Vec::new();
        for dir in &self.certificate_dirs {
            for cert_path in Self::certificate_paths(Path::new(dir)) {
                match read_certificate(&cert_path) {
                    Ok(bytes) => match parse_certificate(&bytes) {
                        Ok(certificate) => lineages.push(Lineage {
                            name: lineage_name(&cert_path),
                            source: cert_path,
                            certificate,
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
        lineages
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

/// Whether an `ssl_certificate` path points into a lineage directory.
///
/// nginx in a container sees the lineage under its own mount point, so the
/// comparison is by the last two path components (`live/<name>`), not by the
/// full path.
fn references_lineage(reference: &Path, lineage: &Lineage) -> bool {
    let (Some(ref_dir), Some(lineage_dir)) = (reference.parent(), lineage.source.parent()) else {
        return false;
    };
    if ref_dir == lineage_dir {
        return true;
    }
    let tail = |p: &Path| {
        (
            p.file_name().map(|n| n.to_os_string()),
            p.parent()
                .and_then(|q| q.file_name())
                .map(|n| n.to_os_string()),
        )
    };
    let (ref_name, ref_parent) = tail(ref_dir);
    let (lin_name, lin_parent) = tail(lineage_dir);
    ref_name.is_some() && ref_name == lin_name && ref_parent.is_some() && ref_parent == lin_parent
}

/// Decide which lineages are in use and report them, plus any served
/// certificate that no lineage on disk accounts for.
pub fn resolve(
    lineages: &[Lineage],
    nginx: &NginxView,
    served: &ServedView,
    now: DateTime<Utc>,
) -> Vec<TlsCertificateInfo> {
    let nginx_known = nginx.running && !nginx.certificate_paths.is_empty();
    let mut certificates = Vec::new();

    for lineage in lineages {
        let cert = &lineage.certificate;
        let served_here: Vec<&ParsedCertificate> = cert
            .domains
            .iter()
            .filter_map(|d| served.by_domain.get(&d.to_ascii_lowercase()))
            .collect();

        let in_use = if !served_here.is_empty() {
            // What the listener hands out is the ground truth
            served_here.iter().any(|s| s.same_certificate(cert))
        } else if nginx_known
            && nginx
                .certificate_paths
                .iter()
                .any(|p| references_lineage(p, lineage))
        {
            true
        } else if nginx_known || served.listening {
            // The host serves HTTPS, and nothing in it uses this lineage
            false
        } else {
            // No better evidence: the newest lineage for each name is in use
            !lineages.iter().any(|other| {
                other.certificate.not_after > cert.not_after
                    && other
                        .certificate
                        .domains
                        .iter()
                        .any(|d| cert.domains.iter().any(|mine| mine.eq_ignore_ascii_case(d)))
            })
        };

        certificates.push(TlsCertificateInfo {
            name: lineage.name.clone(),
            domains: cert.domains.clone(),
            issuer: cert.issuer.clone(),
            not_after: Some(cert.not_after),
            days_until_expiry: days_until(cert.not_after, now),
            source: lineage.source.to_string_lossy().to_string(),
            orphaned: !in_use,
        });
    }

    // Served certificates no lineage accounts for (another ACME client, a
    // certificate outside the configured directories): they are what the
    // site depends on, so they are reported in their own right.
    let mut reported: Vec<&ParsedCertificate> = Vec::new();
    for (domain, served_cert) in &served.by_domain {
        let on_disk = lineages
            .iter()
            .any(|l| l.certificate.same_certificate(served_cert));
        if on_disk || reported.iter().any(|r| r.same_certificate(served_cert)) {
            continue;
        }
        reported.push(served_cert);
        certificates.push(TlsCertificateInfo {
            name: domain.clone(),
            domains: served_cert.domains.clone(),
            issuer: served_cert.issuer.clone(),
            not_after: Some(served_cert.not_after),
            days_until_expiry: days_until(served_cert.not_after, now),
            source: served.address.clone(),
            orphaned: false,
        });
    }

    certificates
}

/// Names to send as SNI: every concrete name the lineages and nginx know.
fn probe_names(lineages: &[Lineage], nginx: &NginxView) -> Vec<String> {
    let mut names: BTreeSet<String> = lineages
        .iter()
        .flat_map(|l| l.certificate.domains.iter())
        .chain(nginx.server_names.iter())
        .filter(|n| probeable(n))
        .map(|n| n.to_ascii_lowercase())
        .collect();
    if names.len() > MAX_PROBED_DOMAINS {
        debug!(
            "Probing only {} of {} names",
            MAX_PROBED_DOMAINS,
            names.len()
        );
        names = names.into_iter().take(MAX_PROBED_DOMAINS).collect();
    }
    names.into_iter().collect()
}

// ─────────────────────────────────────────
// nginx configuration
// ─────────────────────────────────────────

/// Read the `ssl_certificate` and `server_name` directives of a running
/// nginx. Configuration of an nginx that is not running is ignored: it serves
/// nothing.
pub fn read_nginx(config_dirs: &[String]) -> NginxView {
    let running = process_running("nginx");
    if !running {
        return NginxView::default();
    }
    let mut view = parse_nginx_dirs(config_dirs);
    view.running = true;
    view
}

/// Parse every configuration file under the given roots.
pub fn parse_nginx_dirs(config_dirs: &[String]) -> NginxView {
    let mut files = Vec::new();
    for dir in config_dirs {
        nginx_files(Path::new(dir), 0, &mut files);
    }
    files.sort();

    let mut view = NginxView::default();
    for file in files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            parse_nginx_config(&text, &mut view);
        }
    }
    view.certificate_paths.sort();
    view.certificate_paths.dedup();
    view.server_names.sort();
    view.server_names.dedup();
    view
}

/// Configuration files nginx would load. `*-available` directories hold
/// disabled sites, and backups are not configuration.
fn nginx_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_NGINX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !name.ends_with("-available") {
                nginx_files(&path, depth + 1, out);
            }
            continue;
        }
        let in_conf_d = dir.file_name().is_some_and(|d| d == "conf.d");
        let backup = name.ends_with('~')
            || [".bak", ".old", ".orig", ".disabled", ".save"]
                .iter()
                .any(|s| name.ends_with(s))
            || name.contains(".dpkg-");
        let small = std::fs::metadata(&path)
            .map(|m| m.len() <= MAX_NGINX_FILE_BYTES)
            .unwrap_or(false);
        if backup || !small || (in_conf_d && !name.ends_with(".conf")) {
            continue;
        }
        out.push(path);
    }
}

/// Pull `ssl_certificate` paths and `server_name` values out of one file.
pub fn parse_nginx_config(text: &str, view: &mut NginxView) {
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        for statement in line.split(';') {
            let mut words = statement.split_whitespace();
            match words.next() {
                Some("ssl_certificate") => {
                    if let Some(path) = words.next() {
                        let path = path.trim_matches(|c| c == '"' || c == '\'');
                        // `$variable` paths are resolved at request time
                        if !path.contains('$') {
                            view.certificate_paths.push(PathBuf::from(path));
                        }
                    }
                }
                Some("server_name") => {
                    view.server_names.extend(
                        words
                            .map(|w| w.trim_matches(|c| c == '"' || c == '\''))
                            .filter(|w| probeable(w))
                            .map(str::to_string),
                    );
                }
                _ => {}
            }
        }
    }
}

/// Whether a process with this command name runs on the host. Containers
/// share the host's process table, so a containerised nginx counts.
fn process_running(command: &str) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.filter_map(|e| e.ok()).any(|entry| {
        std::fs::read_to_string(entry.path().join("comm"))
            .map(|comm| comm.trim() == command)
            .unwrap_or(false)
    })
}

// ─────────────────────────────────────────
// Local TLS handshake
// ─────────────────────────────────────────

/// Probe the listener once per name, concurrently.
async fn probe_served(address: &str, names: Vec<String>) -> ServedView {
    let mut view = ServedView {
        address: address.to_string(),
        ..Default::default()
    };
    let Ok(socket) = address.parse::<std::net::SocketAddr>() else {
        warn!("Invalid TLS probe address {}", address);
        return view;
    };

    let reachable = tokio::task::spawn_blocking(move || {
        std::net::TcpStream::connect_timeout(&socket, PROBE_TIMEOUT).is_ok()
    })
    .await
    .unwrap_or(false);
    if !reachable {
        return view;
    }
    view.listening = true;

    let handles: Vec<_> = names
        .into_iter()
        .map(|name| {
            tokio::task::spawn_blocking(move || {
                let served = served_certificate(socket, &name, PROBE_TIMEOUT);
                (name, served)
            })
        })
        .collect();

    for handle in handles {
        if let Ok((name, Some(der))) = handle.await {
            match parse_certificate_der(&der) {
                // A default certificate handed to an unknown name says
                // nothing about that name
                Ok(cert) if cert.covers(&name) => {
                    view.by_domain.insert(name, cert);
                }
                Ok(_) => debug!(
                    "{} answered {} with another name's certificate",
                    address, name
                ),
                Err(e) => debug!(
                    "{} served an unreadable certificate for {}: {}",
                    address, name, e
                ),
            }
        }
    }
    view
}

/// The leaf certificate a TLS server presents for `name`, as DER.
///
/// The chain is deliberately not verified: the point is to read what is
/// served, including an expired or self-signed certificate.
pub fn served_certificate(
    socket: std::net::SocketAddr,
    name: &str,
    timeout: Duration,
) -> Option<Vec<u8>> {
    use std::sync::Arc;

    let server_name = rustls::pki_types::ServerName::try_from(name.to_string()).ok()?;
    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(ReadAnyCertificate))
        .with_no_client_auth();
    let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name).ok()?;

    let mut tcp = std::net::TcpStream::connect_timeout(&socket, timeout).ok()?;
    tcp.set_read_timeout(Some(timeout)).ok()?;
    tcp.set_write_timeout(Some(timeout)).ok()?;

    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut tcp) {
            debug!("TLS probe of {} failed: {}", name, e);
            return None;
        }
    }
    conn.peer_certificates()
        .and_then(|chain| chain.first())
        .map(|leaf| leaf.as_ref().to_vec())
}

/// Accepts whatever certificate is presented, so it can be read
#[derive(Debug)]
struct ReadAnyCertificate;

impl rustls::client::danger::ServerCertVerifier for ReadAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
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

    #[test]
    fn test_host_discovery_is_opt_in() {
        let plain = TlsCollector::new(Vec::new());
        assert!(plain.nginx_config_dirs.is_empty());
        assert!(plain.probe_address.is_none());

        let host = TlsCollector::new(Vec::new()).with_host_discovery();
        assert_eq!(host.nginx_config_dirs, vec![DEFAULT_NGINX_CONFIG_DIR]);
        assert_eq!(host.probe_address.as_deref(), Some(DEFAULT_PROBE_ADDRESS));
    }

    // ─────────────────────────────────────────
    // Which certificate is in use
    // ─────────────────────────────────────────

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap()
    }

    fn certificate(domains: &[&str], serial: &str, days_left: i64) -> ParsedCertificate {
        ParsedCertificate {
            domains: domains.iter().map(|d| d.to_string()).collect(),
            issuer: "R11".into(),
            not_after: now() + chrono::Duration::days(days_left) + chrono::Duration::hours(1),
            serial: serial.into(),
        }
    }

    fn lineage(name: &str, certificate: ParsedCertificate) -> Lineage {
        Lineage {
            name: name.into(),
            source: PathBuf::from(format!("/etc/letsencrypt/live/{}/cert.pem", name)),
            certificate,
        }
    }

    fn by_name<'a>(certs: &'a [TlsCertificateInfo], name: &str) -> &'a TlsCertificateInfo {
        certs.iter().find(|c| c.name == name).expect(name)
    }

    fn running_nginx(paths: &[&str]) -> NginxView {
        NginxView {
            running: true,
            certificate_paths: paths.iter().map(PathBuf::from).collect(),
            server_names: Vec::new(),
        }
    }

    #[test]
    fn test_old_lineage_next_to_renewed_one_is_orphaned() {
        // certbot renewed into example.com-0001 and left example.com behind
        let lineages = vec![
            lineage("example.com", certificate(&["example.com"], "01", -35)),
            lineage("example.com-0001", certificate(&["example.com"], "02", 60)),
        ];
        let certs = resolve(
            &lineages,
            &NginxView::default(),
            &ServedView::default(),
            now(),
        );

        assert!(by_name(&certs, "example.com").orphaned);
        assert!(!by_name(&certs, "example.com-0001").orphaned);
        assert_eq!(by_name(&certs, "example.com-0001").days_until_expiry, 60);
    }

    #[test]
    fn test_lone_lineage_without_other_evidence_is_in_use() {
        let lineages = vec![lineage(
            "solo.example",
            certificate(&["solo.example"], "01", 5),
        )];
        let certs = resolve(
            &lineages,
            &NginxView::default(),
            &ServedView::default(),
            now(),
        );
        assert!(!certs[0].orphaned);
    }

    #[test]
    fn test_lineage_referenced_by_running_nginx_is_in_use_even_if_older() {
        let lineages = vec![
            lineage("site.example", certificate(&["site.example"], "01", 3)),
            lineage(
                "site.example-0001",
                certificate(&["site.example"], "02", 80),
            ),
        ];
        let nginx = running_nginx(&["/etc/letsencrypt/live/site.example/fullchain.pem"]);
        let certs = resolve(&lineages, &nginx, &ServedView::default(), now());

        let used = by_name(&certs, "site.example");
        assert!(!used.orphaned, "nginx serves it, its expiry is real");
        assert_eq!(used.days_until_expiry, 3);
        assert!(by_name(&certs, "site.example-0001").orphaned);
    }

    #[test]
    fn test_nginx_reference_through_a_container_mount_matches_the_lineage() {
        let lineages = vec![lineage(
            "app.example",
            certificate(&["app.example"], "01", 30),
        )];
        let nginx = running_nginx(&["/certs/live/app.example/fullchain.pem"]);
        let certs = resolve(&lineages, &nginx, &ServedView::default(), now());
        assert!(!certs[0].orphaned);
    }

    #[test]
    fn test_unreferenced_lineage_on_a_host_with_running_nginx_is_orphaned() {
        let lineages = vec![
            lineage("used.example", certificate(&["used.example"], "01", 40)),
            lineage("stale.example", certificate(&["stale.example"], "02", -20)),
        ];
        let nginx = running_nginx(&["/etc/letsencrypt/live/used.example/fullchain.pem"]);
        let certs = resolve(&lineages, &nginx, &ServedView::default(), now());

        assert!(!by_name(&certs, "used.example").orphaned);
        assert!(by_name(&certs, "stale.example").orphaned);
    }

    #[test]
    fn test_configuration_of_a_stopped_nginx_is_not_evidence() {
        let lineages = vec![
            lineage("a.example", certificate(&["a.example"], "01", 10)),
            lineage("b.example", certificate(&["b.example"], "02", 10)),
        ];
        let stopped = NginxView {
            running: false,
            certificate_paths: vec![PathBuf::from(
                "/etc/letsencrypt/live/a.example/fullchain.pem",
            )],
            server_names: Vec::new(),
        };
        let certs = resolve(&lineages, &stopped, &ServedView::default(), now());
        assert!(certs.iter().all(|c| !c.orphaned));
    }

    #[test]
    fn test_served_certificate_wins_over_stale_files() {
        // The production case: another server (Caddy) renewed the names and
        // serves them, while certbot's lineages expired on disk
        let stale = certificate(&["shop.example", "www.shop.example"], "01", -35);
        let fresh = certificate(&["shop.example", "www.shop.example"], "99", 33);
        let lineages = vec![lineage("shop.example", stale)];
        let served = ServedView {
            listening: true,
            address: "127.0.0.1:443".into(),
            by_domain: [
                ("shop.example".to_string(), fresh.clone()),
                ("www.shop.example".to_string(), fresh),
            ]
            .into_iter()
            .collect(),
        };
        // Even a running nginx pointing at the stale file loses to the listener
        let nginx = running_nginx(&["/etc/letsencrypt/live/shop.example/fullchain.pem"]);
        let certs = resolve(&lineages, &nginx, &served, now());

        assert_eq!(
            certs.len(),
            2,
            "one stale lineage plus one served certificate"
        );
        assert!(by_name(&certs, "shop.example").orphaned);
        let live = certs.iter().find(|c| c.source == "127.0.0.1:443").unwrap();
        assert!(!live.orphaned);
        assert_eq!(live.days_until_expiry, 33);
    }

    #[test]
    fn test_served_certificate_matching_the_lineage_is_reported_once() {
        let cert = certificate(&["same.example"], "07", 4);
        let lineages = vec![lineage("same.example", cert.clone())];
        let served = ServedView {
            listening: true,
            address: "127.0.0.1:443".into(),
            by_domain: [("same.example".to_string(), cert)].into_iter().collect(),
        };
        let certs = resolve(&lineages, &NginxView::default(), &served, now());

        assert_eq!(certs.len(), 1);
        assert!(!certs[0].orphaned, "it is what the listener serves");
        assert_eq!(certs[0].days_until_expiry, 4);
    }

    #[test]
    fn test_lineage_not_served_by_the_listener_is_orphaned() {
        // Something serves HTTPS, but refuses these names: nobody uses it
        let lineages = vec![lineage(
            "gone.example",
            certificate(&["gone.example"], "01", -3),
        )];
        let served = ServedView {
            listening: true,
            address: "127.0.0.1:443".into(),
            by_domain: BTreeMap::new(),
        };
        let certs = resolve(&lineages, &NginxView::default(), &served, now());
        assert!(certs[0].orphaned);
    }

    #[test]
    fn test_name_matching_handles_wildcards_and_case() {
        assert!(name_matches("Example.com", "example.COM"));
        assert!(name_matches("*.example.com", "api.example.com"));
        assert!(!name_matches("*.example.com", "example.com"));
        assert!(!name_matches("*.example.com", "a.b.example.com"));
        assert!(!name_matches("example.com", "other.com"));
    }

    #[test]
    fn test_probe_names_skip_wildcards_and_placeholders() {
        let lineages = vec![lineage(
            "w",
            certificate(&["*.example.com", "Example.com"], "01", 10),
        )];
        let nginx = NginxView {
            running: true,
            certificate_paths: Vec::new(),
            server_names: vec!["api.example.com".into(), "example.com".into()],
        };
        assert_eq!(
            probe_names(&lineages, &nginx),
            vec!["api.example.com".to_string(), "example.com".to_string()]
        );
    }

    #[test]
    fn test_parse_nginx_config_reads_certificates_and_names() {
        let mut view = NginxView::default();
        parse_nginx_config(
            r#"
            server {
                listen 443 ssl;
                server_name site.example www.site.example _ ~^re\.example$;
                ssl_certificate /etc/letsencrypt/live/site.example/fullchain.pem; # managed by Certbot
                ssl_certificate_key /etc/letsencrypt/live/site.example/privkey.pem;
                # ssl_certificate /etc/letsencrypt/live/commented/fullchain.pem;
                ssl_certificate "/etc/ssl/quoted.pem"; ssl_certificate /etc/ssl/$ssl_server_name.pem;
            }
            "#,
            &mut view,
        );
        assert_eq!(
            view.certificate_paths,
            vec![
                PathBuf::from("/etc/letsencrypt/live/site.example/fullchain.pem"),
                PathBuf::from("/etc/ssl/quoted.pem"),
            ]
        );
        assert_eq!(view.server_names, vec!["site.example", "www.site.example"]);
    }

    #[test]
    fn test_parse_nginx_dirs_skips_disabled_sites_and_backups() {
        let root = tempfile::tempdir().unwrap();
        let enabled = root.path().join("sites-enabled");
        let available = root.path().join("sites-available");
        let conf_d = root.path().join("conf.d");
        for dir in [&enabled, &available, &conf_d] {
            std::fs::create_dir(dir).unwrap();
        }
        let directive = |name: &str| {
            format!(
                "ssl_certificate /etc/letsencrypt/live/{}/fullchain.pem;",
                name
            )
        };
        std::fs::write(enabled.join("live"), directive("enabled")).unwrap();
        std::fs::write(enabled.join("live.bak"), directive("backup")).unwrap();
        std::fs::write(available.join("off"), directive("disabled")).unwrap();
        std::fs::write(conf_d.join("extra.conf"), directive("confd")).unwrap();
        std::fs::write(conf_d.join("notes.txt"), directive("notes")).unwrap();

        let view = parse_nginx_dirs(&[root.path().to_string_lossy().to_string()]);
        let names: Vec<String> = view
            .certificate_paths
            .iter()
            .map(|p| {
                p.parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            })
            .collect();
        assert_eq!(names, vec!["confd", "enabled"]);
    }

    #[test]
    fn test_parse_certificate_der_matches_pem() {
        let (_, pem) = x509_parser::pem::parse_x509_pem(SAMPLE_PEM).unwrap();
        let from_der = parse_certificate_der(&pem.contents).unwrap();
        assert_eq!(from_der, parse_certificate(SAMPLE_PEM).unwrap());
        assert!(!from_der.serial.is_empty());
        assert!(parse_certificate_der(b"junk").is_err());
    }

    #[tokio::test]
    async fn test_probe_of_a_closed_port_reports_no_listener() {
        // Bind then drop, so the port is known to be closed
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let view = probe_served(&format!("127.0.0.1:{}", port), vec!["a.example".into()]).await;
        assert!(!view.listening);
        assert!(view.by_domain.is_empty());
    }

    #[tokio::test]
    async fn test_probe_of_a_listener_that_is_not_tls_serves_nothing() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            // Accept the reachability check and the handshake, then hang up
            for stream in listener.incoming().take(2) {
                drop(stream);
            }
        });
        let view = probe_served(&address.to_string(), vec!["a.example".into()]).await;
        assert!(view.listening);
        assert!(view.by_domain.is_empty());
    }

    #[tokio::test]
    async fn test_probe_with_an_invalid_address_is_harmless() {
        let view = probe_served("not an address", vec!["a.example".into()]).await;
        assert!(!view.listening);
    }
}

#[cfg(test)]
#[cfg(feature = "unit-doubles")]
#[path = "../../tests/doubles/certificate_units.rs"]
pub(crate) mod campaign_tls_units;
