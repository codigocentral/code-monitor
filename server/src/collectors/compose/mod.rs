//! Compose drift: the running container against the compose file on disk
//!
//! The Docker collector reads the container as it runs; `docker compose up`
//! recreates it from the file. When the two disagree the error is armed and
//! goes off on the next deploy, done by someone who thinks they are only
//! restarting a service: an older tag in the file is a downgrade, and a bind,
//! port or network present only on the container is silently undone.
//!
//! Compose labels every container with the files it was created from
//! (`com.docker.compose.project.config_files`), the project directory and
//! the service. From those this module reads the files (merged in label
//! order, `${VAR}` resolved from the project's `.env`), compares image,
//! ports with their bind address, mounts with `:ro`, restart policy,
//! networks and environment keys, and confirms a divergence over two
//! consecutive audit passes before reporting it — a container caught between
//! `stop` and `up` during a deploy is not a finding.
//!
//! Environment values are never read from the container, compared, logged
//! or reported: only the presence of each key. The `.env` file is read only
//! to resolve the variables of the compared fields.
//!
//! Layout: [`runtime`] reads `docker inspect`, [`declared`] parses the files,
//! [`compare`] decides; all three are pure. This module does the file I/O,
//! the silencing and the two-pass confirmation.

pub mod compare;
pub mod declared;
pub mod interpolate;
pub mod model;
pub mod runtime;

use declared::{DeclaredProject, ProjectContext};
use interpolate::Variables;
use runtime::{ComposeLabels, ContainerRuntime};
use serde::{Deserialize, Serialize};
use shared::compose::{ComposeAssessment, ComposeStatus};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Largest compose or env file read; anything bigger is not a compose file.
const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// `[compose_drift]` in the server configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComposeDriftConfig {
    /// Compare running containers with their compose files (default true)
    pub enabled: bool,
    /// Services whose divergence is intended: `project/service`,
    /// `project/*` or a bare `service` (any project)
    pub silence: Vec<String>,
}

impl Default for ComposeDriftConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            silence: Vec::new(),
        }
    }
}

impl ComposeDriftConfig {
    pub fn is_silenced(&self, labels: &ComposeLabels) -> bool {
        let target = labels.target();
        let any_service = format!("{}/*", labels.project);
        self.silence
            .iter()
            .any(|entry| *entry == target || *entry == any_service || *entry == labels.service)
    }
}

/// Read a file, refusing anything larger than [`MAX_FILE_BYTES`].
fn read_capped(path: &Path) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Err(std::io::Error::other("larger than 1 MiB"));
    }
    let mut text = String::new();
    file.take(MAX_FILE_BYTES).read_to_string(&mut text)?;
    Ok(text)
}

fn absolute(working_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}

/// The interpolation variables: the `environment_file` label's files, or
/// the project's `.env`. A missing file is no variables, not an error.
///
/// Neither is one the agent may not open: `.env` files hold secrets, and the
/// recommended setup grants the agent the compose files only. The fields
/// that need a variable from it are then left uncompared, exactly as with an
/// unset variable, instead of hiding the whole project as unreadable.
fn read_variables(labels: &ComposeLabels, working_dir: &Path) -> Result<Variables, String> {
    let paths: Vec<PathBuf> = if labels.env_files.is_empty() {
        vec![working_dir.join(".env")]
    } else {
        labels
            .env_files
            .iter()
            .map(|f| absolute(working_dir, f))
            .collect()
    };
    let mut vars = Variables::default();
    for path in paths {
        match read_capped(&path) {
            Ok(text) => vars.extend(Variables::parse_env_file(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                tracing::debug!(
                    "compose: {} not readable, variables left unresolved",
                    path.display()
                );
            }
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        }
    }
    Ok(vars)
}

/// Read and parse the project a container's labels point to.
///
/// `Err` is the reason the project is indeterminate, for the
/// `compose_unreadable` detail: a path and an I/O or YAML error, never the
/// content of a file.
pub fn load_project(labels: &ComposeLabels) -> Result<DeclaredProject, String> {
    if labels.config_files.is_empty() {
        return Err("the container's labels do not name its compose files".into());
    }
    // As compose resolves it, a POSIX path: the base for relative binds.
    let working_dir = if labels.working_dir.is_empty() {
        model::posix_parent(&labels.config_files[0]).to_string()
    } else {
        labels.working_dir.clone()
    };
    // The same directory on the agent's own filesystem, to read the files.
    let host_dir = if labels.working_dir.is_empty() {
        Path::new(&labels.config_files[0])
            .parent()
            .unwrap_or(Path::new("/"))
            .to_path_buf()
    } else {
        PathBuf::from(&labels.working_dir)
    };
    let vars = read_variables(labels, &host_dir)?;
    let mut files = Vec::with_capacity(labels.config_files.len());
    for file in &labels.config_files {
        let path = absolute(&host_dir, file);
        let text =
            read_capped(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        files.push((path.display().to_string(), text));
    }
    let ctx = ProjectContext {
        name: &labels.project,
        working_dir: &working_dir,
        vars: &vars,
    };
    declared::parse_project(&files, &ctx)
}

/// Verdict for one container, without the confirmation.
///
/// `projects` caches each project for the pass, so replicas and sibling
/// services read their files once.
fn assess_one(
    runtime: &ContainerRuntime,
    config: &ComposeDriftConfig,
    projects: &mut HashMap<ComposeLabelsKey, Result<DeclaredProject, String>>,
) -> ComposeAssessment {
    let Some(labels) = &runtime.compose else {
        return ComposeAssessment {
            target: runtime.name.clone(),
            container: runtime.name.clone(),
            status: ComposeStatus::Undeclared,
        };
    };
    let status = if config.is_silenced(labels) {
        ComposeStatus::Silenced
    } else {
        let project = projects
            .entry(ComposeLabelsKey::of(labels))
            .or_insert_with(|| load_project(labels));
        match project {
            Ok(project) => compare::compare(project.services.get(&labels.service), runtime),
            Err(reason) => ComposeStatus::Unreadable {
                reason: reason.clone(),
            },
        }
    };
    ComposeAssessment {
        target: labels.target(),
        container: runtime.name.clone(),
        status,
    }
}

/// What identifies a project's files: the same files, directory and env
/// files parse to the same result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ComposeLabelsKey {
    config_files: Vec<String>,
    working_dir: String,
    env_files: Vec<String>,
    project: String,
}

impl ComposeLabelsKey {
    fn of(labels: &ComposeLabels) -> Self {
        Self {
            config_files: labels.config_files.clone(),
            working_dir: labels.working_dir.clone(),
            env_files: labels.env_files.clone(),
            project: labels.project.clone(),
        }
    }
}

/// Holds a divergence back until the next pass sees the same one.
#[derive(Debug, Default)]
pub struct Confirmation {
    previous: HashMap<(String, String), ComposeStatus>,
}

/// Drift and unreadable files are what a deploy in progress produces.
fn needs_confirmation(status: &ComposeStatus) -> bool {
    matches!(
        status,
        ComposeStatus::Drifted { .. } | ComposeStatus::Unreadable { .. }
    )
}

impl Confirmation {
    /// Report a divergence only when the previous pass saw exactly the same
    /// one for the same container; otherwise it is `Pending`.
    pub fn confirm(&mut self, raw: Vec<ComposeAssessment>) -> Vec<ComposeAssessment> {
        let mut seen = HashMap::new();
        let confirmed = raw
            .into_iter()
            .map(|mut assessment| {
                if needs_confirmation(&assessment.status) {
                    let key = (assessment.target.clone(), assessment.container.clone());
                    let repeated = self.previous.get(&key) == Some(&assessment.status);
                    seen.insert(key, assessment.status.clone());
                    if !repeated {
                        assessment.status = ComposeStatus::Pending;
                    }
                }
                assessment
            })
            .collect();
        self.previous = seen;
        confirmed
    }
}

/// The compose comparison as the audit runs it, pass after pass.
#[derive(Debug, Default)]
pub struct ComposeAudit {
    config: ComposeDriftConfig,
    confirmation: Confirmation,
}

impl ComposeAudit {
    pub fn new(config: ComposeDriftConfig) -> Self {
        Self {
            config,
            confirmation: Confirmation::default(),
        }
    }

    /// Assess every running container; `None` when the check is disabled.
    pub fn assess(&mut self, runtimes: &[ContainerRuntime]) -> Option<Vec<ComposeAssessment>> {
        if !self.config.enabled {
            return None;
        }
        let mut projects = HashMap::new();
        let raw: Vec<ComposeAssessment> = runtimes
            .iter()
            .map(|runtime| assess_one(runtime, &self.config, &mut projects))
            .collect();
        for assessment in &raw {
            if let ComposeStatus::Unreadable { reason } = &assessment.status {
                tracing::debug!("compose: {} indeterminate: {}", assessment.target, reason);
            }
        }
        Some(self.confirmation.confirm(raw))
    }
}

#[cfg(test)]
mod tests;
