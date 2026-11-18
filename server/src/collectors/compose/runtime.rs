//! What a running container actually has, read from `docker inspect`

use super::model::{
    is_anonymous_volume_name, normalize_restart, resolve_path, Mount, MountKind, Port,
};
use bollard::models::{
    ContainerInspectResponse, MountPoint, MountPointTypeEnum, PortMap, RestartPolicy,
    RestartPolicyNameEnum,
};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

pub const LABEL_PROJECT: &str = "com.docker.compose.project";
pub const LABEL_SERVICE: &str = "com.docker.compose.service";
pub const LABEL_CONFIG_FILES: &str = "com.docker.compose.project.config_files";
pub const LABEL_WORKING_DIR: &str = "com.docker.compose.project.working_dir";
pub const LABEL_ENVIRONMENT_FILE: &str = "com.docker.compose.project.environment_file";
pub const LABEL_ONEOFF: &str = "com.docker.compose.oneoff";

/// Where the compose project of a container lives, from its labels.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComposeLabels {
    pub project: String,
    pub service: String,
    /// Every `-f` file, in the order compose merged them
    pub config_files: Vec<String>,
    pub working_dir: String,
    /// `--env-file` files, when compose recorded them; empty means `.env`
    pub env_files: Vec<String>,
}

impl ComposeLabels {
    /// `project/service`: stable across container recreation.
    pub fn target(&self) -> String {
        format!("{}/{}", self.project, self.service)
    }
}

/// The fields of a running container that a compose file declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContainerRuntime {
    pub name: String,
    /// `None` when the container has no compose labels (`docker run`)
    pub compose: Option<ComposeLabels>,
    /// The reference the container was created from (`Config.Image`)
    pub image: String,
    /// Image ID, to look up the variables the image itself defines
    pub image_id: String,
    pub ports: Vec<Port>,
    pub mounts: Vec<Mount>,
    pub restart: String,
    pub network_mode: String,
    pub networks: BTreeSet<String>,
    /// Environment keys. The value of each `KEY=value` is dropped as it is
    /// read: it is never stored, compared, logged or sent anywhere.
    pub env_keys: BTreeSet<String>,
    /// Keys the image defines (`ENV` in its Dockerfile), when known; they
    /// are in every container and no compose file declares them
    pub image_env_keys: Option<BTreeSet<String>>,
}

/// The keys of `KEY=value` entries; values are not copied.
pub fn env_keys(entries: &[String]) -> BTreeSet<String> {
    entries
        .iter()
        .filter_map(|entry| entry.split('=').next())
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_list(value: Option<&String>) -> Vec<String> {
    value
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The compose origin of a container, or `None` without the labels.
pub fn compose_labels(labels: &HashMap<String, String>) -> Option<ComposeLabels> {
    let project = labels.get(LABEL_PROJECT)?;
    let service = labels.get(LABEL_SERVICE)?;
    Some(ComposeLabels {
        project: project.clone(),
        service: service.clone(),
        config_files: split_list(labels.get(LABEL_CONFIG_FILES)),
        working_dir: labels.get(LABEL_WORKING_DIR).cloned().unwrap_or_default(),
        env_files: split_list(labels.get(LABEL_ENVIRONMENT_FILE)),
    })
}

fn ports(map: &PortMap) -> Vec<Port> {
    let mut out = Vec::new();
    for (key, bindings) in map {
        let (port, protocol) = key.split_once('/').unwrap_or((key.as_str(), "tcp"));
        let Ok(port) = port.parse::<u32>() else {
            continue;
        };
        for binding in bindings.iter().flatten() {
            out.push(Port::new(
                binding.host_ip.as_deref().unwrap_or(""),
                binding.host_port.as_deref().unwrap_or(""),
                port,
                protocol,
            ));
        }
    }
    out.sort();
    out
}

fn mount(point: &MountPoint) -> Option<Mount> {
    let target = point.destination.clone()?;
    let read_only = point.rw == Some(false);
    let (kind, source) = match point.typ? {
        MountPointTypeEnum::BIND => (
            MountKind::Bind,
            point
                .source
                .as_deref()
                .map(|s| resolve_path(Path::new("/"), s)),
        ),
        MountPointTypeEnum::VOLUME => match point.name.as_deref() {
            Some(name) if !is_anonymous_volume_name(name) => {
                (MountKind::Volume, Some(name.to_string()))
            }
            _ => (MountKind::Anonymous, None),
        },
        // tmpfs, named pipes and cluster volumes are not compared
        _ => return None,
    };
    Some(Mount {
        target,
        kind,
        source,
        read_only,
    })
}

fn restart(policy: Option<&RestartPolicy>) -> String {
    let name = policy.and_then(|p| p.name);
    let retries = policy.and_then(|p| p.maximum_retry_count).unwrap_or(0);
    let text = match name {
        Some(RestartPolicyNameEnum::ALWAYS) => "always".to_string(),
        Some(RestartPolicyNameEnum::UNLESS_STOPPED) => "unless-stopped".to_string(),
        Some(RestartPolicyNameEnum::ON_FAILURE) if retries > 0 => format!("on-failure:{retries}"),
        Some(RestartPolicyNameEnum::ON_FAILURE) => "on-failure".to_string(),
        _ => "no".to_string(),
    };
    normalize_restart(&text)
}

/// The compose-relevant state of a running container.
///
/// `None` for a container that is not running (its next start reuses the
/// same configuration; recreation is what the compose file decides) and for
/// `compose run` one-offs, which are not meant to match the service.
pub fn runtime_from_inspect(inspect: &ContainerInspectResponse) -> Option<ContainerRuntime> {
    let running = inspect.state.as_ref().and_then(|s| s.running) == Some(true);
    let config = inspect.config.as_ref();
    let labels = config.and_then(|c| c.labels.as_ref());
    let oneoff = labels
        .and_then(|l| l.get(LABEL_ONEOFF))
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if !running || oneoff {
        return None;
    }
    let host = inspect.host_config.as_ref();
    Some(ContainerRuntime {
        name: inspect
            .name
            .as_deref()
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        compose: labels.and_then(compose_labels),
        image: config.and_then(|c| c.image.clone()).unwrap_or_default(),
        image_id: inspect.image.clone().unwrap_or_default(),
        ports: host
            .and_then(|h| h.port_bindings.as_ref())
            .map(ports)
            .unwrap_or_default(),
        mounts: inspect.mounts.iter().flatten().filter_map(mount).collect(),
        restart: restart(host.and_then(|h| h.restart_policy.as_ref())),
        network_mode: host
            .and_then(|h| h.network_mode.clone())
            .unwrap_or_default(),
        networks: inspect
            .network_settings
            .as_ref()
            .and_then(|n| n.networks.as_ref())
            .map(|n| n.keys().cloned().collect())
            .unwrap_or_default(),
        env_keys: config
            .and_then(|c| c.env.as_deref())
            .map(env_keys)
            .unwrap_or_default(),
        image_env_keys: None,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use bollard::models::{
        ContainerConfig, ContainerState, EndpointSettings, HostConfig, NetworkSettings, PortBinding,
    };

    /// An inspect response as Docker returns it for a compose container.
    pub fn inspect(labels: &[(&str, &str)], env: &[&str]) -> ContainerInspectResponse {
        let mut port_bindings = PortMap::new();
        port_bindings.insert(
            "80/tcp".into(),
            Some(vec![PortBinding {
                host_ip: Some("127.0.0.1".into()),
                host_port: Some("8080".into()),
            }]),
        );
        // Exposed but not published
        port_bindings.insert("9000/tcp".into(), None);
        let mut networks = HashMap::new();
        networks.insert("shop_default".to_string(), EndpointSettings::default());
        ContainerInspectResponse {
            name: Some("/shop-web-1".into()),
            image: Some("sha256:feed".into()),
            state: Some(ContainerState {
                running: Some(true),
                ..Default::default()
            }),
            config: Some(ContainerConfig {
                image: Some("registry.example.com/shop/web:1.5.2".into()),
                env: Some(env.iter().map(|e| e.to_string()).collect()),
                labels: Some(
                    labels
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                ),
                ..Default::default()
            }),
            host_config: Some(HostConfig {
                port_bindings: Some(port_bindings),
                restart_policy: Some(RestartPolicy {
                    name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                    maximum_retry_count: Some(0),
                }),
                network_mode: Some("shop_default".into()),
                ..Default::default()
            }),
            mounts: Some(vec![
                MountPoint {
                    typ: Some(MountPointTypeEnum::BIND),
                    source: Some("/srv/shop/conf/".into()),
                    destination: Some("/etc/app".into()),
                    rw: Some(false),
                    ..Default::default()
                },
                MountPoint {
                    typ: Some(MountPointTypeEnum::VOLUME),
                    name: Some("shop_data".into()),
                    destination: Some("/var/lib/app".into()),
                    rw: Some(true),
                    ..Default::default()
                },
                MountPoint {
                    typ: Some(MountPointTypeEnum::VOLUME),
                    name: Some("0123456789abcdef".repeat(4)),
                    destination: Some("/cache".into()),
                    rw: Some(true),
                    ..Default::default()
                },
                MountPoint {
                    typ: Some(MountPointTypeEnum::TMPFS),
                    destination: Some("/run".into()),
                    ..Default::default()
                },
            ]),
            network_settings: Some(NetworkSettings {
                networks: Some(networks),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    pub const COMPOSE_LABELS: &[(&str, &str)] = &[
        (LABEL_PROJECT, "shop"),
        (LABEL_SERVICE, "web"),
        (
            LABEL_CONFIG_FILES,
            "/srv/shop/compose.yml,/srv/shop/compose.override.yml",
        ),
        (LABEL_WORKING_DIR, "/srv/shop"),
    ];

    #[test]
    fn test_reads_every_compared_field() {
        let rt =
            runtime_from_inspect(&inspect(COMPOSE_LABELS, &["PATH=/bin", "API_URL=x"])).unwrap();
        assert_eq!(rt.name, "shop-web-1");
        assert_eq!(rt.image, "registry.example.com/shop/web:1.5.2");
        assert_eq!(rt.image_id, "sha256:feed");
        assert_eq!(rt.ports, vec![Port::new("127.0.0.1", "8080", 80, "tcp")]);
        assert_eq!(rt.restart, "unless-stopped");
        assert_eq!(rt.network_mode, "shop_default");
        assert_eq!(rt.networks, BTreeSet::from(["shop_default".to_string()]));
        let labels = rt.compose.unwrap();
        assert_eq!(labels.target(), "shop/web");
        assert_eq!(labels.config_files.len(), 2);
        assert_eq!(labels.working_dir, "/srv/shop");
        assert!(labels.env_files.is_empty());
    }

    #[test]
    fn test_mounts_keep_kind_source_and_mode() {
        let rt = runtime_from_inspect(&inspect(COMPOSE_LABELS, &[])).unwrap();
        let by_target: HashMap<_, _> = rt.mounts.iter().map(|m| (m.target.as_str(), m)).collect();
        assert_eq!(by_target.len(), 3, "tmpfs is not compared");
        let conf = by_target["/etc/app"];
        assert_eq!(conf.kind, MountKind::Bind);
        assert_eq!(conf.source.as_deref(), Some("/srv/shop/conf"));
        assert!(conf.read_only);
        assert_eq!(by_target["/var/lib/app"].kind, MountKind::Volume);
        assert!(!by_target["/var/lib/app"].read_only);
        assert_eq!(by_target["/cache"].kind, MountKind::Anonymous);
    }

    #[test]
    fn test_env_values_are_dropped_on_read() {
        let rt = runtime_from_inspect(&inspect(
            COMPOSE_LABELS,
            &["DB_PASSWORD=hunter2", "EMPTY=", "FLAG"],
        ))
        .unwrap();
        assert_eq!(
            rt.env_keys,
            BTreeSet::from(["DB_PASSWORD".into(), "EMPTY".into(), "FLAG".into()])
        );
        // Nothing the runtime carries holds a value
        assert!(!format!("{rt:?}").contains("hunter2"));
    }

    #[test]
    fn test_without_labels_has_no_compose_origin() {
        let rt = runtime_from_inspect(&inspect(&[], &[])).unwrap();
        assert!(rt.compose.is_none());
    }

    #[test]
    fn test_stopped_and_oneoff_containers_are_skipped() {
        let mut stopped = inspect(COMPOSE_LABELS, &[]);
        stopped.state = Some(ContainerState {
            running: Some(false),
            ..Default::default()
        });
        assert!(runtime_from_inspect(&stopped).is_none());

        let mut labels = COMPOSE_LABELS.to_vec();
        labels.push((LABEL_ONEOFF, "True"));
        assert!(runtime_from_inspect(&inspect(&labels, &[])).is_none());
    }

    #[test]
    fn test_restart_policies() {
        let policy = |name, retries| RestartPolicy {
            name: Some(name),
            maximum_retry_count: Some(retries),
        };
        assert_eq!(restart(None), "no");
        assert_eq!(
            restart(Some(&policy(RestartPolicyNameEnum::EMPTY, 0))),
            "no"
        );
        assert_eq!(
            restart(Some(&policy(RestartPolicyNameEnum::ALWAYS, 0))),
            "always"
        );
        assert_eq!(
            restart(Some(&policy(RestartPolicyNameEnum::ON_FAILURE, 3))),
            "on-failure:3"
        );
        assert_eq!(
            restart(Some(&policy(RestartPolicyNameEnum::ON_FAILURE, 0))),
            "on-failure"
        );
    }

    #[test]
    fn test_env_file_label_is_split() {
        let mut labels = COMPOSE_LABELS.to_vec();
        labels.push((LABEL_ENVIRONMENT_FILE, "/srv/shop/.env, /srv/shop/prod.env"));
        let rt = runtime_from_inspect(&inspect(&labels, &[])).unwrap();
        assert_eq!(
            rt.compose.unwrap().env_files,
            vec!["/srv/shop/.env", "/srv/shop/prod.env"]
        );
    }
}
