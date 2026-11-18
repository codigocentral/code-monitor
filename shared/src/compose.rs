//! Running container against the docker-compose file that declares it
//!
//! The Docker collector sees the container as it runs; the compose file on
//! disk says how the next `docker compose up` will recreate it. When the two
//! disagree nothing fails today — the difference is armed and goes off on the
//! next deploy: a tag older than the running one is a downgrade (on an
//! application with migrations already applied, a data loss), and a bind,
//! port or network present only on the container is silently undone.
//!
//! The server reads and compares; this module only carries the verdict, so
//! the audit can turn it into findings without knowing about YAML.

use serde::Serialize;

/// Which part of the service a difference is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposeField {
    /// The service is no longer in the compose file
    Service,
    Image,
    Ports,
    Volumes,
    Restart,
    Networks,
    NetworkMode,
    /// Presence of an environment variable. Only the key is ever compared;
    /// a value is never read from the container, compared or carried.
    EnvKey,
}

impl ComposeField {
    pub fn as_str(&self) -> &'static str {
        match self {
            ComposeField::Service => "service",
            ComposeField::Image => "image",
            ComposeField::Ports => "ports",
            ComposeField::Volumes => "volumes",
            ComposeField::Restart => "restart",
            ComposeField::Networks => "networks",
            ComposeField::NetworkMode => "network_mode",
            ComposeField::EnvKey => "env",
        }
    }
}

/// One way in which the running container differs from its declaration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ComposeDifference {
    pub field: ComposeField,
    /// What inside the field differs: a container port (`80/tcp`), a mount
    /// target, an environment key. Empty for single-valued fields.
    pub item: String,
    /// What the compose file declares (`absent` when it does not)
    pub declared: String,
    /// What the container runs with (`absent` when it does not)
    pub running: String,
}

impl ComposeDifference {
    /// One line for humans: `ports 80/tcp: declared 127.0.0.1:8080, running 0.0.0.0:8080`.
    pub fn describe(&self) -> String {
        let field = self.field.as_str();
        let subject = if self.item.is_empty() {
            field.to_string()
        } else {
            format!("{field} {}", self.item)
        };
        format!(
            "{subject}: declared {}, running {}",
            self.declared, self.running
        )
    }
}

/// The compose file declares an older tag of the image that is running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DowngradeRisk {
    pub declared: String,
    pub running: String,
}

/// The verdict on one container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ComposeStatus {
    /// Matches its declaration on every compared field
    InSync,
    /// No compose labels (started with `docker run`): nothing declares it,
    /// which is not a divergence
    Undeclared,
    /// The configuration lists this service as an intended divergence
    Silenced,
    /// Seen diverging once; reported only if the next pass agrees, so a
    /// container caught between `stop` and `up` is not a finding
    Pending,
    /// Diverges, confirmed in two consecutive passes
    Drifted {
        differences: Vec<ComposeDifference>,
        downgrade: Option<DowngradeRisk>,
    },
    /// The compose file could not be read or understood: indeterminate,
    /// confirmed in two consecutive passes
    Unreadable { reason: String },
}

impl ComposeStatus {
    /// Stable label for the `state` of `code_monitor_compose_services`.
    pub fn label(&self) -> &'static str {
        match self {
            ComposeStatus::InSync => "in_sync",
            ComposeStatus::Undeclared => "undeclared",
            ComposeStatus::Silenced => "silenced",
            ComposeStatus::Pending => "pending",
            ComposeStatus::Drifted { .. } => "drifted",
            ComposeStatus::Unreadable { .. } => "unreadable",
        }
    }

    /// Every label, in a fixed order, so the gauge always has all states.
    pub const LABELS: &'static [&'static str] = &[
        "in_sync",
        "undeclared",
        "silenced",
        "pending",
        "drifted",
        "unreadable",
    ];
}

/// One running container and what its compose file says about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComposeAssessment {
    /// `project/service`, or the container name when it has no compose labels
    pub target: String,
    pub container: String,
    pub status: ComposeStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_describe_with_and_without_item() {
        let port = ComposeDifference {
            field: ComposeField::Ports,
            item: "80/tcp".into(),
            declared: "127.0.0.1:8080".into(),
            running: "0.0.0.0:8080".into(),
        };
        assert_eq!(
            port.describe(),
            "ports 80/tcp: declared 127.0.0.1:8080, running 0.0.0.0:8080"
        );
        let restart = ComposeDifference {
            field: ComposeField::Restart,
            item: String::new(),
            declared: "always".into(),
            running: "no".into(),
        };
        assert_eq!(restart.describe(), "restart: declared always, running no");
    }

    #[test]
    fn test_every_status_has_a_listed_label() {
        let all = [
            ComposeStatus::InSync,
            ComposeStatus::Undeclared,
            ComposeStatus::Silenced,
            ComposeStatus::Pending,
            ComposeStatus::Drifted {
                differences: vec![],
                downgrade: None,
            },
            ComposeStatus::Unreadable {
                reason: String::new(),
            },
        ];
        for status in all {
            assert!(ComposeStatus::LABELS.contains(&status.label()));
        }
        assert_eq!(ComposeStatus::LABELS.len(), 6);
    }

    // ─────────────────────────────────────────
    // findings
    // ─────────────────────────────────────────

    use crate::alerts::AlertSeverity;
    use crate::findings::{evaluate, Check, Finding, HostSnapshot};

    fn assessment(target: &str, status: ComposeStatus) -> ComposeAssessment {
        ComposeAssessment {
            target: target.into(),
            container: format!("{}-1", target.replace('/', "-")),
            status,
        }
    }

    fn diff(field: ComposeField, item: &str, declared: &str, running: &str) -> ComposeDifference {
        ComposeDifference {
            field,
            item: item.into(),
            declared: declared.into(),
            running: running.into(),
        }
    }

    fn findings_for(compose: Vec<ComposeAssessment>) -> Vec<Finding> {
        let snapshot = HostSnapshot {
            containers: Some(vec![]),
            compose: Some(compose),
            ..Default::default()
        };
        evaluate(&snapshot, chrono::Utc::now())
    }

    #[test]
    fn test_quiet_states_produce_no_finding() {
        let findings = findings_for(vec![
            assessment("shop/web", ComposeStatus::InSync),
            assessment("adhoc", ComposeStatus::Undeclared),
            assessment("shop/legacy", ComposeStatus::Silenced),
            assessment("shop/worker", ComposeStatus::Pending),
        ]);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn test_drift_is_a_warning_with_the_differences_in_detail() {
        let findings = findings_for(vec![assessment(
            "shop/web",
            ComposeStatus::Drifted {
                differences: vec![
                    diff(
                        ComposeField::Ports,
                        "80/tcp",
                        "127.0.0.1:8080",
                        "0.0.0.0:8080",
                    ),
                    diff(ComposeField::EnvKey, "API_URL", "set", "absent"),
                ],
                downgrade: None,
            },
        )]);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.check, Check::ComposeDrift);
        assert_eq!(f.severity, AlertSeverity::Warning);
        assert_eq!(f.target, "shop/web");
        assert_eq!(f.value, 2.0);
        let detail = f.detail.as_deref().unwrap();
        assert!(detail.contains("ports 80/tcp: declared 127.0.0.1:8080, running 0.0.0.0:8080"));
        assert!(detail.contains("env API_URL: declared set, running absent"));
        assert!(f.guidance.is_some());
    }

    #[test]
    fn test_downgrade_is_its_own_finding() {
        let image = diff(ComposeField::Image, "", "app:1.4.0", "app:1.5.2");
        let downgrade = Some(DowngradeRisk {
            declared: "app:1.4.0".into(),
            running: "app:1.5.2".into(),
        });
        let only_image = findings_for(vec![assessment(
            "shop/api",
            ComposeStatus::Drifted {
                differences: vec![image.clone()],
                downgrade: downgrade.clone(),
            },
        )]);
        assert_eq!(only_image.len(), 1);
        assert_eq!(only_image[0].check, Check::ComposeDowngradeRisk);
        assert_eq!(only_image[0].severity, AlertSeverity::Warning);
        assert_eq!(
            only_image[0].detail.as_deref(),
            Some("compose declares app:1.4.0, the container runs app:1.5.2")
        );

        // Other differences still count as drift, without the image twice
        let both = findings_for(vec![assessment(
            "shop/api",
            ComposeStatus::Drifted {
                differences: vec![image, diff(ComposeField::Restart, "", "always", "no")],
                downgrade,
            },
        )]);
        let checks: Vec<_> = both.iter().map(|f| (f.check, f.value)).collect();
        assert_eq!(
            checks,
            vec![
                (Check::ComposeDrift, 1.0),
                (Check::ComposeDowngradeRisk, 1.0)
            ]
        );
    }

    #[test]
    fn test_unreadable_is_a_visible_info() {
        let findings = findings_for(vec![assessment(
            "shop/web",
            ComposeStatus::Unreadable {
                reason: "permission denied".into(),
            },
        )]);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].check, Check::ComposeUnreadable);
        assert_eq!(findings[0].severity, AlertSeverity::Info);
        assert_eq!(findings[0].detail.as_deref(), Some("permission denied"));
    }

    #[test]
    fn test_long_drift_detail_is_bounded() {
        let differences = (0..20)
            .map(|i| diff(ComposeField::EnvKey, &format!("K{i}"), "set", "absent"))
            .collect();
        let findings = findings_for(vec![assessment(
            "shop/web",
            ComposeStatus::Drifted {
                differences,
                downgrade: None,
            },
        )]);
        assert_eq!(findings[0].value, 20.0);
        assert!(findings[0]
            .detail
            .as_deref()
            .unwrap()
            .ends_with("and 12 more"));
    }

    #[test]
    fn test_no_compose_finding_while_docker_is_blind() {
        let snapshot = HostSnapshot {
            containers: None,
            compose: Some(vec![assessment(
                "shop/web",
                ComposeStatus::Unreadable { reason: "x".into() },
            )]),
            ..Default::default()
        };
        assert!(evaluate(&snapshot, chrono::Utc::now()).is_empty());
    }
}
