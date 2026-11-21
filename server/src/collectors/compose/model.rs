//! Normalised published ports and mounts, as both sides are compared

/// One published port: `host_ip:host_port -> container`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Port {
    /// `0.0.0.0` when no address was given: Docker binds every interface
    pub host_ip: String,
    /// Empty for an ephemeral port chosen by Docker
    pub host_port: String,
    /// `80/tcp`
    pub container: String,
}

impl Port {
    pub fn new(host_ip: &str, host_port: &str, container_port: u32, protocol: &str) -> Self {
        let host_ip = host_ip.trim_start_matches('[').trim_end_matches(']');
        Self {
            host_ip: normalize_host_ip(host_ip),
            host_port: host_port.to_string(),
            container: format!("{container_port}/{}", protocol.to_ascii_lowercase()),
        }
    }

    /// `127.0.0.1:8080`, or `0.0.0.0:random` for an ephemeral port.
    pub fn binding(&self) -> String {
        let port = if self.host_port.is_empty() {
            "random"
        } else {
            &self.host_port
        };
        format!("{}:{port}", self.host_ip)
    }
}

/// An empty bind address is Docker's "every interface".
pub fn normalize_host_ip(ip: &str) -> String {
    if ip.is_empty() {
        "0.0.0.0".to_string()
    } else {
        ip.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MountKind {
    Bind,
    Volume,
    /// A volume without a name, created for the container (image `VOLUME`)
    Anonymous,
}

/// One mount, keyed by where it appears inside the container.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mount {
    pub target: String,
    pub kind: MountKind,
    /// Host path of a bind, name of a volume. `None` for an anonymous volume,
    /// or a declared bind whose path cannot be known (`~`)
    pub source: Option<String>,
    pub read_only: bool,
}

impl Mount {
    /// Whether a declared mount is satisfied by a running one.
    pub fn satisfied_by(&self, running: &Mount) -> bool {
        let same_source = self.kind == MountKind::Anonymous
            || self.source.is_none()
            || self.source == running.source;
        self.kind == running.kind && self.read_only == running.read_only && same_source
    }

    /// `/srv/app/data:ro`, `volume shop_data`, `anonymous volume`.
    pub fn describe(&self) -> String {
        let source = self.source.as_deref().unwrap_or("(path not resolvable)");
        let base = match self.kind {
            MountKind::Bind => source.to_string(),
            MountKind::Volume => format!("volume {source}"),
            MountKind::Anonymous => "anonymous volume".to_string(),
        };
        if self.read_only {
            format!("{base}:ro")
        } else {
            base
        }
    }
}

/// Whether a volume name is one Docker generated (64 hex characters).
pub fn is_anonymous_volume_name(name: &str) -> bool {
    name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit())
}

/// Resolve `path` against `base` and clean `.` and `..` lexically, as
/// compose does before handing a bind to Docker.
///
/// Compose paths describe Linux containers, so this is POSIX path logic on
/// strings, never `std::path`: the result is the same on any host OS.
pub fn resolve_path(base: &str, path: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{base}/{path}")
    };
    let mut clean: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                clean.pop();
            }
            other => clean.push(other),
        }
    }
    let rest = clean.join("/");
    if joined.starts_with('/') {
        format!("/{rest}")
    } else {
        rest
    }
}

/// The directory part of a POSIX `path`: `/srv/shop` for
/// `/srv/shop/compose.yml`, `/` at the root and empty for a bare file name.
pub fn posix_parent(path: &str) -> &str {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "",
    }
}

/// `no`, `always`, `unless-stopped`, `on-failure` or `on-failure:N`.
pub fn normalize_restart(policy: &str) -> String {
    let policy = policy.trim().to_ascii_lowercase();
    match policy.as_str() {
        "" | "no" | "false" => "no".to_string(),
        "on-failure:0" => "on-failure".to_string(),
        _ => policy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_defaults_to_every_interface() {
        let port = Port::new("", "8080", 80, "TCP");
        assert_eq!(port.host_ip, "0.0.0.0");
        assert_eq!(port.container, "80/tcp");
        assert_eq!(port.binding(), "0.0.0.0:8080");
        assert_eq!(Port::new("[::1]", "", 53, "udp").binding(), "::1:random");
    }

    #[test]
    fn test_resolve_path_cleans_relative_parts() {
        let base = "/srv/shop";
        assert_eq!(resolve_path(base, "./data"), "/srv/shop/data");
        assert_eq!(resolve_path(base, "../shared/conf/"), "/srv/shared/conf");
        assert_eq!(resolve_path(base, "/etc/app"), "/etc/app");
        assert_eq!(
            resolve_path("/srv/shop/", "data//sub/./x"),
            "/srv/shop/data/sub/x"
        );
        assert_eq!(resolve_path("/", "../../etc"), "/etc");
        assert_eq!(resolve_path("/", "/"), "/");
    }

    #[test]
    fn test_resolve_path_is_posix_on_every_host() {
        // A backslash is an ordinary file name character in POSIX, not a
        // separator, whatever OS the agent was built for.
        assert_eq!(resolve_path("/srv", "a\\b"), "/srv/a\\b");
        assert_eq!(resolve_path("relative", "./data"), "relative/data");
    }

    #[test]
    fn test_posix_parent() {
        assert_eq!(posix_parent("/srv/shop/compose.yml"), "/srv/shop");
        assert_eq!(posix_parent("/compose.yml"), "/");
        assert_eq!(posix_parent("compose.yml"), "");
        assert_eq!(posix_parent("/srv/shop/"), "/srv");
    }

    #[test]
    fn test_restart_normalisation() {
        assert_eq!(normalize_restart(""), "no");
        assert_eq!(normalize_restart("No"), "no");
        assert_eq!(normalize_restart("on-failure:0"), "on-failure");
        assert_eq!(normalize_restart("on-failure:3"), "on-failure:3");
        assert_eq!(normalize_restart("unless-stopped"), "unless-stopped");
    }

    #[test]
    fn test_mount_matching() {
        let bind = |source: Option<&str>, ro: bool| Mount {
            target: "/data".into(),
            kind: MountKind::Bind,
            source: source.map(str::to_string),
            read_only: ro,
        };
        let running = bind(Some("/srv/data"), true);
        assert!(bind(Some("/srv/data"), true).satisfied_by(&running));
        assert!(!bind(Some("/srv/data"), false).satisfied_by(&running));
        assert!(!bind(Some("/srv/other"), true).satisfied_by(&running));
        // A path that cannot be resolved only checks kind and mode
        assert!(bind(None, true).satisfied_by(&running));
        assert_eq!(running.describe(), "/srv/data:ro");
    }

    #[test]
    fn test_anonymous_volume_names() {
        assert!(is_anonymous_volume_name(&"ab12".repeat(16)));
        assert!(!is_anonymous_volume_name("shop_data"));
    }
}
