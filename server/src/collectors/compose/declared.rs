//! What the compose files declare for each service, after merge and
//! interpolation
//!
//! Pure: the caller reads the files, this module only parses their text.
//! Files are merged in the order of the `config_files` label, the way
//! `docker compose -f a.yml -f b.yml` does: single values are replaced,
//! `ports` are appended, `volumes` are replaced by mount target, `networks`
//! and `environment` by key.

use super::interpolate::Variables;
use super::model::{normalize_restart, resolve_path, Mount, MountKind, Port};
use shared::compose::ComposeField;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlLoader};

/// More ports than this in one range is not a real deployment; the field
/// becomes unknown rather than allocating for it.
const MAX_PORT_RANGE: u32 = 1024;

/// What the parse needs to know about the project.
pub struct ProjectContext<'a> {
    /// Compose project name, which prefixes volume and network names
    pub name: &'a str,
    /// Where relative bind paths are resolved from
    pub working_dir: &'a Path,
    pub vars: &'a Variables,
}

/// One service as the merged compose files declare it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeclaredService {
    /// `None` for a service that is only built locally
    pub image: Option<String>,
    pub ports: BTreeSet<Port>,
    pub mounts: Vec<Mount>,
    pub restart: String,
    pub network_mode: Option<String>,
    /// Real network names (`project_default`, or the `name:` given)
    pub networks: BTreeSet<String>,
    /// Environment keys; `true` when the file gives the key a value, `false`
    /// for a pass-through (`- KEY`) that depends on the deployer's shell.
    /// No value is ever kept.
    pub env: BTreeMap<String, bool>,
    /// Keys also come from `env_file`, which is not read
    pub env_file: bool,
    /// Fields that could not be resolved and must not be compared
    pub unknown: BTreeSet<ComposeField>,
}

/// Every service of the merged project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeclaredProject {
    pub services: BTreeMap<String, DeclaredService>,
}

/// A mount as written, before names and paths are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawMount {
    target: String,
    source: Option<String>,
    /// `bind` or `volume` from the long syntax; `None` decides by the source
    kind: Option<MountKind>,
    read_only: bool,
}

/// A service as one file declares it.
#[derive(Debug, Clone, Default)]
struct ServiceLayer {
    image: Option<String>,
    restart: Option<String>,
    network_mode: Option<String>,
    ports: Vec<Port>,
    mounts: Vec<RawMount>,
    networks: BTreeSet<String>,
    env: BTreeMap<String, bool>,
    env_file: bool,
    unknown: BTreeSet<ComposeField>,
}

/// `name:` / `external:` of a top-level volume or network.
#[derive(Debug, Clone, Default)]
struct Resource {
    name: Option<String>,
    external: bool,
}

/// One file, or the merge of several.
#[derive(Debug, Default)]
struct Layer {
    services: BTreeMap<String, ServiceLayer>,
    volumes: BTreeMap<String, Resource>,
    networks: BTreeMap<String, Resource>,
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

fn get<'a>(hash: &'a Hash, name: &str) -> Option<&'a Yaml> {
    hash.get(&key(name)).filter(|v| !matches!(v, Yaml::Null))
}

/// A scalar as text; mappings and sequences have none.
fn scalar(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A mapping with its `<<: *anchor` merge keys applied; explicit keys win,
/// and among merged mappings the first one listed wins.
fn mapping(value: &Yaml) -> Option<Hash> {
    let Yaml::Hash(hash) = value else {
        return None;
    };
    let merged: Vec<&Yaml> = match hash.get(&key("<<")) {
        Some(Yaml::Array(items)) => items.iter().collect(),
        Some(item) => vec![item],
        None => vec![],
    };
    let mut out = Hash::new();
    for source in merged.iter().rev().filter_map(|y| mapping(y)) {
        out.extend(source);
    }
    for (k, v) in hash.iter().filter(|(k, _)| **k != key("<<")) {
        out.insert(k.clone(), v.clone());
    }
    Some(out)
}

/// Interpolate a scalar field; an unresolved variable marks it unknown.
fn interpolated(
    value: Option<&Yaml>,
    vars: &Variables,
    field: ComposeField,
    unknown: &mut BTreeSet<ComposeField>,
) -> Option<String> {
    let text = scalar(value?)?;
    match vars.interpolate(&text) {
        Ok(resolved) => Some(resolved),
        Err(_) => {
            unknown.insert(field);
            None
        }
    }
}

fn parse_range(text: &str) -> Option<(u32, u32)> {
    let (start, end) = text.split_once('-').unwrap_or((text, text));
    let start: u32 = start.trim().parse().ok()?;
    let end: u32 = end.trim().parse().ok()?;
    (start <= end && end <= u32::from(u16::MAX)).then_some((start, end))
}

/// Expand `host_ip`, a host port or range and a container port or range into
/// one entry per container port.
fn expand_ports(host_ip: &str, host: &str, container: &str, protocol: &str) -> Option<Vec<Port>> {
    let (c_start, c_end) = parse_range(container)?;
    if c_end - c_start >= MAX_PORT_RANGE {
        return None;
    }
    if host.is_empty() {
        return Some(
            (c_start..=c_end)
                .map(|p| Port::new(host_ip, "", p, protocol))
                .collect(),
        );
    }
    let (h_start, h_end) = parse_range(host)?;
    // A host range for a single port lets Docker pick one: not knowable
    if h_end - h_start != c_end - c_start {
        return None;
    }
    Some(
        (c_start..=c_end)
            .map(|p| Port::new(host_ip, &(h_start + p - c_start).to_string(), p, protocol))
            .collect(),
    )
}

/// Split `[::1]:8080:80` or `127.0.0.1:8080:80` into address and the rest.
fn split_host_ip(spec: &str) -> (&str, &str) {
    if let Some(inner) = spec.strip_prefix('[') {
        if let Some((ip, rest)) = inner.split_once("]:") {
            return (ip, rest);
        }
    }
    if spec.matches(':').count() == 2 {
        return spec.split_once(':').unwrap_or(("", spec));
    }
    ("", spec)
}

/// Short syntax: `80`, `8080:80`, `127.0.0.1:8080:80/udp`, `127.0.0.1::80`.
fn short_port(spec: &str) -> Option<Vec<Port>> {
    let (spec, protocol) = spec.rsplit_once('/').unwrap_or((spec, "tcp"));
    let (host_ip, rest) = split_host_ip(spec);
    let (host, container) = rest.rsplit_once(':').unwrap_or(("", rest));
    expand_ports(host_ip, host, container, protocol)
}

/// Long syntax: `target`, `published`, `host_ip`, `protocol`.
fn long_port(entry: &Hash, vars: &Variables) -> Option<Vec<Port>> {
    let text = |name: &str| -> Option<String> {
        get(entry, name)
            .and_then(scalar)
            .map(|s| vars.interpolate(&s).ok())
            .unwrap_or_else(|| Some(String::new()))
    };
    let target = text("target")?;
    let protocol = text("protocol")?;
    let protocol = if protocol.is_empty() {
        "tcp".into()
    } else {
        protocol
    };
    expand_ports(&text("host_ip")?, &text("published")?, &target, &protocol)
}

fn port_entry(entry: &Yaml, vars: &Variables) -> Option<Vec<Port>> {
    match entry {
        Yaml::Hash(hash) => long_port(hash, vars),
        other => short_port(&vars.interpolate(&scalar(other)?).ok()?),
    }
}

fn parse_ports(
    value: Option<&Yaml>,
    vars: &Variables,
    unknown: &mut BTreeSet<ComposeField>,
) -> Vec<Port> {
    let Some(Yaml::Array(items)) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in items {
        match port_entry(entry, vars) {
            Some(ports) => out.extend(ports),
            None => {
                unknown.insert(ComposeField::Ports);
            }
        }
    }
    out
}

/// Short syntax: `/data`, `data:/data`, `./conf:/etc/app:ro,z`.
fn short_mount(spec: &str) -> Option<RawMount> {
    let parts: Vec<&str> = spec.split(':').collect();
    let (source, target, mode) = match parts.as_slice() {
        [target] => (None, *target, ""),
        [source, target] => (Some(*source), *target, ""),
        [source, target, mode] => (Some(*source), *target, *mode),
        _ => return None,
    };
    Some(RawMount {
        target: target.to_string(),
        source: source.filter(|s| !s.is_empty()).map(str::to_string),
        kind: None,
        read_only: mode.split(',').any(|option| option == "ro"),
    })
}

/// Long syntax; `Ok(None)` for tmpfs and named pipes, which are not compared.
fn long_mount(entry: &Hash, vars: &Variables) -> Result<Option<RawMount>, ()> {
    let text = |name: &str| -> Result<Option<String>, ()> {
        match get(entry, name).and_then(scalar) {
            Some(s) => vars.interpolate(&s).map(Some).map_err(|_| ()),
            None => Ok(None),
        }
    };
    let kind = match text("type")?.as_deref() {
        Some("bind") => Some(MountKind::Bind),
        Some("volume") | None => Some(MountKind::Volume),
        Some(_) => return Ok(None),
    };
    let target = text("target")?.ok_or(())?;
    let read_only = matches!(get(entry, "read_only"), Some(Yaml::Boolean(true)));
    Ok(Some(RawMount {
        target,
        source: text("source")?,
        kind,
        read_only,
    }))
}

fn mount_entry(entry: &Yaml, vars: &Variables) -> Result<Option<RawMount>, ()> {
    match entry {
        Yaml::Hash(hash) => long_mount(hash, vars),
        other => {
            let spec = scalar(other).ok_or(())?;
            let spec = vars.interpolate(&spec).map_err(|_| ())?;
            short_mount(&spec).map(Some).ok_or(())
        }
    }
}

fn parse_mounts(
    value: Option<&Yaml>,
    vars: &Variables,
    unknown: &mut BTreeSet<ComposeField>,
) -> Vec<RawMount> {
    let Some(Yaml::Array(items)) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in items {
        match mount_entry(entry, vars) {
            Ok(mount) => out.extend(mount),
            Err(()) => {
                unknown.insert(ComposeField::Volumes);
            }
        }
    }
    out
}

/// Network keys from the list or the mapping form.
fn parse_network_keys(value: Option<&Yaml>) -> BTreeSet<String> {
    match value {
        Some(Yaml::Array(items)) => items.iter().filter_map(scalar).collect(),
        Some(Yaml::Hash(hash)) => hash.keys().filter_map(scalar).collect(),
        _ => BTreeSet::new(),
    }
}

/// Environment keys, and whether the file gives each a value.
///
/// Only keys are taken: `KEY: value` and `KEY=value` contribute their key
/// and the fact that a value exists, nothing else. Values are not
/// interpolated, copied or kept.
fn parse_env_keys(value: Option<&Yaml>) -> BTreeMap<String, bool> {
    match value {
        Some(Yaml::Hash(hash)) => hash
            .iter()
            .filter_map(|(k, v)| Some((scalar(k)?, !matches!(v, Yaml::Null))))
            .collect(),
        Some(Yaml::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Yaml::String(entry) => match entry.split_once('=') {
                    Some((key, _)) => Some((key.to_string(), true)),
                    None => Some((entry.clone(), false)),
                },
                _ => None,
            })
            .collect(),
        _ => BTreeMap::new(),
    }
}

fn parse_service(name: &str, value: &Yaml, vars: &Variables) -> Result<ServiceLayer, String> {
    let hash = mapping(value).unwrap_or_default();
    if get(&hash, "extends").is_some() {
        return Err(format!(
            "service {name} uses extends, which is not supported"
        ));
    }
    let mut unknown = BTreeSet::new();
    let image = interpolated(get(&hash, "image"), vars, ComposeField::Image, &mut unknown);
    let restart = interpolated(
        get(&hash, "restart"),
        vars,
        ComposeField::Restart,
        &mut unknown,
    );
    let network_mode = interpolated(
        get(&hash, "network_mode"),
        vars,
        ComposeField::NetworkMode,
        &mut unknown,
    );
    let ports = parse_ports(get(&hash, "ports"), vars, &mut unknown);
    let mounts = parse_mounts(get(&hash, "volumes"), vars, &mut unknown);
    Ok(ServiceLayer {
        image,
        restart,
        network_mode,
        ports,
        mounts,
        networks: parse_network_keys(get(&hash, "networks")),
        env: parse_env_keys(get(&hash, "environment")),
        env_file: get(&hash, "env_file").is_some(),
        unknown,
    })
}

fn parse_resources(value: Option<&Yaml>, vars: &Variables) -> BTreeMap<String, Resource> {
    let Some(Yaml::Hash(hash)) = value else {
        return BTreeMap::new();
    };
    hash.iter()
        .filter_map(|(k, v)| {
            let decl = mapping(v).unwrap_or_default();
            let name = get(&decl, "name")
                .and_then(scalar)
                .and_then(|n| vars.interpolate(&n).ok());
            let external = matches!(get(&decl, "external"), Some(Yaml::Boolean(true)));
            Some((scalar(k)?, Resource { name, external }))
        })
        .collect()
}

/// Parse one file's text.
fn parse_layer(path: &str, text: &str, vars: &Variables) -> Result<Layer, String> {
    let docs =
        YamlLoader::load_from_str(text).map_err(|e| format!("invalid YAML in {path}: {e}"))?;
    let root = match docs.first() {
        None | Some(Yaml::Null) => return Ok(Layer::default()),
        Some(doc) => mapping(doc).ok_or_else(|| format!("{path} is not a compose mapping"))?,
    };
    if get(&root, "include").is_some() {
        return Err(format!("{path} uses include, which is not supported"));
    }
    let mut layer = Layer {
        volumes: parse_resources(get(&root, "volumes"), vars),
        networks: parse_resources(get(&root, "networks"), vars),
        ..Default::default()
    };
    if let Some(Yaml::Hash(services)) = get(&root, "services") {
        for (name, value) in services {
            let name = scalar(name).ok_or_else(|| format!("{path}: invalid service name"))?;
            let service = parse_service(&name, value, vars)?;
            layer.services.insert(name, service);
        }
    }
    Ok(layer)
}

/// Take a single value from the override, which also makes it known again.
fn override_scalar(
    base: &mut Option<String>,
    over: Option<String>,
    field: ComposeField,
    unknown: &mut BTreeSet<ComposeField>,
) {
    if over.is_some() {
        *base = over;
        unknown.remove(&field);
    }
}

impl ServiceLayer {
    fn merge(&mut self, over: ServiceLayer) {
        override_scalar(
            &mut self.image,
            over.image,
            ComposeField::Image,
            &mut self.unknown,
        );
        override_scalar(
            &mut self.restart,
            over.restart,
            ComposeField::Restart,
            &mut self.unknown,
        );
        override_scalar(
            &mut self.network_mode,
            over.network_mode,
            ComposeField::NetworkMode,
            &mut self.unknown,
        );
        self.ports.extend(over.ports);
        for mount in over.mounts {
            self.mounts.retain(|m| m.target != mount.target);
            self.mounts.push(mount);
        }
        self.networks.extend(over.networks);
        self.env.extend(over.env);
        self.env_file |= over.env_file;
        self.unknown.extend(over.unknown);
    }
}

impl Layer {
    fn merge(&mut self, over: Layer) {
        for (name, service) in over.services {
            match self.services.get_mut(&name) {
                Some(base) => base.merge(service),
                None => {
                    self.services.insert(name, service);
                }
            }
        }
        merge_resources(&mut self.volumes, over.volumes);
        merge_resources(&mut self.networks, over.networks);
    }
}

fn merge_resources(base: &mut BTreeMap<String, Resource>, over: BTreeMap<String, Resource>) {
    for (key, resource) in over {
        let entry = base.entry(key).or_default();
        entry.name = resource.name.or(entry.name.take());
        entry.external |= resource.external;
    }
}

/// The real name of a volume or network: its `name:`, its key when it is
/// external, `project_key` otherwise.
fn resource_name(key: &str, resources: &BTreeMap<String, Resource>, project: &str) -> String {
    match resources.get(key) {
        Some(Resource {
            name: Some(name), ..
        }) => name.clone(),
        Some(Resource { external: true, .. }) => key.to_string(),
        _ => format!("{project}_{key}"),
    }
}

fn is_path(source: &str) -> bool {
    source.starts_with('/') || source.starts_with('.') || source.starts_with('~')
}

fn resolve_mount(raw: RawMount, layer: &Layer, ctx: &ProjectContext) -> Mount {
    let kind = match (&raw.source, raw.kind) {
        (None, _) => MountKind::Anonymous,
        (Some(_), Some(kind)) => kind,
        (Some(source), None) if is_path(source) => MountKind::Bind,
        (Some(_), None) => MountKind::Volume,
    };
    let source = match (kind, raw.source) {
        // `~` is the home of whoever ran compose: not knowable
        (MountKind::Bind, Some(path)) if path.starts_with('~') => None,
        (MountKind::Bind, Some(path)) => Some(resolve_path(ctx.working_dir, &path)),
        (MountKind::Volume, Some(name)) => Some(resource_name(&name, &layer.volumes, ctx.name)),
        _ => None,
    };
    Mount {
        target: raw.target,
        kind,
        source,
        read_only: raw.read_only,
    }
}

fn resolve_service(service: ServiceLayer, layer: &Layer, ctx: &ProjectContext) -> DeclaredService {
    let mut networks = service.networks;
    if networks.is_empty() && service.network_mode.is_none() {
        networks.insert("default".to_string());
    }
    let mut mounts: Vec<Mount> = service
        .mounts
        .into_iter()
        .map(|raw| resolve_mount(raw, layer, ctx))
        .collect();
    mounts.sort();
    DeclaredService {
        image: service.image,
        ports: service.ports.into_iter().collect(),
        mounts,
        restart: normalize_restart(service.restart.as_deref().unwrap_or("no")),
        network_mode: service.network_mode,
        networks: networks
            .iter()
            .map(|key| resource_name(key, &layer.networks, ctx.name))
            .collect(),
        env: service.env,
        env_file: service.env_file,
        unknown: service.unknown,
    }
}

/// Parse and merge `files` (`(path, text)`, in label order).
pub fn parse_project(
    files: &[(String, String)],
    ctx: &ProjectContext,
) -> Result<DeclaredProject, String> {
    let mut merged = Layer::default();
    for (path, text) in files {
        merged.merge(parse_layer(path, text, ctx.vars)?);
    }
    let services = std::mem::take(&mut merged.services);
    Ok(DeclaredProject {
        services: services
            .into_iter()
            .map(|(name, service)| (name, resolve_service(service, &merged, ctx)))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_with(files: &[&str], vars: &Variables) -> Result<DeclaredProject, String> {
        let files: Vec<(String, String)> = files
            .iter()
            .enumerate()
            .map(|(i, text)| (format!("compose-{i}.yml"), text.to_string()))
            .collect();
        let ctx = ProjectContext {
            name: "shop",
            working_dir: Path::new("/srv/shop"),
            vars,
        };
        parse_project(&files, &ctx)
    }

    fn web(files: &[&str]) -> DeclaredService {
        parse_with(files, &Variables::default()).unwrap().services["web"].clone()
    }

    #[test]
    fn test_short_and_long_ports() {
        let service = web(&[r#"
services:
  web:
    image: nginx:1.25
    ports:
      - "127.0.0.1:8080:80"
      - 8443:443
      - "53:53/udp"
      - "9000"
      - "[::1]:9100:9100"
      - "127.0.0.1::7000"
      - "6000-6001:6000-6001"
      - target: 5432
        published: "15432"
        host_ip: 192.0.2.10
"#]);
        let mut bindings: Vec<String> = service
            .ports
            .iter()
            .map(|p| format!("{} {}", p.container, p.binding()))
            .collect();
        bindings.sort();
        assert_eq!(
            bindings,
            vec![
                "443/tcp 0.0.0.0:8443",
                "53/udp 0.0.0.0:53",
                "5432/tcp 192.0.2.10:15432",
                "6000/tcp 0.0.0.0:6000",
                "6001/tcp 0.0.0.0:6001",
                "7000/tcp 127.0.0.1:random",
                "80/tcp 127.0.0.1:8080",
                "9000/tcp 0.0.0.0:random",
                "9100/tcp ::1:9100",
            ]
        );
        assert!(service.unknown.is_empty());
    }

    #[test]
    fn test_unparsable_port_makes_the_field_unknown() {
        let service = web(&["services:\n  web:\n    ports: [\"8000-8010:80\"]\n"]);
        assert!(service.unknown.contains(&ComposeField::Ports));
    }

    #[test]
    fn test_volumes_resolve_paths_and_names() {
        let service = web(&[r#"
services:
  web:
    volumes:
      - ./conf:/etc/app:ro
      - data:/var/lib/app
      - shared:/shared
      - /cache
      - ~/keys:/keys:ro
      - type: bind
        source: /srv/logs
        target: /logs
        read_only: true
      - type: tmpfs
        target: /run
volumes:
  data:
  shared:
    external: true
"#]);
        let described: Vec<(String, String)> = service
            .mounts
            .iter()
            .map(|m| (m.target.clone(), m.describe()))
            .collect();
        assert_eq!(
            described,
            vec![
                ("/cache".into(), "anonymous volume".into()),
                ("/etc/app".into(), "/srv/shop/conf:ro".into()),
                ("/keys".into(), "(path not resolvable):ro".into()),
                ("/logs".into(), "/srv/logs:ro".into()),
                ("/shared".into(), "volume shared".into()),
                ("/var/lib/app".into(), "volume shop_data".into()),
            ]
        );
    }

    #[test]
    fn test_override_merges_in_label_order() {
        let base = r#"
services:
  web:
    image: shop/web:1.4.0
    restart: always
    ports: ["127.0.0.1:8080:80"]
    volumes: ["./conf:/etc/app:ro"]
    environment:
      LOG_LEVEL: info
"#;
        let over = r#"
services:
  web:
    image: shop/web:1.5.2
    ports: ["127.0.0.1:9090:9090"]
    volumes: ["./conf-prod:/etc/app"]
    environment:
      - EXTRA=1
  worker:
    image: shop/worker:1
"#;
        let project = parse_with(&[base, over], &Variables::default()).unwrap();
        let web = &project.services["web"];
        assert_eq!(web.image.as_deref(), Some("shop/web:1.5.2"));
        assert_eq!(web.restart, "always");
        assert_eq!(web.ports.len(), 2, "ports are appended");
        assert_eq!(web.mounts.len(), 1, "volumes are replaced by target");
        assert_eq!(web.mounts[0].source.as_deref(), Some("/srv/shop/conf-prod"));
        assert!(!web.mounts[0].read_only);
        assert_eq!(
            web.env.keys().collect::<Vec<_>>(),
            vec!["EXTRA", "LOG_LEVEL"]
        );
        assert!(project.services.contains_key("worker"));
    }

    #[test]
    fn test_interpolation_from_env_file() {
        let vars = Variables::parse_env_file("TAG=1.5.2\nBIND=127.0.0.1\nPORT=\nRESTART=\n");
        let project = parse_with(
            &[r#"
services:
  web:
    image: "shop/web:${TAG}"
    ports: ["${BIND}:${PORT:-8080}:80"]
    restart: ${RESTART:-unless-stopped}
"#],
            &vars,
        )
        .unwrap();
        let web = &project.services["web"];
        assert_eq!(web.image.as_deref(), Some("shop/web:1.5.2"));
        assert_eq!(web.ports.iter().next().unwrap().binding(), "127.0.0.1:8080");
        assert_eq!(web.restart, "unless-stopped");
        assert!(web.unknown.is_empty());
    }

    #[test]
    fn test_default_of_an_undefined_variable_leaves_its_field_unknown() {
        // A CI job that exports PORT and IMAGE before `compose up` runs
        // something else than the defaults: they say nothing about the container
        let project = parse_with(
            &[r#"
services:
  web:
    image: "${IMAGE:-shop/web:local}"
    ports: ["127.0.0.1:${PORT:-8080}:80"]
    restart: always
"#],
            &Variables::default(),
        )
        .unwrap();
        let web = &project.services["web"];
        assert!(web.unknown.contains(&ComposeField::Image));
        assert!(web.unknown.contains(&ComposeField::Ports));
        assert_eq!(web.restart, "always");
    }

    #[test]
    fn test_unresolved_variable_marks_only_its_field() {
        let service =
            web(&["services:\n  web:\n    image: shop/web:${TAG}\n    restart: always\n"]);
        assert!(service.unknown.contains(&ComposeField::Image));
        assert_eq!(service.image, None);
        assert_eq!(service.restart, "always");
        // A later file that sets it makes it known again
        let service = web(&[
            "services:\n  web:\n    image: shop/web:${TAG}\n",
            "services:\n  web:\n    image: shop/web:2\n",
        ]);
        assert!(service.unknown.is_empty());
        assert_eq!(service.image.as_deref(), Some("shop/web:2"));
    }

    #[test]
    fn test_environment_keeps_keys_only() {
        let service = web(&[r#"
services:
  web:
    environment:
      DB_PASSWORD: hunter2
      PASSTHROUGH:
"#]);
        assert_eq!(
            service.env,
            BTreeMap::from([("DB_PASSWORD".into(), true), ("PASSTHROUGH".into(), false)])
        );
        assert!(!format!("{service:?}").contains("hunter2"));
        let listed = web(&["services:\n  web:\n    environment: [\"TOKEN=abc\", \"FROM_SHELL\"]\n    env_file: app.env\n"]);
        assert_eq!(
            listed.env,
            BTreeMap::from([("FROM_SHELL".into(), false), ("TOKEN".into(), true)])
        );
        assert!(listed.env_file);
        assert!(!format!("{listed:?}").contains("abc"));
    }

    #[test]
    fn test_networks_default_names_and_mode() {
        let service = web(&["services:\n  web:\n    image: x\n"]);
        assert_eq!(service.networks, BTreeSet::from(["shop_default".into()]));
        let service = web(&[r#"
services:
  web:
    networks: [front, proxy]
networks:
  front:
  proxy:
    name: edge-proxy
"#]);
        assert_eq!(
            service.networks,
            BTreeSet::from(["edge-proxy".into(), "shop_front".into()])
        );
        let service = web(&["services:\n  web:\n    network_mode: host\n"]);
        assert!(service.networks.is_empty());
        assert_eq!(service.network_mode.as_deref(), Some("host"));
    }

    #[test]
    fn test_yaml_merge_keys() {
        let service = web(&[r#"
x-common: &common
  restart: always
  image: shop/web:1
services:
  web:
    <<: *common
    image: shop/web:2
"#]);
        assert_eq!(service.restart, "always");
        assert_eq!(service.image.as_deref(), Some("shop/web:2"));
    }

    #[test]
    fn test_unsupported_and_invalid_files_are_errors() {
        let vars = Variables::default();
        assert!(parse_with(&["services: [unclosed"], &vars)
            .unwrap_err()
            .contains("invalid YAML"));
        assert!(parse_with(&["include: [other.yml]\n"], &vars)
            .unwrap_err()
            .contains("include"));
        assert!(
            parse_with(&["services:\n  web:\n    extends: base\n"], &vars)
                .unwrap_err()
                .contains("extends")
        );
        assert!(parse_with(&["- a list"], &vars).is_err());
        // An empty file is an empty project, not an error
        assert!(parse_with(&[""], &vars).unwrap().services.is_empty());
    }
}
