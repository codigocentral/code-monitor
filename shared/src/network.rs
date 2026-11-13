//! Network state as systemd-networkd reports it
//!
//! `systemd-networkd-wait-online.service` fails at boot whenever a link is
//! still "configuring" when its timeout runs out. On providers that keep
//! sending IPv6 router advertisements the main interface can sit in
//! `routable (configuring)` forever while the host is perfectly online, so
//! the unit stays `failed` from boot until the next reboot. Whether that
//! failure still means anything depends on the network *now*, which is what
//! this module reads.

use serde::{Deserialize, Serialize};

/// One row of `networkctl list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkLink {
    pub name: String,
    /// `ether`, `loopback`, `wireguard`, `bridge`, ...
    pub kind: String,
    /// `routable`, `degraded`, `carrier`, `off`, ...
    pub operational: String,
    /// `configured`, `configuring`, `unmanaged`, `failed`, ...
    pub setup: String,
}

/// What `networkctl` says about the host right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkState {
    /// `Online state:` from `networkctl status` (systemd 249+), when present
    pub online_state: Option<String>,
    pub links: Vec<NetworkLink>,
}

/// Link kinds that carry the host's own uplink, as opposed to virtual ones
/// that are routable as soon as they exist.
const UPLINK_KINDS: &[&str] = &["ether", "wlan", "wwan", "bond", "vlan"];

impl NetworkState {
    /// Whether the host has working connectivity now.
    ///
    /// The `Online state` line is networkd's own verdict and wins when it is
    /// there; older versions only list links, so an uplink that is routable
    /// stands in for it.
    pub fn is_online(&self) -> bool {
        match self.online_state.as_deref() {
            Some(state) => matches!(state, "online" | "partial"),
            None => self
                .links
                .iter()
                .any(|l| UPLINK_KINDS.contains(&l.kind.as_str()) && l.operational == "routable"),
        }
    }

    /// Links still in `configuring`: the usual reason wait-online timed out.
    pub fn configuring_links(&self) -> Vec<&str> {
        self.links
            .iter()
            .filter(|l| l.setup == "configuring")
            .map(|l| l.name.as_str())
            .collect()
    }

    /// One line for humans: `online (eth0 routable, configuring)`.
    pub fn summary(&self) -> String {
        let state = self.online_state.as_deref().unwrap_or("unknown");
        let uplinks: Vec<String> = self
            .links
            .iter()
            .filter(|l| UPLINK_KINDS.contains(&l.kind.as_str()))
            .map(|l| format!("{} {} ({})", l.name, l.operational, l.setup))
            .collect();
        if uplinks.is_empty() {
            state.to_string()
        } else {
            format!("{state}; {}", uplinks.join(", "))
        }
    }
}

/// Whether a unit is networkd's boot-time wait for connectivity.
pub fn is_wait_online_unit(name: &str) -> bool {
    name == "systemd-networkd-wait-online.service"
        || name.starts_with("systemd-networkd-wait-online@")
}

/// Parse `networkctl list`, with or without the header and footer.
pub fn parse_networkctl_list(text: &str) -> Vec<NetworkLink> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // IDX LINK TYPE OPERATIONAL SETUP
            if fields.len() < 5 || fields[0].parse::<u32>().is_err() {
                return None;
            }
            Some(NetworkLink {
                name: fields[1].to_string(),
                kind: fields[2].to_string(),
                operational: fields[3].to_string(),
                setup: fields[4].to_string(),
            })
        })
        .collect()
}

/// The `Online state:` value of `networkctl status`.
pub fn parse_online_state(status: &str) -> Option<String> {
    status.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Online state:")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host whose uplink never leaves `configuring` (provider keeps sending
    /// router advertisements) while it is fully online.
    const LIST: &str = " IDX LINK            TYPE      OPERATIONAL SETUP
   1 lo              loopback  carrier     unmanaged
   2 eth0            ether     routable    configuring
   3 wg0             wireguard routable    unmanaged
   4 br-87a018dbf9a6 bridge    routable    unmanaged

4 links listed.
";

    const STATUS: &str = "● Interfaces: 1, 2, 3, 4
       State: routable
Online state: online
     Address: 192.0.2.10 on eth0
";

    #[test]
    fn parses_links_and_online_state() {
        let links = parse_networkctl_list(LIST);
        assert_eq!(links.len(), 4);
        assert_eq!(
            links[1],
            NetworkLink {
                name: "eth0".into(),
                kind: "ether".into(),
                operational: "routable".into(),
                setup: "configuring".into(),
            }
        );
        assert_eq!(parse_online_state(STATUS).as_deref(), Some("online"));
        assert_eq!(parse_online_state("State: routable\n"), None);
    }

    #[test]
    fn online_state_wins_over_links() {
        let state = NetworkState {
            online_state: parse_online_state(STATUS),
            links: parse_networkctl_list(LIST),
        };
        assert!(state.is_online());
        assert_eq!(state.configuring_links(), vec!["eth0"]);
        assert_eq!(state.summary(), "online; eth0 routable (configuring)");

        let offline = NetworkState {
            online_state: Some("offline".into()),
            links: state.links.clone(),
        };
        assert!(!offline.is_online());
    }

    #[test]
    fn without_online_state_a_routable_uplink_counts() {
        let mut links = parse_networkctl_list(LIST);
        let state = NetworkState {
            online_state: None,
            links: links.clone(),
        };
        assert!(state.is_online());
        // Bridges and tunnels are routable on their own; they prove nothing
        links[1].operational = "degraded".into();
        let virtual_only = NetworkState {
            online_state: None,
            links,
        };
        assert!(!virtual_only.is_online());
        assert_eq!(NetworkState::default().summary(), "unknown");
    }

    #[test]
    fn recognises_wait_online_units() {
        assert!(is_wait_online_unit("systemd-networkd-wait-online.service"));
        assert!(is_wait_online_unit(
            "systemd-networkd-wait-online@eth0.service"
        ));
        assert!(!is_wait_online_unit("networking.service"));
        assert!(!is_wait_online_unit("NetworkManager-wait-online.service"));
    }
}
