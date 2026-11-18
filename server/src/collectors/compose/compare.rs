//! Declared service against running container: pure, no I/O

use super::declared::DeclaredService;
use super::model::{normalize_restart, Mount, MountKind, Port};
use super::runtime::ContainerRuntime;
use shared::compose::{ComposeDifference, ComposeField, ComposeStatus, DowngradeRisk};
use std::collections::{BTreeMap, BTreeSet};

const ABSENT: &str = "absent";

fn difference(field: ComposeField, item: &str, declared: &str, running: &str) -> ComposeDifference {
    ComposeDifference {
        field,
        item: item.to_string(),
        declared: declared.to_string(),
        running: running.to_string(),
    }
}

/// An image reference reduced to what identifies it: `nginx`,
/// `docker.io/library/nginx` and `nginx:latest` are the same image.
#[derive(Debug, PartialEq, Eq)]
struct ImageRef {
    name: String,
    tag: Option<String>,
    digest: Option<String>,
}

impl ImageRef {
    fn parse(reference: &str) -> Self {
        let (rest, digest) = match reference.trim().split_once('@') {
            Some((rest, digest)) => (rest, Some(digest.to_string())),
            None => (reference.trim(), None),
        };
        // A colon after the last slash is a tag; before it, a registry port
        let slash = rest.rfind('/').map_or(0, |i| i + 1);
        let (name, tag) = match rest[slash..].rfind(':') {
            Some(i) => (&rest[..slash + i], Some(rest[slash + i + 1..].to_string())),
            None => (rest, None),
        };
        let tag = tag.or_else(|| digest.is_none().then(|| "latest".to_string()));
        Self {
            name: normalize_image_name(name),
            tag,
            digest,
        }
    }
}

fn normalize_image_name(name: &str) -> String {
    let name = ["docker.io/", "index.docker.io/", "registry-1.docker.io/"]
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix))
        .unwrap_or(name);
    name.strip_prefix("library/").unwrap_or(name).to_string()
}

/// `v1.25.3-alpine` → `([1, 25, 3], "-alpine")`; `None` without a version.
fn version_key(tag: &str) -> Option<(Vec<u64>, &str)> {
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let end = tag
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(tag.len());
    let numbers: Vec<u64> = tag[..end]
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    Some((numbers, &tag[end..]))
}

/// Whether `declared` is an older version than `running`.
///
/// Only versions of the same shape are ordered (`1.4.0` against `1.5.2`,
/// `1.4-alpine` against `1.5-alpine`). A prefix (`1.25` against `1.25.3`)
/// is a floating tag that may well resolve to the running version, and
/// `latest`, `stable` or a commit hash say nothing about order.
fn is_older(declared: &str, running: &str) -> bool {
    let (Some((d, d_suffix)), Some((r, r_suffix))) = (version_key(declared), version_key(running))
    else {
        return false;
    };
    if d_suffix != r_suffix {
        return false;
    }
    d.iter()
        .zip(&r)
        .find(|(a, b)| a != b)
        .is_some_and(|(a, b)| a < b)
}

/// Compare images; a downgrade when the same repository is declared at an
/// older tag than the running one.
fn compare_image(
    declared: &str,
    running: &str,
    out: &mut Vec<ComposeDifference>,
) -> Option<DowngradeRisk> {
    // Created from an image ID: there is no reference to compare
    if running.is_empty() || running.starts_with("sha256:") {
        return None;
    }
    let (d, r) = (ImageRef::parse(declared), ImageRef::parse(running));
    if d == r {
        return None;
    }
    out.push(difference(ComposeField::Image, "", declared, running));
    let older =
        d.name == r.name && matches!((&d.tag, &r.tag), (Some(dt), Some(rt)) if is_older(dt, rt));
    older.then(|| DowngradeRisk {
        declared: declared.to_string(),
        running: running.to_string(),
    })
}

fn bindings_by_port<'a>(
    ports: impl Iterator<Item = &'a Port>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut by_port: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for port in ports {
        by_port
            .entry(port.container.clone())
            .or_default()
            .insert(port.binding());
    }
    by_port
}

fn show(set: Option<&BTreeSet<String>>) -> String {
    set.map_or_else(
        || ABSENT.to_string(),
        |s| s.iter().cloned().collect::<Vec<_>>().join(", "),
    )
}

/// Published ports, by container port, including the bind address.
fn compare_ports(
    declared: &DeclaredService,
    running: &ContainerRuntime,
    out: &mut Vec<ComposeDifference>,
) {
    let d = bindings_by_port(declared.ports.iter());
    let r = bindings_by_port(running.ports.iter());
    let keys: BTreeSet<&String> = d.keys().chain(r.keys()).collect();
    for key in keys {
        let (a, b) = (d.get(key), r.get(key));
        if a != b {
            out.push(difference(ComposeField::Ports, key, &show(a), &show(b)));
        }
    }
}

/// Mounts, by target: kind, source and `:ro`. Anonymous volumes that only
/// the container has come from the image's `VOLUME` and are not drift.
fn compare_mounts(
    declared: &DeclaredService,
    running: &ContainerRuntime,
    out: &mut Vec<ComposeDifference>,
) {
    let d: BTreeMap<&str, &Mount> = declared
        .mounts
        .iter()
        .map(|m| (m.target.as_str(), m))
        .collect();
    let r: BTreeMap<&str, &Mount> = running
        .mounts
        .iter()
        .map(|m| (m.target.as_str(), m))
        .collect();
    let targets: BTreeSet<&str> = d.keys().chain(r.keys()).copied().collect();
    for target in targets {
        let describe = |m: Option<&&Mount>| m.map_or_else(|| ABSENT.to_string(), |m| m.describe());
        let differs = match (d.get(target), r.get(target)) {
            (Some(a), Some(b)) => !a.satisfied_by(b),
            (Some(_), None) => true,
            (None, Some(b)) => b.kind != MountKind::Anonymous,
            (None, None) => false,
        };
        if differs {
            out.push(difference(
                ComposeField::Volumes,
                target,
                &describe(d.get(target)),
                &describe(r.get(target)),
            ));
        }
    }
}

fn compare_restart(
    declared: &DeclaredService,
    running: &ContainerRuntime,
    out: &mut Vec<ComposeDifference>,
) {
    let (d, r) = (
        normalize_restart(&declared.restart),
        normalize_restart(&running.restart),
    );
    if d != r {
        out.push(difference(ComposeField::Restart, "", &d, &r));
    }
}

/// `host`, `none` and `container:<id>` take the container off compose's
/// networks entirely.
fn is_special_mode(mode: &str) -> bool {
    matches!(mode, "host" | "none") || mode.starts_with("container:")
}

fn mode_matches(declared: &str, running: &str) -> bool {
    // `service:db` becomes `container:<id of db>`; the id is not knowable
    let shares = |m: &str| m.starts_with("service:") || m.starts_with("container:");
    declared == running || (shares(declared) && running.starts_with("container:"))
}

fn join(set: &BTreeSet<String>) -> String {
    if set.is_empty() {
        return "none".to_string();
    }
    set.iter().cloned().collect::<Vec<_>>().join(", ")
}

fn compare_networks(
    declared: &DeclaredService,
    running: &ContainerRuntime,
    out: &mut Vec<ComposeDifference>,
) {
    if let Some(mode) = &declared.network_mode {
        if !mode_matches(mode, &running.network_mode) {
            out.push(difference(
                ComposeField::NetworkMode,
                "",
                mode,
                &running.network_mode,
            ));
        }
        return;
    }
    if is_special_mode(&running.network_mode) {
        out.push(difference(
            ComposeField::NetworkMode,
            "",
            "(compose networks)",
            &running.network_mode,
        ));
        return;
    }
    if declared.networks != running.networks {
        out.push(difference(
            ComposeField::Networks,
            "",
            &join(&declared.networks),
            &join(&running.networks),
        ));
    }
}

/// Environment by key presence only; values are never looked at.
///
/// A key the file sets but the container lacks is drift. A key only the
/// container has is drift unless the image defines it, or the service also
/// reads an `env_file` (whose keys are not read), or the image's keys are not
/// known. Pass-through keys (`- KEY`) depend on the deployer's shell and are
/// never reported missing.
fn compare_env(
    declared: &DeclaredService,
    running: &ContainerRuntime,
    out: &mut Vec<ComposeDifference>,
) {
    for (key, has_value) in &declared.env {
        if *has_value && !running.env_keys.contains(key) {
            out.push(difference(ComposeField::EnvKey, key, "set", ABSENT));
        }
    }
    let Some(image_keys) = running
        .image_env_keys
        .as_ref()
        .filter(|_| !declared.env_file)
    else {
        return;
    };
    for key in &running.env_keys {
        if !declared.env.contains_key(key) && !image_keys.contains(key) {
            out.push(difference(ComposeField::EnvKey, key, ABSENT, "set"));
        }
    }
}

type FieldCheck = fn(&DeclaredService, &ContainerRuntime, &mut Vec<ComposeDifference>);

/// The field-by-field checks, each skipped when its field is unknown.
const FIELD_CHECKS: &[(&[ComposeField], FieldCheck)] = &[
    (&[ComposeField::Ports], compare_ports),
    (&[ComposeField::Volumes], compare_mounts),
    (&[ComposeField::Restart], compare_restart),
    (
        &[ComposeField::Networks, ComposeField::NetworkMode],
        compare_networks,
    ),
    (&[ComposeField::EnvKey], compare_env),
];

/// Verdict for one container, before the two-pass confirmation.
///
/// `declared` is `None` when the project no longer has the service.
pub fn compare(declared: Option<&DeclaredService>, running: &ContainerRuntime) -> ComposeStatus {
    let Some(declared) = declared else {
        return ComposeStatus::Drifted {
            differences: vec![difference(ComposeField::Service, "", ABSENT, "present")],
            downgrade: None,
        };
    };
    let mut differences = Vec::new();
    let downgrade = declared
        .image
        .as_deref()
        .filter(|_| !declared.unknown.contains(&ComposeField::Image))
        .and_then(|image| compare_image(image, &running.image, &mut differences));
    for (fields, check) in FIELD_CHECKS {
        if fields.iter().all(|f| !declared.unknown.contains(f)) {
            check(declared, running, &mut differences);
        }
    }
    differences.sort();
    if differences.is_empty() {
        ComposeStatus::InSync
    } else {
        ComposeStatus::Drifted {
            differences,
            downgrade,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(target: &str, source: &str, read_only: bool) -> Mount {
        Mount {
            target: target.into(),
            kind: MountKind::Bind,
            source: Some(source.into()),
            read_only,
        }
    }

    fn declared() -> DeclaredService {
        DeclaredService {
            image: Some("shop/web:1.5.2".into()),
            ports: BTreeSet::from([Port::new("127.0.0.1", "8080", 80, "tcp")]),
            mounts: vec![bind("/etc/app", "/srv/shop/conf", true)],
            restart: "unless-stopped".into(),
            network_mode: None,
            networks: BTreeSet::from(["shop_default".into()]),
            env: BTreeMap::from([("API_URL".into(), true), ("FROM_SHELL".into(), false)]),
            env_file: false,
            unknown: BTreeSet::new(),
        }
    }

    fn running() -> ContainerRuntime {
        ContainerRuntime {
            name: "shop-web-1".into(),
            compose: None,
            image: "docker.io/shop/web:1.5.2".into(),
            image_id: "sha256:feed".into(),
            ports: vec![Port::new("127.0.0.1", "8080", 80, "tcp")],
            mounts: vec![
                bind("/etc/app", "/srv/shop/conf", true),
                Mount {
                    target: "/cache".into(),
                    kind: MountKind::Anonymous,
                    source: None,
                    read_only: false,
                },
            ],
            restart: "unless-stopped".into(),
            network_mode: "shop_default".into(),
            networks: BTreeSet::from(["shop_default".into()]),
            env_keys: BTreeSet::from(["API_URL".into(), "PATH".into()]),
            image_env_keys: Some(BTreeSet::from(["PATH".into()])),
        }
    }

    fn differences(status: ComposeStatus) -> Vec<String> {
        match status {
            ComposeStatus::Drifted { differences, .. } => differences
                .iter()
                .map(ComposeDifference::describe)
                .collect(),
            ComposeStatus::InSync => vec![],
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn test_matching_container_is_in_sync() {
        // Registry prefix, anonymous image volume and image ENV are not drift
        assert_eq!(
            compare(Some(&declared()), &running()),
            ComposeStatus::InSync
        );
    }

    #[test]
    fn test_same_tag_in_any_spelling_matches() {
        let mut d = declared();
        d.image = Some("nginx".into());
        let mut r = running();
        r.image = "docker.io/library/nginx:latest".into();
        assert_eq!(compare(Some(&d), &r), ComposeStatus::InSync);
    }

    #[test]
    fn test_different_tag_is_drift_without_downgrade() {
        let mut d = declared();
        d.image = Some("shop/web:1.6.0".into());
        let status = compare(Some(&d), &running());
        let ComposeStatus::Drifted { downgrade, .. } = &status else {
            panic!("{status:?}");
        };
        assert!(downgrade.is_none(), "a newer declared tag is an upgrade");
        assert_eq!(
            differences(status),
            vec!["image: declared shop/web:1.6.0, running docker.io/shop/web:1.5.2"]
        );
    }

    #[test]
    fn test_older_declared_tag_is_a_downgrade_risk() {
        let mut d = declared();
        d.image = Some("shop/web:1.4.9".into());
        let ComposeStatus::Drifted { downgrade, .. } = compare(Some(&d), &running()) else {
            panic!("expected drift");
        };
        assert_eq!(
            downgrade,
            Some(DowngradeRisk {
                declared: "shop/web:1.4.9".into(),
                running: "docker.io/shop/web:1.5.2".into(),
            })
        );
    }

    #[test]
    fn test_version_ordering() {
        assert!(is_older("1.4.0", "1.5.2"));
        assert!(is_older("v1.9", "v1.10"));
        assert!(is_older("15.3-alpine", "16.1-alpine"));
        assert!(!is_older("1.5.2", "1.4.0"));
        assert!(!is_older("1.25", "1.25.3"), "floating tag");
        assert!(!is_older("1.4-alpine", "1.5"), "different variant");
        assert!(!is_older("latest", "1.5"));
        assert!(!is_older("abc123", "def456"));
    }

    #[test]
    fn test_image_reference_parsing() {
        let r = ImageRef::parse("registry.example.com:5000/team/app:2.1");
        assert_eq!(r.name, "registry.example.com:5000/team/app");
        assert_eq!(r.tag.as_deref(), Some("2.1"));
        let r = ImageRef::parse("registry.example.com:5000/team/app");
        assert_eq!(r.tag.as_deref(), Some("latest"));
        let r = ImageRef::parse("app@sha256:abc");
        assert_eq!((r.tag, r.digest.as_deref()), (None, Some("sha256:abc")));
    }

    #[test]
    fn test_container_created_from_image_id_is_not_compared() {
        let mut r = running();
        r.image = "sha256:0123".into();
        assert_eq!(compare(Some(&declared()), &r), ComposeStatus::InSync);
    }

    #[test]
    fn test_bind_address_difference_is_drift() {
        let mut r = running();
        r.ports = vec![Port::new("", "8080", 80, "tcp")];
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["ports 80/tcp: declared 127.0.0.1:8080, running 0.0.0.0:8080"]
        );
    }

    #[test]
    fn test_port_only_on_one_side_is_drift() {
        let mut r = running();
        r.ports.push(Port::new("0.0.0.0", "9090", 9090, "tcp"));
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["ports 9090/tcp: declared absent, running 0.0.0.0:9090"]
        );
        let mut r = running();
        r.ports.clear();
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["ports 80/tcp: declared 127.0.0.1:8080, running absent"]
        );
    }

    #[test]
    fn test_read_only_flag_is_compared() {
        let mut r = running();
        r.mounts[0].read_only = false;
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["volumes /etc/app: declared /srv/shop/conf:ro, running /srv/shop/conf"]
        );
    }

    #[test]
    fn test_bind_only_on_the_container_is_drift() {
        let mut r = running();
        r.mounts.push(bind("/backup", "/mnt/backup", false));
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["volumes /backup: declared absent, running /mnt/backup"]
        );
    }

    #[test]
    fn test_restart_and_networks() {
        let mut r = running();
        r.restart = "no".into();
        r.networks.insert("edge-proxy".into());
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec![
                "restart: declared unless-stopped, running no",
                "networks: declared shop_default, running edge-proxy, shop_default",
            ]
        );
    }

    #[test]
    fn test_network_mode() {
        let mut r = running();
        r.network_mode = "host".into();
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["network_mode: declared (compose networks), running host"]
        );
        let mut d = declared();
        d.network_mode = Some("service:vpn".into());
        r.network_mode = "container:0123abcd".into();
        assert_eq!(compare(Some(&d), &r), ComposeStatus::InSync);
    }

    #[test]
    fn test_env_key_present_and_absent() {
        // Declared with a value, missing in the container
        let mut r = running();
        r.env_keys.remove("API_URL");
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["env API_URL: declared set, running absent"]
        );
        // Only in the container, and not from the image
        let mut r = running();
        r.env_keys.insert("LEGACY_FLAG".into());
        assert_eq!(
            differences(compare(Some(&declared()), &r)),
            vec!["env LEGACY_FLAG: declared absent, running set"]
        );
        // With an env_file, or without the image's keys, extras are unknown
        let mut d = declared();
        d.env_file = true;
        assert_eq!(compare(Some(&d), &r), ComposeStatus::InSync);
        let mut r = r.clone();
        r.image_env_keys = None;
        assert_eq!(compare(Some(&declared()), &r), ComposeStatus::InSync);
    }

    #[test]
    fn test_unknown_fields_are_skipped() {
        let mut d = declared();
        d.image = None;
        d.unknown = BTreeSet::from([ComposeField::Image, ComposeField::Ports]);
        let mut r = running();
        r.image = "shop/web:0.1".into();
        r.ports.clear();
        assert_eq!(compare(Some(&d), &r), ComposeStatus::InSync);
    }

    #[test]
    fn test_service_removed_from_compose() {
        assert_eq!(
            differences(compare(None, &running())),
            vec!["service: declared absent, running present"]
        );
    }
}
