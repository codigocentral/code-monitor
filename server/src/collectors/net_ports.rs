//! Listening socket collector
//!
//! Reports which services listen on which interfaces. On a host with a public
//! address, a database bound to `0.0.0.0` is an open door that nobody notices,
//! because the service works exactly the same either way.
//!
//! Reads `/proc/net/tcp` directly rather than shelling out to `ss` or
//! `netstat`, neither of which is guaranteed to exist on a minimal image — and
//! a monitoring agent that silently reports nothing because a tool is missing
//! is worse than one that reports nothing at all.

use anyhow::Result;
use shared::types::{BindScope, ListeningPortInfo};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use tracing::debug;

/// `TCP_LISTEN` as printed in the `st` column
const TCP_LISTEN: &str = "0A";

/// Collector for sockets in the listening state
pub struct NetPortsCollector {
    /// Root to read `net/tcp` and process tables from, overridable for testing
    proc_root: String,
}

/// A socket as the kernel reports it, before the owning process is resolved
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSocket {
    pub address: IpAddr,
    pub port: u16,
    pub inode: u64,
}

/// Decode the address half of a `/proc/net/tcp` line.
///
/// The kernel prints the address as native words, so the bytes come out in the
/// host's order — little-endian on every platform this agent targets. Decoding
/// through `to_le_bytes` keeps that explicit rather than hidden in a swap.
fn parse_address_hex(hex: &str) -> Option<IpAddr> {
    match hex.len() {
        8 => {
            let raw = u32::from_str_radix(hex, 16).ok()?;
            Some(IpAddr::V4(Ipv4Addr::from(raw.to_le_bytes())))
        }
        32 => {
            let mut bytes = [0u8; 16];
            for (group, chunk) in bytes.chunks_mut(4).enumerate() {
                let start = group * 8;
                let raw = u32::from_str_radix(&hex[start..start + 8], 16).ok()?;
                chunk.copy_from_slice(&raw.to_le_bytes());
            }
            Some(IpAddr::V6(Ipv6Addr::from(bytes)))
        }
        _ => None,
    }
}

/// Parse one `/proc/net/tcp` row, keeping only sockets in the listening state.
///
/// Returns `None` for the header, malformed rows, and every established or
/// closing connection — those are traffic, not exposure.
pub fn parse_socket_line(line: &str) -> Option<RawSocket> {
    let mut fields = line.split_whitespace();

    // Column 0 is the slot number, "12:", which the header does not have
    let slot = fields.next()?;
    if !slot.ends_with(':') {
        return None;
    }

    let local = fields.next()?;
    let _remote = fields.next()?;
    let state = fields.next()?;

    if state != TCP_LISTEN {
        return None;
    }

    let (address_hex, port_hex) = local.split_once(':')?;
    let address = parse_address_hex(address_hex)?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;

    // tx_queue:rx_queue, tr:tm->when, retrnsmt, uid, timeout, then the inode
    let inode = fields.nth(5).and_then(|i| i.parse::<u64>().ok())?;

    Some(RawSocket {
        address,
        port,
        inode,
    })
}

/// Parse a whole `/proc/net/tcp` table.
pub fn parse_socket_table(contents: &str) -> Vec<RawSocket> {
    contents.lines().filter_map(parse_socket_line).collect()
}

impl NetPortsCollector {
    pub fn new() -> Self {
        Self {
            proc_root: "/proc".to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn with_proc_root(proc_root: &str) -> Self {
        Self {
            proc_root: proc_root.to_string(),
        }
    }

    /// Collect listening sockets.
    ///
    /// Never errors: a host without `/proc` reports nothing rather than failing
    /// the RPC.
    pub async fn collect(&self) -> Result<Vec<ListeningPortInfo>> {
        let root = Path::new(&self.proc_root);
        let owners = self.socket_owners();

        let mut ports = Vec::new();
        for (file, protocol) in [("net/tcp", "tcp"), ("net/tcp6", "tcp6")] {
            let path = root.join(file);
            let contents = match std::fs::read_to_string(&path) {
                Ok(contents) => contents,
                Err(e) => {
                    debug!("Cannot read {}: {}", path.display(), e);
                    continue;
                }
            };

            for socket in parse_socket_table(&contents) {
                let (pid, process_name) = owners
                    .get(&socket.inode)
                    .map(|(pid, name)| (Some(*pid), name.clone()))
                    .unwrap_or((None, String::new()));

                ports.push(ListeningPortInfo {
                    address: socket.address.to_string(),
                    port: socket.port,
                    protocol: protocol.to_string(),
                    bind_scope: BindScope::classify(&socket.address.to_string()),
                    pid,
                    process_name,
                    sensitive: ListeningPortInfo::is_sensitive_port(socket.port),
                });
            }
        }

        // Exposed data stores first, then by port: the findings lead the list
        ports.sort_by_key(|p| {
            (
                !p.is_exposed_datastore(),
                !p.bind_scope.is_exposed(),
                p.port,
            )
        });
        Ok(ports)
    }

    /// Map socket inodes to the process holding them.
    ///
    /// Walks `/proc/[pid]/fd`, where a socket appears as a link to
    /// `socket:[inode]`. Processes owned by other users are unreadable without
    /// privileges, so their sockets are still reported — just without a name.
    /// An exposed port matters whether or not we can say who opened it.
    fn socket_owners(&self) -> HashMap<u64, (u32, String)> {
        let mut owners = HashMap::new();

        let entries = match std::fs::read_dir(&self.proc_root) {
            Ok(entries) => entries,
            Err(e) => {
                debug!("Cannot enumerate {}: {}", self.proc_root, e);
                return owners;
            }
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let pid: u32 = match entry.file_name().to_string_lossy().parse() {
                Ok(pid) => pid,
                Err(_) => continue, // Not a process directory
            };

            let fd_dir = entry.path().join("fd");
            let fds = match std::fs::read_dir(&fd_dir) {
                Ok(fds) => fds,
                Err(_) => continue, // Another user's process
            };

            let name = std::fs::read_to_string(entry.path().join("comm"))
                .map(|c| c.trim().to_string())
                .unwrap_or_default();

            for fd in fds.filter_map(|f| f.ok()) {
                if let Ok(target) = std::fs::read_link(fd.path()) {
                    if let Some(inode) = parse_socket_link(&target.to_string_lossy()) {
                        owners.insert(inode, (pid, name.clone()));
                    }
                }
            }
        }

        owners
    }
}

/// Extract the inode from a `socket:[12345]` symlink target.
fn parse_socket_link(target: &str) -> Option<u64> {
    target
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

impl Default for NetPortsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real rows copied from a running host
    const TCP_TABLE: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 18050 1 0000000000df2018 100 0 0 10 0
   1: 0100007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22102 1 00000000e7266a6f 100 0 0 10 0
   2: 0100007F:1F90 7F000001:C1B4 01 00000000:00000000 00:00000000 00000000  1000        0 31337 1 0000000000000000 100 0 0 10 0
";

    #[test]
    fn test_parse_ipv4_wildcard() {
        assert_eq!(
            parse_address_hex("00000000"),
            Some(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)))
        );
    }

    #[test]
    fn test_parse_ipv4_loopback() {
        // 0100007F is 127.0.0.1 read as a native word
        assert_eq!(
            parse_address_hex("0100007F"),
            Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
        );
    }

    #[test]
    fn test_parse_ipv4_private_address() {
        // 10.0.0.9, a private VPN range
        assert_eq!(
            parse_address_hex("0900000A"),
            Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9)))
        );
    }

    #[test]
    fn test_parse_ipv6_wildcard() {
        assert_eq!(
            parse_address_hex("00000000000000000000000000000000"),
            Some(IpAddr::V6(Ipv6Addr::UNSPECIFIED))
        );
    }

    #[test]
    fn test_parse_ipv6_loopback() {
        assert_eq!(
            parse_address_hex("00000000000000000000000001000000"),
            Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
        );
    }

    #[test]
    fn test_parse_address_rejects_wrong_length() {
        assert!(parse_address_hex("").is_none());
        assert!(parse_address_hex("0100").is_none());
        assert!(parse_address_hex("zzzzzzzz").is_none());
    }

    // ─────────────────────────────────────────
    // Table parsing
    // ─────────────────────────────────────────

    #[test]
    fn test_parse_table_keeps_only_listening_sockets() {
        let sockets = parse_socket_table(TCP_TABLE);

        assert_eq!(
            sockets.len(),
            2,
            "the established connection must not be reported as exposure"
        );
    }

    #[test]
    fn test_parse_table_decodes_address_and_port() {
        let sockets = parse_socket_table(TCP_TABLE);

        assert_eq!(sockets[0].address.to_string(), "0.0.0.0");
        assert_eq!(sockets[0].port, 5432);
        assert_eq!(sockets[0].inode, 18050);

        assert_eq!(sockets[1].address.to_string(), "127.0.0.1");
        assert_eq!(sockets[1].port, 53);
    }

    #[test]
    fn test_parse_table_skips_the_header() {
        let header = TCP_TABLE.lines().next().unwrap();
        assert!(parse_socket_line(header).is_none());
    }

    #[test]
    fn test_parse_table_skips_malformed_rows() {
        assert!(parse_socket_line("   3: garbage").is_none());
        assert!(parse_socket_line("").is_none());
        assert!(parse_socket_line("not a row at all").is_none());
    }

    #[test]
    fn test_parse_empty_table() {
        assert!(parse_socket_table("").is_empty());
    }

    // ─────────────────────────────────────────
    // Socket links
    // ─────────────────────────────────────────

    #[test]
    fn test_parse_socket_link() {
        assert_eq!(parse_socket_link("socket:[18050]"), Some(18050));
    }

    #[test]
    fn test_parse_socket_link_ignores_other_descriptors() {
        assert!(parse_socket_link("/dev/null").is_none());
        assert!(parse_socket_link("pipe:[1234]").is_none());
        assert!(parse_socket_link("socket:[notanumber]").is_none());
    }

    // ─────────────────────────────────────────
    // Collection
    // ─────────────────────────────────────────

    #[tokio::test]
    async fn test_collect_missing_proc_is_not_an_error() {
        let collector = NetPortsCollector::with_proc_root("/nonexistent/proc");
        assert!(collector.collect().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_collect_reads_a_synthetic_proc() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("net")).unwrap();
        std::fs::write(root.path().join("net/tcp"), TCP_TABLE).unwrap();

        let collector = NetPortsCollector::with_proc_root(&root.path().to_string_lossy());
        let ports = collector.collect().await.unwrap();

        assert_eq!(ports.len(), 2);
        let exposed = ports.iter().find(|p| p.port == 5432).unwrap();
        assert_eq!(exposed.bind_scope, BindScope::AllInterfaces);
        assert!(exposed.sensitive, "5432 is a database port");
        assert!(
            exposed.is_exposed_datastore(),
            "postgres on 0.0.0.0 is the finding the audit was after"
        );
    }

    #[tokio::test]
    async fn test_collect_classifies_loopback_as_safe() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("net")).unwrap();
        std::fs::write(root.path().join("net/tcp"), TCP_TABLE).unwrap();

        let ports = NetPortsCollector::with_proc_root(&root.path().to_string_lossy())
            .collect()
            .await
            .unwrap();

        let dns = ports.iter().find(|p| p.port == 53).unwrap();
        assert_eq!(dns.bind_scope, BindScope::Loopback);
        assert!(!dns.is_exposed_datastore());
    }

    #[tokio::test]
    async fn test_collect_puts_findings_first() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:0035 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 100 1 0 100 0 0 10 0
   1: 00000000:1538 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 101 1 0 100 0 0 10 0
";
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("net")).unwrap();
        std::fs::write(root.path().join("net/tcp"), table).unwrap();

        let ports = NetPortsCollector::with_proc_root(&root.path().to_string_lossy())
            .collect()
            .await
            .unwrap();

        assert_eq!(
            ports[0].port, 5432,
            "the exposed database must lead the list, not the loopback DNS resolver"
        );
    }

    #[tokio::test]
    async fn test_collect_reads_both_address_families() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("net")).unwrap();
        std::fs::write(root.path().join("net/tcp"), TCP_TABLE).unwrap();
        std::fs::write(
            root.path().join("net/tcp6"),
            "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000   974        0 5555 1 0 100 0 0 10 5
",
        )
        .unwrap();

        let ports = NetPortsCollector::with_proc_root(&root.path().to_string_lossy())
            .collect()
            .await
            .unwrap();

        assert_eq!(ports.len(), 3);
        assert!(ports.iter().any(|p| p.protocol == "tcp6" && p.port == 8080));
    }
}
