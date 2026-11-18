//! End to end: compose fixtures in a temporary directory against inspect
//! doubles, through the two-pass confirmation.

use super::runtime::tests::inspect;
use super::runtime::{
    runtime_from_inspect, LABEL_CONFIG_FILES, LABEL_ENVIRONMENT_FILE, LABEL_PROJECT, LABEL_SERVICE,
    LABEL_WORKING_DIR,
};
use super::*;
use shared::compose::{ComposeDifference, ComposeField, DowngradeRisk};
use std::collections::BTreeSet;
use tempfile::TempDir;

/// Declares exactly what the inspect double runs, with the bind address and
/// the tag coming from `.env`.
const BASE: &str = r#"
services:
  web:
    image: registry.example.com/shop/web:${WEB_TAG:-1.5.2}
    restart: unless-stopped
    ports:
      - "${WEB_BIND}:8080:80"
    volumes:
      - /srv/shop/conf:/etc/app:ro
      - data:/var/lib/app
    environment:
      API_URL: https://api.example.com
      FROM_SHELL:
volumes:
  data:
"#;

struct Project {
    dir: TempDir,
}

impl Project {
    fn new() -> Self {
        let project = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        project.write("compose.yml", BASE);
        project.write(".env", "WEB_BIND=127.0.0.1\nWEB_TAG=\n");
        project
    }

    fn path(&self, name: &str) -> String {
        self.dir.path().join(name).display().to_string()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.dir.path().join(name), text).unwrap();
    }

    fn labels(&self, files: &[&str]) -> Vec<(&'static str, String)> {
        let files: Vec<String> = files.iter().map(|f| self.path(f)).collect();
        vec![
            (LABEL_PROJECT, "shop".into()),
            (LABEL_SERVICE, "web".into()),
            (LABEL_CONFIG_FILES, files.join(",")),
            (LABEL_WORKING_DIR, self.dir.path().display().to_string()),
        ]
    }

    /// The running container, as the inspect double describes it.
    fn container(&self, files: &[&str], env: &[&str]) -> ContainerRuntime {
        runtime_with(&self.labels(files), env)
    }
}

fn runtime_with(labels: &[(&str, String)], env: &[&str]) -> ContainerRuntime {
    let labels: Vec<(&str, &str)> = labels.iter().map(|(k, v)| (*k, v.as_str())).collect();
    runtime_from_inspect(&inspect(&labels, env)).unwrap()
}

const ENV: &[&str] = &["API_URL=https://api.example.com", "PATH=/usr/bin"];

/// Two passes over the same container; the second is the confirmed one.
fn two_passes(runtime: &ContainerRuntime) -> (ComposeStatus, ComposeStatus) {
    let mut audit = ComposeAudit::new(ComposeDriftConfig::default());
    let first = audit.assess(std::slice::from_ref(runtime)).unwrap();
    let second = audit.assess(std::slice::from_ref(runtime)).unwrap();
    (first[0].status.clone(), second[0].status.clone())
}

fn confirmed(runtime: &ContainerRuntime) -> ComposeStatus {
    two_passes(runtime).1
}

fn described(status: &ComposeStatus) -> Vec<String> {
    match status {
        ComposeStatus::Drifted { differences, .. } => differences
            .iter()
            .map(ComposeDifference::describe)
            .collect(),
        other => panic!("expected drift, got {other:?}"),
    }
}

#[test]
fn test_matching_files_are_in_sync() {
    let project = Project::new();
    let (first, second) = two_passes(&project.container(&["compose.yml"], ENV));
    assert_eq!(first, ComposeStatus::InSync);
    assert_eq!(second, ComposeStatus::InSync);
}

#[test]
fn test_different_tag_is_drift() {
    let project = Project::new();
    project.write(".env", "WEB_BIND=127.0.0.1\nWEB_TAG=1.6.0\n");
    let status = confirmed(&project.container(&["compose.yml"], ENV));
    assert_eq!(
        described(&status),
        vec![
            "image: declared registry.example.com/shop/web:1.6.0, running registry.example.com/shop/web:1.5.2"
        ]
    );
    assert!(matches!(
        status,
        ComposeStatus::Drifted {
            downgrade: None,
            ..
        }
    ));
}

#[test]
fn test_older_tag_in_compose_is_a_downgrade_risk() {
    let project = Project::new();
    project.write(".env", "WEB_BIND=127.0.0.1\nWEB_TAG=1.4.0\n");
    let ComposeStatus::Drifted { downgrade, .. } =
        confirmed(&project.container(&["compose.yml"], ENV))
    else {
        panic!("expected drift");
    };
    assert_eq!(
        downgrade,
        Some(DowngradeRisk {
            declared: "registry.example.com/shop/web:1.4.0".into(),
            running: "registry.example.com/shop/web:1.5.2".into(),
        })
    );
}

#[test]
fn test_bind_on_every_interface_is_drift() {
    // The container listens on 127.0.0.1; the file would publish everywhere
    let project = Project::new();
    project.write(".env", "WEB_BIND=0.0.0.0\n");
    assert_eq!(
        described(&confirmed(&project.container(&["compose.yml"], ENV))),
        vec!["ports 80/tcp: declared 0.0.0.0:8080, running 127.0.0.1:8080"]
    );
}

#[test]
fn test_override_dropping_read_only_is_drift() {
    let project = Project::new();
    project.write(
        "compose.override.yml",
        "services:\n  web:\n    volumes:\n      - /srv/shop/conf:/etc/app\n",
    );
    let runtime = project.container(&["compose.yml", "compose.override.yml"], ENV);
    assert_eq!(
        described(&confirmed(&runtime)),
        vec!["volumes /etc/app: declared /srv/shop/conf, running /srv/shop/conf:ro"]
    );
}

#[test]
fn test_override_is_merged_in_label_order() {
    let project = Project::new();
    project.write(
        "old-tag.yml",
        "services:\n  web:\n    image: registry.example.com/shop/web:1.4.0\n",
    );
    project.write(
        "running-tag.yml",
        "services:\n  web:\n    image: registry.example.com/shop/web:1.5.2\n",
    );
    // The last file wins: the running tag, in sync
    let in_order = project.container(&["compose.yml", "old-tag.yml", "running-tag.yml"], ENV);
    assert_eq!(confirmed(&in_order), ComposeStatus::InSync);
    // Swapped, the older tag wins: a downgrade
    let swapped = project.container(&["compose.yml", "running-tag.yml", "old-tag.yml"], ENV);
    assert!(matches!(
        confirmed(&swapped),
        ComposeStatus::Drifted {
            downgrade: Some(_),
            ..
        }
    ));
}

#[test]
fn test_unresolved_variable_is_not_a_false_drift() {
    // Without .env, WEB_BIND is unknown: ports are not compared at all
    let project = Project::new();
    std::fs::remove_file(project.dir.path().join(".env")).unwrap();
    let mut runtime = project.container(&["compose.yml"], ENV);
    runtime.ports.clear();
    assert_eq!(confirmed(&runtime), ComposeStatus::InSync);
}

#[test]
fn test_dot_env_without_permission_is_treated_as_absent() {
    use std::os::unix::fs::PermissionsExt;
    // root reads anything, so the permission cannot be denied there
    // SAFETY: geteuid has no preconditions and cannot fail
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    // The agent is not meant to read secrets: an .env it may not open
    // leaves the interpolated fields unresolved instead of hiding the
    // whole project as unreadable.
    let project = Project::new();
    let env = project.dir.path().join(".env");
    std::fs::set_permissions(&env, std::fs::Permissions::from_mode(0o000)).unwrap();
    let runtime = project.container(&["compose.yml"], ENV);
    let (first, second) = two_passes(&runtime);
    std::fs::set_permissions(&env, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !matches!(second, ComposeStatus::Unreadable { .. }),
        "{second:?}"
    );
    assert_eq!(first, second, "the verdict must be stable across passes");
    // The bind address comes from .env, so the port is not compared: no
    // false drift either
    assert_eq!(second, ComposeStatus::InSync);
}

#[test]
fn test_environment_file_label_replaces_dot_env() {
    let project = Project::new();
    project.write(".env", "WEB_BIND=0.0.0.0\n");
    project.write("prod.env", "WEB_BIND=127.0.0.1\n");
    let mut labels = project.labels(&["compose.yml"]);
    labels.push((LABEL_ENVIRONMENT_FILE, project.path("prod.env")));
    assert_eq!(
        confirmed(&runtime_with(&labels, ENV)),
        ComposeStatus::InSync
    );
}

#[test]
fn test_container_without_labels_is_undeclared() {
    let runtime = runtime_with(&[], ENV);
    let (first, second) = two_passes(&runtime);
    assert_eq!(first, ComposeStatus::Undeclared);
    assert_eq!(second, ComposeStatus::Undeclared);
}

#[test]
fn test_missing_file_is_indeterminate_after_two_passes() {
    let project = Project::new();
    let runtime = project.container(&["gone.yml"], ENV);
    let (first, second) = two_passes(&runtime);
    assert_eq!(first, ComposeStatus::Pending);
    let ComposeStatus::Unreadable { reason } = second else {
        panic!("expected unreadable, got {second:?}");
    };
    assert!(reason.contains("gone.yml"), "{reason}");
}

#[test]
fn test_invalid_yaml_is_indeterminate() {
    let project = Project::new();
    project.write("compose.yml", "services: [unclosed\n");
    let status = confirmed(&project.container(&["compose.yml"], ENV));
    assert!(
        matches!(&status, ComposeStatus::Unreadable { reason } if reason.contains("invalid YAML")),
        "{status:?}"
    );
}

#[test]
fn test_unreadable_file_is_indeterminate() {
    use std::os::unix::fs::PermissionsExt;
    // root reads anything, so the permission cannot be denied there
    // SAFETY: geteuid has no preconditions and cannot fail
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let project = Project::new();
    let path = project.dir.path().join("compose.yml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let status = confirmed(&project.container(&["compose.yml"], ENV));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        matches!(&status, ComposeStatus::Unreadable { reason } if reason.contains("ermission denied")),
        "{status:?}"
    );
}

#[test]
fn test_labels_without_files_are_indeterminate() {
    let labels = vec![
        (LABEL_PROJECT, "shop".to_string()),
        (LABEL_SERVICE, "web".to_string()),
    ];
    assert!(matches!(
        confirmed(&runtime_with(&labels, ENV)),
        ComposeStatus::Unreadable { .. }
    ));
}

#[test]
fn test_divergence_needs_two_identical_passes() {
    let project = Project::new();
    let in_sync = project.container(&["compose.yml"], ENV);
    let mut drifted = in_sync.clone();
    drifted.restart = "no".into();
    let mut other = in_sync.clone();
    other.networks.insert("edge-proxy".into());

    let mut audit = ComposeAudit::new(ComposeDriftConfig::default());
    let mut pass = |runtime: &ContainerRuntime| {
        audit.assess(std::slice::from_ref(runtime)).unwrap()[0]
            .status
            .clone()
    };
    // A deploy caught half-way: seen once, then gone
    assert_eq!(pass(&drifted), ComposeStatus::Pending);
    assert_eq!(pass(&in_sync), ComposeStatus::InSync);
    // Seen twice: reported
    assert_eq!(pass(&drifted), ComposeStatus::Pending);
    assert!(matches!(pass(&drifted), ComposeStatus::Drifted { .. }));
    assert!(matches!(pass(&drifted), ComposeStatus::Drifted { .. }));
    // A different divergence starts over
    assert_eq!(pass(&other), ComposeStatus::Pending);
    assert!(matches!(pass(&other), ComposeStatus::Drifted { .. }));
}

#[test]
fn test_env_key_presence_only() {
    let project = Project::new();
    // Declared with a value, missing in the container
    let missing = project.container(&["compose.yml"], &["PATH=/usr/bin"]);
    assert_eq!(
        described(&confirmed(&missing)),
        vec!["env API_URL: declared set, running absent"]
    );
    // Only in the container, not from the image: reported by key
    let mut extra = project.container(&["compose.yml"], &[ENV[0], ENV[1], "OLD_SECRET=s3cr3t"]);
    extra.image_env_keys = Some(BTreeSet::from(["PATH".to_string()]));
    let status = confirmed(&extra);
    assert_eq!(
        described(&status),
        vec!["env OLD_SECRET: declared absent, running set"]
    );
    // No value of any variable reaches the verdict
    let printed = format!("{status:?}");
    assert!(!printed.contains("s3cr3t"));
    assert!(!printed.contains("api.example.com"));
    // The same key with any value is in sync
    let same_key = project.container(&["compose.yml"], &["API_URL=other", "PATH=/x"]);
    assert_eq!(confirmed(&same_key), ComposeStatus::InSync);
}

#[test]
fn test_service_removed_from_compose_is_drift() {
    let project = Project::new();
    project.write("compose.yml", "services:\n  api:\n    image: shop/api:1\n");
    let status = confirmed(&project.container(&["compose.yml"], ENV));
    let ComposeStatus::Drifted { differences, .. } = status else {
        panic!("expected drift");
    };
    assert_eq!(differences[0].field, ComposeField::Service);
}

#[test]
fn test_silenced_services() {
    let project = Project::new();
    project.write(".env", "WEB_BIND=0.0.0.0\n");
    let runtime = project.container(&["compose.yml"], ENV);
    for entry in ["shop/web", "shop/*", "web"] {
        let mut audit = ComposeAudit::new(ComposeDriftConfig {
            enabled: true,
            silence: vec![entry.to_string()],
        });
        for _ in 0..2 {
            let status = &audit.assess(std::slice::from_ref(&runtime)).unwrap()[0].status;
            assert_eq!(*status, ComposeStatus::Silenced, "{entry}");
        }
    }
    let mut audit = ComposeAudit::new(ComposeDriftConfig {
        enabled: true,
        silence: vec!["other/web".into()],
    });
    audit.assess(std::slice::from_ref(&runtime));
    assert!(matches!(
        audit.assess(std::slice::from_ref(&runtime)).unwrap()[0].status,
        ComposeStatus::Drifted { .. }
    ));
}

#[test]
fn test_disabled_runs_nothing() {
    let mut audit = ComposeAudit::new(ComposeDriftConfig {
        enabled: false,
        silence: vec![],
    });
    assert!(audit.assess(&[runtime_with(&[], ENV)]).is_none());
}

#[test]
fn test_config_defaults_and_parsing() {
    assert_eq!(
        ComposeDriftConfig::default(),
        ComposeDriftConfig {
            enabled: true,
            silence: vec![]
        }
    );
    let parsed: ComposeDriftConfig = toml::from_str("silence = [\"shop/web\"]").unwrap();
    assert!(parsed.enabled);
    assert_eq!(parsed.silence, vec!["shop/web"]);
}

#[test]
fn test_targets_are_project_and_service() {
    let project = Project::new();
    let mut audit = ComposeAudit::new(ComposeDriftConfig::default());
    let out = audit
        .assess(&[
            project.container(&["compose.yml"], ENV),
            runtime_with(&[], ENV),
        ])
        .unwrap();
    assert_eq!(out[0].target, "shop/web");
    assert_eq!(out[0].container, "shop-web-1");
    assert_eq!(out[1].target, "shop-web-1");
}
