//! Certificate parser units on public ASN.1 fixtures and controlled file reads.
use super::*;
use std::{cell::RefCell, path::PathBuf};
thread_local! {static REFUSE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };}
pub fn read_certificate(path: &Path) -> std::io::Result<Vec<u8>> {
    if REFUSE.with(|refuse| refuse.borrow().as_deref() == Some(path)) {
        return Err(std::io::Error::other("unit certificate read refused"));
    }
    assert!(
        path.starts_with(std::env::var_os("TMPDIR").unwrap()),
        "unit reads only owned fixtures"
    );
    std::fs::read(path)
}
#[test]
fn parses_legacy_common_name_and_ignores_ip_subject_alternative_name() {
    let legacy = parse_certificate(include_bytes!("../fixtures/legacy-cn.pem")).unwrap();
    assert_eq!(legacy.domains, ["legacy-unit.invalid"]);
    assert_eq!(legacy.issuer, "legacy-unit.invalid");
    let ip = parse_certificate(include_bytes!("../fixtures/ip-san.pem")).unwrap();
    assert!(ip.domains.is_empty());
    assert_eq!(ip.issuer, "unknown");
}
#[test]
fn malformed_der_and_expiry_decoder_failure_keep_error_context() {
    let err = parse_certificate(b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n")
        .unwrap_err();
    assert!(err.to_string().starts_with("malformed certificate:"));
    fn unrepresentable(_: i64, _: u32) -> Option<DateTime<Utc>> {
        None
    }
    assert_eq!(
        parse_certificate_with_expiry(include_bytes!("../fixtures/legacy-cn.pem"), unrepresentable)
            .unwrap_err()
            .to_string(),
        "certificate carries an unrepresentable expiry"
    );
}
pub(crate) async fn exercise_log_fallback() {
    run_inventory_read_failure().await;
    assert!(TlsCollector::new(vec!["/unit-missing-certificates".into()])
        .collect()
        .await
        .unwrap()
        .certificates
        .is_empty());
}
#[tokio::test]
async fn file_disappearing_after_inventory_does_not_hide_other_lineages() {
    let (_guard, logs) = crate::unit_logs::capture();
    run_inventory_read_failure().await;
    assert!(logs
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("unit certificate read refused")));
}
async fn run_inventory_read_failure() {
    let root = tempfile::tempdir().unwrap();
    let unreadable = root.path().join("unreadable/cert.pem");
    std::fs::create_dir(unreadable.parent().unwrap()).unwrap();
    std::fs::write(&unreadable, b"will become unreadable").unwrap();
    std::fs::write(root.path().join("not-a-lineage"), b"ignore regular files").unwrap();
    let good = root.path().join("good/cert.pem");
    std::fs::create_dir(good.parent().unwrap()).unwrap();
    std::fs::write(&good, include_bytes!("../fixtures/legacy-cn.pem")).unwrap();
    REFUSE.with(|refuse| *refuse.borrow_mut() = Some(unreadable));
    let bad = root.path().join("bad/cert.pem");
    std::fs::create_dir(bad.parent().unwrap()).unwrap();
    std::fs::write(bad, b"invalid certificate").unwrap();
    let result = TlsCollector::new(vec![root.path().to_str().unwrap().into()])
        .collect()
        .await;
    REFUSE.with(|refuse| refuse.borrow_mut().take());
    let snapshot = result.unwrap();
    assert_eq!(snapshot.certificates.len(), 1);
    assert_eq!(snapshot.certificates[0].name, "good");
}
