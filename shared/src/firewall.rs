//! Firewall view: would this host accept a new connection from the internet?
//!
//! A data store listening on `0.0.0.0` is only an open door if the packet
//! filter lets the internet reach it. Most hosts run ufw (or plain iptables)
//! with a default-deny policy, so the bind address alone produced a warning on
//! every database that was in fact closed from outside.
//!
//! This module reads the rules as `iptables-save` / `ip6tables-save` print them
//! — both the legacy and the nf_tables backends speak that format, and ufw is
//! a layer on top of it — and answers one question per port: is there any
//! public source address whose new TCP connection the rules would accept?
//!
//! The analysis is deliberately one-sided. Whatever it cannot model (rate
//! limits, ipsets, `recent`, unknown targets) is assumed to possibly match, so
//! an `ACCEPT` it does not understand still counts as accepting and a `DROP` it
//! does not understand does not count as blocking. A port can be reported as
//! exposed when it is not; it is never reported as closed when it is open.
//!
//! Docker publishes ports with `DNAT` in the `nat` table, so a published port
//! never crosses `INPUT` (where ufw lives) and is filtered by `FORWARD`
//! instead (`DOCKER-USER`, then Docker's own chains). Ports that a `DNAT` rule
//! catches are judged on that path.
//!
//! Rules that exist only as native nftables tables (not visible to
//! `iptables-save`) are not read. Those can only drop more, so ignoring them
//! keeps the verdict on the safe side.

use std::collections::HashMap;
use std::net::IpAddr;

/// Address family of a rule set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    V4,
    V6,
}

/// What the firewall does with a new TCP connection from the internet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    /// Some public source address is accepted
    Public,
    /// Every public source is dropped or rejected
    Filtered,
}

/// The packet-filter rules of one address family, as `iptables-save` prints them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ruleset {
    tables: HashMap<String, Table>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Table {
    chains: HashMap<String, Chain>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct Chain {
    /// `ACCEPT` / `DROP` for built-in chains, `None` for user chains
    policy: Option<String>,
    rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
struct Rule {
    matches: Vec<Match>,
    target: Option<Target>,
}

#[derive(Debug, Clone, PartialEq)]
struct Match {
    negated: bool,
    option: String,
    values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Target {
    name: String,
    goto: bool,
    options: Vec<Match>,
}

/// Everything the audit needs to judge exposure on a host.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FirewallSnapshot {
    /// `iptables-save`; `None` when it could not be read
    pub v4: Option<Ruleset>,
    /// `ip6tables-save`; `None` when it could not be read
    pub v6: Option<Ruleset>,
    /// Interfaces holding an IPv4 default route: where the internet comes in
    pub public_ifaces_v4: Vec<String>,
    /// Interfaces holding an IPv6 default route
    pub public_ifaces_v6: Vec<String>,
}

impl FirewallSnapshot {
    /// Whether a socket bound to `address:port` (TCP) is reachable from the
    /// internet, or `None` when the rules needed to tell were not read.
    ///
    /// A socket on `::` is dual-stack on Linux by default, so it is judged
    /// against both families; it is public if either one lets traffic in.
    pub fn exposure(&self, address: &str, port: u16) -> Option<Exposure> {
        let families: &[Family] = match address.parse::<IpAddr>().ok()? {
            IpAddr::V4(_) => &[Family::V4],
            IpAddr::V6(_) => &[Family::V4, Family::V6],
        };
        let mut unknown = false;
        for family in families {
            let (rules, ifaces) = match family {
                Family::V4 => (&self.v4, &self.public_ifaces_v4),
                Family::V6 => (&self.v6, &self.public_ifaces_v6),
            };
            match rules {
                Some(rules) => {
                    if rules.accepts_from_internet(port, ifaces) {
                        return Some(Exposure::Public);
                    }
                }
                None => unknown = true,
            }
        }
        if unknown {
            None
        } else {
            Some(Exposure::Filtered)
        }
    }

    /// Whether every family was read.
    pub fn complete(&self) -> bool {
        self.v4.is_some() && self.v6.is_some()
    }
}

// ─────────────────────────────────────────
// Parsing
// ─────────────────────────────────────────

/// Split a rule line the way iptables-save quotes it: double quotes group,
/// a backslash escapes the next character.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    started = true;
                }
            }
            '"' => {
                quoted = !quoted;
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

fn is_option(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next() == Some('-')
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '-')
}

/// One name per option, whichever spelling iptables-save chose.
fn canonical(option: &str) -> &str {
    match option {
        "--protocol" => "-p",
        "--source" | "--src" => "-s",
        "--destination" | "--dst" => "-d",
        "--in-interface" => "-i",
        "--out-interface" => "-o",
        "--match" => "-m",
        "--destination-port" => "--dport",
        "--source-port" => "--sport",
        "--destination-ports" => "--dports",
        "--source-ports" => "--sports",
        "--state" => "--ctstate",
        other => other,
    }
}

/// Read `option [values...]` pairs, honouring a leading `!`.
fn parse_matches(words: &[String]) -> Vec<Match> {
    let mut matches = Vec::new();
    let mut negated = false;
    let mut i = 0;
    while i < words.len() {
        let word = &words[i];
        if word == "!" {
            negated = true;
            i += 1;
            continue;
        }
        if !is_option(word) {
            i += 1;
            continue;
        }
        let mut values = Vec::new();
        i += 1;
        while i < words.len() && !is_option(&words[i]) && words[i] != "!" {
            values.push(words[i].clone());
            i += 1;
        }
        matches.push(Match {
            negated,
            option: canonical(word).to_string(),
            values,
        });
        negated = false;
    }
    matches
}

fn parse_rule(words: &[String]) -> Rule {
    let jump = words
        .iter()
        .position(|w| matches!(w.as_str(), "-j" | "--jump" | "-g" | "--goto"));
    match jump {
        Some(at) => Rule {
            matches: parse_matches(&words[..at]),
            target: words.get(at + 1).map(|name| Target {
                name: name.clone(),
                goto: matches!(words[at].as_str(), "-g" | "--goto"),
                options: parse_matches(&words[(at + 2).min(words.len())..]),
            }),
        },
        None => Rule {
            matches: parse_matches(words),
            target: None,
        },
    }
}

/// Parse the output of `iptables-save` or `ip6tables-save`.
///
/// Unknown lines are skipped rather than failing the whole rule set.
pub fn parse_iptables_save(text: &str) -> Ruleset {
    let mut ruleset = Ruleset::default();
    let mut current: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('*') {
            ruleset.tables.entry(name.to_string()).or_default();
            current = Some(name.to_string());
            continue;
        }
        if line == "COMMIT" {
            current = None;
            continue;
        }
        let Some(table) = current.as_ref().and_then(|t| ruleset.tables.get_mut(t)) else {
            continue;
        };
        if let Some(declaration) = line.strip_prefix(':') {
            let mut parts = declaration.split_whitespace();
            if let Some(name) = parts.next() {
                let policy = parts.next().filter(|p| *p != "-").map(str::to_string);
                table.chains.entry(name.to_string()).or_default().policy = policy;
            }
            continue;
        }
        let words = split_words(line);
        if words.len() >= 2 && (words[0] == "-A" || words[0] == "--append") {
            let rule = parse_rule(&words[2..]);
            table
                .chains
                .entry(words[1].clone())
                .or_default()
                .rules
                .push(rule);
        }
    }
    ruleset
}

/// Interfaces that hold a default route in `/proc/net/route`.
pub fn parse_ipv4_default_ifaces(proc_net_route: &str) -> Vec<String> {
    let mut ifaces: Vec<String> = proc_net_route
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // Iface Destination Gateway Flags RefCnt Use Metric Mask ...
            let (iface, destination, mask) = (fields.first()?, fields.get(1)?, fields.get(7)?);
            (*destination == "00000000" && *mask == "00000000").then(|| iface.to_string())
        })
        .collect();
    ifaces.sort();
    ifaces.dedup();
    ifaces
}

/// Interfaces that hold a default route in `/proc/net/ipv6_route`.
pub fn parse_ipv6_default_ifaces(proc_net_ipv6_route: &str) -> Vec<String> {
    let zero = "0".repeat(32);
    let mut ifaces: Vec<String> = proc_net_ipv6_route
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // dest prefix src src_prefix next_hop metric refcnt use flags iface
            let (destination, prefix, iface) = (fields.first()?, fields.get(1)?, fields.get(9)?);
            // The kernel keeps an "unreachable" default on lo; that is no way in
            (*destination == zero && *prefix == "00" && *iface != "lo").then(|| iface.to_string())
        })
        .collect();
    ifaces.sort();
    ifaces.dedup();
    ifaces
}

// ─────────────────────────────────────────
// Snapshot files
// ─────────────────────────────────────────

/// Where the `code-monitor-firewall-snapshot` timer writes the rules.
///
/// Reading the packet filter takes `CAP_NET_ADMIN`, which would also let the
/// agent change it. Instead a root oneshot unit runs `iptables-save` /
/// `ip6tables-save` every minute and drops the output here (`root:code-monitor`,
/// `0640`), and the unprivileged agent only reads the files.
pub const DEFAULT_SNAPSHOT_DIR: &str = "/var/lib/code-monitor/firewall";
/// `iptables-save` output inside the snapshot directory
pub const SNAPSHOT_V4_FILE: &str = "iptables.rules";
/// `ip6tables-save` output inside the snapshot directory
pub const SNAPSHOT_V6_FILE: &str = "ip6tables.rules";
/// A snapshot older than this no longer describes the firewall: the timer
/// stopped, so the source is reported down rather than trusting old rules.
pub const SNAPSHOT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(300);

/// Why a snapshot file was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// The file does not exist (timer not installed or not run yet)
    Missing,
    /// Last written longer ago than the allowed age
    Stale { age_secs: u64 },
    /// It exists but could not be read (permissions, not UTF-8...)
    Unreadable(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Missing => write!(f, "missing"),
            SnapshotError::Stale { age_secs } => write!(f, "stale ({}s old)", age_secs),
            SnapshotError::Unreadable(e) => write!(f, "unreadable: {}", e),
        }
    }
}

/// Read one `iptables-save` snapshot file written by the timer.
///
/// The file is trusted only while its mtime is within `max_age` of `now`; a
/// timestamp in the future (clock step) counts as fresh.
pub fn read_rules_file(
    path: &std::path::Path,
    now: std::time::SystemTime,
    max_age: std::time::Duration,
) -> Result<Ruleset, SnapshotError> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(SnapshotError::Missing),
        Err(e) => return Err(SnapshotError::Unreadable(e.to_string())),
    };
    let modified = metadata
        .modified()
        .map_err(|e| SnapshotError::Unreadable(e.to_string()))?;
    let age = now.duration_since(modified).unwrap_or_default();
    if age > max_age {
        return Err(SnapshotError::Stale {
            age_secs: age.as_secs(),
        });
    }
    let text = std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => SnapshotError::Missing,
        _ => SnapshotError::Unreadable(e.to_string()),
    })?;
    Ok(parse_iptables_save(&text))
}

// ─────────────────────────────────────────
// Evaluation
// ─────────────────────────────────────────

/// Three-valued match result: the probe stands for every public source at
/// once, so a rule can match all of them, none, or only some.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tri {
    Yes,
    No,
    Maybe,
}

impl Tri {
    fn and(self, other: Tri) -> Tri {
        match (self, other) {
            (Tri::No, _) | (_, Tri::No) => Tri::No,
            (Tri::Yes, Tri::Yes) => Tri::Yes,
            _ => Tri::Maybe,
        }
    }

    fn not(self) -> Tri {
        match self {
            Tri::Yes => Tri::No,
            Tri::No => Tri::Yes,
            Tri::Maybe => Tri::Maybe,
        }
    }

    fn from_bool(value: bool) -> Tri {
        if value {
            Tri::Yes
        } else {
            Tri::No
        }
    }
}

/// A new TCP connection from some public address.
struct Probe<'a> {
    /// Destination port as the filter sees it (after DNAT on the forward path)
    dport: u16,
    /// Port the client asked for, before any DNAT
    orig_dport: u16,
    /// On the forward path the destination is a container's private address
    forwarded: bool,
    public_ifaces: &'a [String],
}

/// Interfaces the internet does not arrive on, used only when the default
/// route is unknown.
const INTERNAL_IFACE_PREFIXES: &[&str] = &[
    "lo",
    "docker",
    "br-",
    "veth",
    "wg",
    "tun",
    "tap",
    "virbr",
    "tailscale",
    "zt",
    "cni",
    "flannel",
    "cali",
    "kube",
];

/// Address ranges no internet packet carries as its source.
const NON_PUBLIC_V4: &[(&str, u8)] = &[
    ("0.0.0.0", 8),
    ("10.0.0.0", 8),
    ("100.64.0.0", 10),
    ("127.0.0.0", 8),
    ("169.254.0.0", 16),
    ("172.16.0.0", 12),
    ("192.168.0.0", 16),
    ("224.0.0.0", 4),
    ("240.0.0.0", 4),
];
const NON_PUBLIC_V6: &[(&str, u8)] = &[
    ("::", 128),
    ("::1", 128),
    ("fc00::", 7),
    ("fe80::", 10),
    ("ff00::", 8),
];
/// Destinations an inbound internet connection never has.
const NEVER_DESTINATION_V4: &[(&str, u8)] = &[("127.0.0.0", 8), ("224.0.0.0", 4)];
const NEVER_DESTINATION_V6: &[(&str, u8)] = &[("::1", 128), ("ff00::", 8)];

/// `address[/prefix]` as an address and prefix length.
fn parse_cidr(value: &str) -> Option<(IpAddr, u8)> {
    let (addr, prefix) = match value.split_once('/') {
        Some((addr, prefix)) => (addr, Some(prefix)),
        None => (value, None),
    };
    let addr: IpAddr = addr.parse().ok()?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = match prefix {
        // A dotted mask (255.255.255.0) instead of a length
        Some(p) if p.contains('.') => {
            let mask: std::net::Ipv4Addr = p.parse().ok()?;
            u32::from(mask).count_ones() as u8
        }
        Some(p) => p.parse().ok()?,
        None => max,
    };
    (prefix <= max).then_some((addr, prefix))
}

fn to_bits(addr: IpAddr) -> (u128, u8) {
    match addr {
        IpAddr::V4(a) => (u32::from(a) as u128, 32),
        IpAddr::V6(a) => (u128::from(a), 128),
    }
}

/// Whether the network `addr/prefix` lies entirely inside `range/range_prefix`.
fn within(addr: IpAddr, prefix: u8, range: &str, range_prefix: u8) -> bool {
    let Ok(range) = range.parse::<IpAddr>() else {
        return false;
    };
    if addr.is_ipv4() != range.is_ipv4() || prefix < range_prefix {
        return false;
    }
    let (a, width) = to_bits(addr);
    let (r, _) = to_bits(range);
    let shift = u32::from(width - range_prefix);
    if shift >= 128 {
        return true;
    }
    (a >> shift) == (r >> shift)
}

fn within_any(addr: IpAddr, prefix: u8, ranges: &[(&str, u8)]) -> bool {
    ranges
        .iter()
        .any(|(range, len)| within(addr, prefix, range, *len))
}

fn match_source(value: &str) -> Tri {
    let Some((addr, prefix)) = parse_cidr(value) else {
        return Tri::Maybe;
    };
    if prefix == 0 {
        return Tri::Yes;
    }
    let private = if addr.is_ipv4() {
        NON_PUBLIC_V4
    } else {
        NON_PUBLIC_V6
    };
    if within_any(addr, prefix, private) {
        Tri::No
    } else {
        Tri::Maybe
    }
}

fn match_destination(value: &str, probe: &Probe) -> Tri {
    let Some((addr, prefix)) = parse_cidr(value) else {
        return Tri::Maybe;
    };
    if prefix == 0 {
        return Tri::Yes;
    }
    let never = if addr.is_ipv4() {
        NEVER_DESTINATION_V4
    } else {
        NEVER_DESTINATION_V6
    };
    if within_any(addr, prefix, never) {
        return Tri::No;
    }
    // Before DNAT the destination is the address the client dialled, which
    // is public; a rule for a private address of the host does not apply
    if !probe.forwarded {
        let private = if addr.is_ipv4() {
            NON_PUBLIC_V4
        } else {
            NON_PUBLIC_V6
        };
        if within_any(addr, prefix, private) {
            return Tri::No;
        }
    }
    Tri::Maybe
}

fn match_in_iface(value: &str, probe: &Probe) -> Tri {
    let matches_name = |iface: &str| match value.strip_suffix('+') {
        Some(prefix) => iface.starts_with(prefix),
        None => iface == value,
    };
    if probe.public_ifaces.is_empty() {
        if value == "+" {
            return Tri::Yes;
        }
        let internal = INTERNAL_IFACE_PREFIXES.iter().any(|p| {
            value.starts_with(p) || value.strip_suffix('+').is_some_and(|v| v.starts_with(p))
        });
        return if internal { Tri::No } else { Tri::Maybe };
    }
    let hits = probe
        .public_ifaces
        .iter()
        .filter(|i| matches_name(i))
        .count();
    if hits == 0 {
        Tri::No
    } else if hits == probe.public_ifaces.len() {
        Tri::Yes
    } else {
        Tri::Maybe
    }
}

fn port_in(spec: &str, port: u16) -> Option<bool> {
    match spec.split_once(':') {
        Some((low, high)) => {
            let low: u16 = if low.is_empty() { 0 } else { low.parse().ok()? };
            let high: u16 = if high.is_empty() {
                u16::MAX
            } else {
                high.parse().ok()?
            };
            Some((low..=high).contains(&port))
        }
        None => Some(spec.parse::<u16>().ok()? == port),
    }
}

fn match_port(spec: &str, port: u16) -> Tri {
    port_in(spec, port).map_or(Tri::Maybe, Tri::from_bool)
}

fn match_port_list(spec: &str, port: u16) -> Tri {
    let mut any = false;
    for part in spec.split(',') {
        match port_in(part, port) {
            Some(true) => any = true,
            Some(false) => {}
            None => return Tri::Maybe,
        }
    }
    Tri::from_bool(any)
}

fn list_contains(spec: &str, wanted: &[&str]) -> bool {
    spec.split(',')
        .any(|item| wanted.iter().any(|w| item.eq_ignore_ascii_case(w)))
}

fn eval_match(m: &Match, probe: &Probe) -> Tri {
    let value = m.values.first().map(String::as_str).unwrap_or("");
    let result = match m.option.as_str() {
        // Loading a module or commenting never filters by itself
        "-m" | "--comment" => return Tri::Yes,
        "-p" => Tri::from_bool(matches!(
            value.to_ascii_lowercase().as_str(),
            "tcp" | "6" | "all" | "0"
        )),
        "-s" => match_source(value),
        "-d" => match_destination(value, probe),
        "-i" => match_in_iface(value, probe),
        "--dport" => match_port(value, probe.dport),
        "--dports" => match_port_list(value, probe.dport),
        "--ctorigdstport" => match_port(value, probe.orig_dport),
        "--ctstate" => Tri::from_bool(list_contains(value, &["NEW"])),
        "--dst-type" => Tri::from_bool(list_contains(value, &["LOCAL", "UNICAST"])),
        "--src-type" => Tri::from_bool(list_contains(value, &["UNICAST"])),
        // -o, --sport, limits, ipsets, recent, string, ...: cannot tell
        _ => Tri::Maybe,
    };
    if m.negated {
        result.not()
    } else {
        result
    }
}

/// Targets that only observe or mark a packet and let it continue.
const NON_TERMINAL_TARGETS: &[&str] = &[
    "LOG",
    "NFLOG",
    "ULOG",
    "MARK",
    "CONNMARK",
    "CT",
    "NOTRACK",
    "TRACE",
    "AUDIT",
    "TCPMSS",
    "CLASSIFY",
    "DSCP",
    "TOS",
    "TTL",
    "HL",
    "SET",
    "CHECKSUM",
    "LED",
    "IDLETIMER",
    "SECMARK",
    "CONNSECMARK",
];

/// Where a packet can end up after a chain.
#[derive(Debug, Clone, Copy, Default)]
struct Outcomes {
    accept: bool,
    drop: bool,
    /// Left the chain without a verdict (RETURN or end of a user chain)
    returns: bool,
}

/// Effect of one rule's target.
#[derive(Debug, Clone, Copy, Default)]
struct Effect {
    accept: bool,
    drop: bool,
    returns: bool,
    /// Evaluation can go on with the next rule
    continues: bool,
}

const MAX_DEPTH: u8 = 32;

impl Ruleset {
    fn table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    /// Whether a new TCP connection to `port` from some public address would
    /// be accepted, on the input path or, if DNAT catches it, the forward path.
    pub fn accepts_from_internet(&self, port: u16, public_ifaces: &[String]) -> bool {
        let inbound = Probe {
            dport: port,
            orig_dport: port,
            forwarded: false,
            public_ifaces,
        };
        let redirects = self.dnat_destinations(&inbound);
        if !redirects.is_empty() {
            return redirects.into_iter().any(|to_port| {
                let forwarded = Probe {
                    dport: to_port,
                    orig_dport: port,
                    forwarded: true,
                    public_ifaces,
                };
                self.builtin_accepts("FORWARD", &forwarded)
            });
        }
        self.builtin_accepts("INPUT", &inbound)
    }

    /// Ports that `DNAT` rules in the `nat` table may send the probe to.
    ///
    /// A rule bound to a loopback or private host address (`-p
    /// 127.0.0.1:5432:5432`) cannot catch a connection dialled to a public
    /// address, so it does not count.
    fn dnat_destinations(&self, probe: &Probe) -> Vec<u16> {
        let Some(nat) = self.table("nat") else {
            return vec![];
        };
        let mut ports = Vec::new();
        for chain in nat.chains.values() {
            for rule in &chain.rules {
                let Some(target) = rule.target.as_ref().filter(|t| t.name == "DNAT") else {
                    continue;
                };
                if rule_match(rule, probe) == Tri::No {
                    continue;
                }
                let to_port = target
                    .options
                    .iter()
                    .find(|o| o.option == "--to-destination")
                    .and_then(|o| o.values.first())
                    .and_then(|to| destination_port(to))
                    .unwrap_or(probe.dport);
                ports.push(to_port);
            }
        }
        ports
    }

    fn builtin_accepts(&self, chain: &str, probe: &Probe) -> bool {
        // No filter table loaded means nothing filters
        let Some(filter) = self.table("filter") else {
            return true;
        };
        let Some(builtin) = filter.chains.get(chain) else {
            return true;
        };
        let out = eval_chain(filter, chain, probe, 0);
        let policy_accepts = builtin.policy.as_deref() != Some("DROP");
        out.accept || (out.returns && policy_accepts)
    }
}

/// Port of a `--to-destination` value: `1.2.3.4:80`, `[::1]:80`, `1.2.3.4:80-90`.
fn destination_port(to: &str) -> Option<u16> {
    let port = if let Some(rest) = to.strip_prefix('[') {
        rest.split_once("]:")?.1
    } else {
        to.rsplit_once(':')?.1
    };
    port.split('-').next()?.parse().ok()
}

fn rule_match(rule: &Rule, probe: &Probe) -> Tri {
    rule.matches
        .iter()
        .fold(Tri::Yes, |acc, m| acc.and(eval_match(m, probe)))
}

fn eval_chain(table: &Table, name: &str, probe: &Probe, depth: u8) -> Outcomes {
    let Some(chain) = table.chains.get(name) else {
        return Outcomes {
            returns: true,
            ..Default::default()
        };
    };
    if depth > MAX_DEPTH {
        // Give up on the safe side: it may be accepted
        return Outcomes {
            accept: true,
            returns: true,
            drop: false,
        };
    }

    let mut out = Outcomes::default();
    for rule in &chain.rules {
        let matched = rule_match(rule, probe);
        if matched == Tri::No {
            continue;
        }
        let Some(target) = &rule.target else {
            continue;
        };
        let effect = target_effect(table, target, probe, depth);
        out.accept |= effect.accept;
        out.drop |= effect.drop;
        out.returns |= effect.returns;
        if matched == Tri::Yes && !effect.continues {
            return out;
        }
    }
    out.returns = true;
    out
}

fn target_effect(table: &Table, target: &Target, probe: &Probe, depth: u8) -> Effect {
    match target.name.as_str() {
        "ACCEPT" => Effect {
            accept: true,
            ..Default::default()
        },
        "DROP" | "REJECT" => Effect {
            drop: true,
            ..Default::default()
        },
        "RETURN" => Effect {
            returns: true,
            ..Default::default()
        },
        name if table.chains.contains_key(name) => {
            let sub = eval_chain(table, name, probe, depth + 1);
            if target.goto {
                // A RETURN in a goto'd chain leaves the calling chain too
                Effect {
                    accept: sub.accept,
                    drop: sub.drop,
                    returns: sub.returns,
                    continues: false,
                }
            } else {
                Effect {
                    accept: sub.accept,
                    drop: sub.drop,
                    returns: false,
                    continues: sub.returns,
                }
            }
        }
        name if NON_TERMINAL_TARGETS.contains(&name) => Effect {
            continues: true,
            ..Default::default()
        },
        // NFQUEUE, a missing chain, an extension we do not know: assume the
        // worst — it may let the packet in, and it may not stop it either
        _ => Effect {
            accept: true,
            continues: true,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape ufw produces with `default deny incoming`, a database open
    /// only to the VPN and the docker networks, and SSH/HTTP open to all.
    /// Addresses are documentation ranges.
    const UFW_DENY: &str = r#"# Generated by iptables-save v1.8.9 (nf_tables)
*filter
:INPUT DROP [1575936:98695196]
:FORWARD DROP [0:0]
:OUTPUT ACCEPT [14470:969689]
:f2b-sshd - [0:0]
:ufw-after-input - [0:0]
:ufw-after-logging-input - [0:0]
:ufw-before-input - [0:0]
:ufw-before-logging-input - [0:0]
:ufw-logging-deny - [0:0]
:ufw-not-local - [0:0]
:ufw-reject-input - [0:0]
:ufw-skip-to-policy-input - [0:0]
:ufw-track-input - [0:0]
:ufw-user-input - [0:0]
:ufw-user-limit - [0:0]
:ufw-user-limit-accept - [0:0]
-A INPUT -p tcp -m multiport --dports 22 -j f2b-sshd
-A INPUT -j ufw-before-logging-input
-A INPUT -j ufw-before-input
-A INPUT -j ufw-after-input
-A INPUT -j ufw-after-logging-input
-A INPUT -j ufw-reject-input
-A INPUT -j ufw-track-input
-A f2b-sshd -s 198.51.100.7/32 -j REJECT --reject-with icmp-port-unreachable
-A f2b-sshd -j RETURN
-A ufw-after-input -p udp -m udp --dport 137 -j ufw-skip-to-policy-input
-A ufw-after-input -p tcp -m tcp --dport 445 -j ufw-skip-to-policy-input
-A ufw-after-input -m addrtype --dst-type BROADCAST -j ufw-skip-to-policy-input
-A ufw-after-logging-input -m limit --limit 3/min --limit-burst 10 -j LOG --log-prefix "[UFW BLOCK] "
-A ufw-before-input -i lo -j ACCEPT
-A ufw-before-input -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
-A ufw-before-input -m conntrack --ctstate INVALID -j ufw-logging-deny
-A ufw-before-input -m conntrack --ctstate INVALID -j DROP
-A ufw-before-input -p icmp -m icmp --icmp-type 8 -j ACCEPT
-A ufw-before-input -p udp -m udp --sport 67 --dport 68 -j ACCEPT
-A ufw-before-input -j ufw-not-local
-A ufw-before-input -d 224.0.0.251/32 -p udp -m udp --dport 5353 -j ACCEPT
-A ufw-before-input -j ufw-user-input
-A ufw-logging-deny -m conntrack --ctstate INVALID -m limit --limit 3/min --limit-burst 10 -j RETURN
-A ufw-logging-deny -m limit --limit 3/min --limit-burst 10 -j LOG --log-prefix "[UFW BLOCK] "
-A ufw-not-local -m addrtype --dst-type LOCAL -j RETURN
-A ufw-not-local -m addrtype --dst-type MULTICAST -j RETURN
-A ufw-not-local -m limit --limit 3/min --limit-burst 10 -j ufw-logging-deny
-A ufw-not-local -j DROP
-A ufw-skip-to-policy-input -j DROP
-A ufw-user-input -p tcp -m tcp --dport 22 -m comment --comment "\'dapp_OpenSSH\'" -j ACCEPT
-A ufw-user-input -p tcp -m multiport --dports 80,443 -m comment --comment "\'dapp_Nginx%20Full\'" -j ACCEPT
-A ufw-user-input -s 10.20.0.0/24 -p tcp -m tcp --dport 5432 -j ACCEPT
-A ufw-user-input -s 172.16.0.0/12 -p tcp -m tcp --dport 5432 -j ACCEPT
-A ufw-user-input -s 10.20.0.0/24 -j ACCEPT
-A ufw-user-input -s 203.0.113.10/32 -p tcp -m tcp --dport 10050 -j ACCEPT
-A ufw-user-input -p tcp -m tcp --dport 9000 -j DROP
-A ufw-user-limit -m limit --limit 3/min -j LOG --log-prefix "[UFW LIMIT BLOCK] "
-A ufw-user-limit -j REJECT --reject-with icmp-port-unreachable
-A ufw-user-limit-accept -j ACCEPT
COMMIT
"#;

    /// Docker publishing 5432 on every address, and 9121 on loopback only.
    const DOCKER_NAT: &str = r#"*nat
:PREROUTING ACCEPT [0:0]
:OUTPUT ACCEPT [0:0]
:DOCKER - [0:0]
-A PREROUTING -m addrtype --dst-type LOCAL -j DOCKER
-A DOCKER -i docker0 -j RETURN
-A DOCKER ! -i br-0123456789ab -p tcp -m tcp --dport 5433 -j DNAT --to-destination 172.18.0.2:5432
-A DOCKER -d 127.0.0.1/32 ! -i docker0 -p tcp -m tcp --dport 9121 -j DNAT --to-destination 172.17.0.2:9121
-A DOCKER -d 198.51.100.8/32 ! -i docker0 -p tcp -m tcp --dport 3307 -j DNAT --to-destination 172.17.0.3:3306
COMMIT
"#;

    const DOCKER_FORWARD: &str = r#":DOCKER - [0:0]
:DOCKER-CT - [0:0]
:DOCKER-FORWARD - [0:0]
:DOCKER-BRIDGE - [0:0]
:DOCKER-USER - [0:0]
-A FORWARD -j DOCKER-USER
-A FORWARD -j DOCKER-FORWARD
-A DOCKER-FORWARD -j DOCKER-CT
-A DOCKER-FORWARD -j DOCKER-BRIDGE
-A DOCKER-FORWARD -i br-0123456789ab -j ACCEPT
-A DOCKER-CT -o br-0123456789ab -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
-A DOCKER-BRIDGE -o br-0123456789ab -j DOCKER
-A DOCKER -d 172.18.0.2/32 ! -i br-0123456789ab -o br-0123456789ab -p tcp -m tcp --dport 5432 -j ACCEPT
-A DOCKER ! -i br-0123456789ab -o br-0123456789ab -j DROP
"#;

    fn eth0() -> Vec<String> {
        vec!["eth0".to_string()]
    }

    fn docker_host(docker_user: &str) -> Ruleset {
        let filter = UFW_DENY.replace(
            ":FORWARD DROP [0:0]\n",
            &format!(":FORWARD DROP [0:0]\n{DOCKER_FORWARD}{docker_user}"),
        );
        parse_iptables_save(&format!("{filter}{DOCKER_NAT}"))
    }

    #[test]
    fn splits_quoted_words() {
        assert_eq!(
            split_words(r#"-A X -m comment --comment "\'dapp_Nginx Full\'" -j ACCEPT"#),
            vec![
                "-A",
                "X",
                "-m",
                "comment",
                "--comment",
                "'dapp_Nginx Full'",
                "-j",
                "ACCEPT"
            ]
        );
    }

    #[test]
    fn parses_tables_chains_and_policies() {
        let rules = parse_iptables_save(UFW_DENY);
        let filter = rules.table("filter").unwrap();
        assert_eq!(
            filter.chains["INPUT"].policy.as_deref(),
            Some("DROP"),
            "built-in chains keep their policy"
        );
        assert_eq!(filter.chains["ufw-user-input"].policy, None);
        assert_eq!(filter.chains["ufw-user-input"].rules.len(), 7);
        let comment = &filter.chains["ufw-user-input"].rules[0].matches;
        assert!(comment.iter().any(|m| m.option == "--comment"));
    }

    #[test]
    fn ufw_default_deny_filters_a_database_open_only_to_private_networks() {
        let rules = parse_iptables_save(UFW_DENY);
        assert!(!rules.accepts_from_internet(5432, &eth0()));
        // Same answer when the default route is unknown
        assert!(!rules.accepts_from_internet(5432, &[]));
    }

    #[test]
    fn ufw_allow_rule_exposes_the_port() {
        let rules = parse_iptables_save(UFW_DENY);
        assert!(rules.accepts_from_internet(22, &eth0()));
        assert!(rules.accepts_from_internet(443, &eth0()));
        // Open to one public address only: still reachable from the internet
        assert!(rules.accepts_from_internet(10050, &eth0()));
        // Explicit deny
        assert!(!rules.accepts_from_internet(9000, &eth0()));
    }

    #[test]
    fn ufw_default_allow_exposes_everything_not_denied() {
        let allow = UFW_DENY.replace(
            "COMMIT",
            "-A ufw-track-input -p tcp -m conntrack --ctstate NEW -j ACCEPT\nCOMMIT",
        );
        let rules = parse_iptables_save(&allow);
        assert!(rules.accepts_from_internet(5432, &eth0()));
        assert!(!rules.accepts_from_internet(9000, &eth0()));
    }

    #[test]
    fn empty_or_permissive_rules_accept() {
        assert!(parse_iptables_save("").accepts_from_internet(5432, &eth0()));
        let open = "*filter\n:INPUT ACCEPT [0:0]\n:FORWARD ACCEPT [0:0]\nCOMMIT\n";
        assert!(parse_iptables_save(open).accepts_from_internet(3306, &eth0()));
    }

    #[test]
    fn interface_rules_follow_the_default_route() {
        let text = "*filter\n:INPUT DROP [0:0]\n-A INPUT -i wg0 -j ACCEPT\n-A INPUT -i eth0 -p tcp --dport 5432 -j DROP\n-A INPUT -p tcp --dport 5432 -j ACCEPT\nCOMMIT\n";
        let rules = parse_iptables_save(text);
        // Internet arrives on eth0: the drop catches it, wg0 is irrelevant
        assert!(!rules.accepts_from_internet(5432, &eth0()));
        // With two ways in, the drop on eth0 does not cover ens4
        let two = vec!["eth0".to_string(), "ens4".to_string()];
        assert!(rules.accepts_from_internet(5432, &two));
        // `! -i lo` covers every public interface
        let not_lo =
            "*filter\n:INPUT ACCEPT [0:0]\n-A INPUT ! -i lo -p tcp --dport 6432 -j DROP\nCOMMIT\n";
        assert!(!parse_iptables_save(not_lo).accepts_from_internet(6432, &eth0()));
    }

    #[test]
    fn rules_it_cannot_judge_never_hide_an_open_port() {
        // A rate-limited drop does not drop everything
        let limited = "*filter\n:INPUT ACCEPT [0:0]\n-A INPUT -p tcp --dport 5432 -m limit --limit 3/min -j DROP\nCOMMIT\n";
        assert!(parse_iptables_save(limited).accepts_from_internet(5432, &eth0()));
        // An accept bound to an ipset may let the internet in
        let ipset = "*filter\n:INPUT DROP [0:0]\n-A INPUT -p tcp --dport 5432 -m set --match-set allow src -j ACCEPT\nCOMMIT\n";
        assert!(parse_iptables_save(ipset).accepts_from_internet(5432, &eth0()));
        // An unknown target may accept
        let queue = "*filter\n:INPUT DROP [0:0]\n-A INPUT -p tcp --dport 5432 -j NFQUEUE --queue-num 1\nCOMMIT\n";
        assert!(parse_iptables_save(queue).accepts_from_internet(5432, &eth0()));
    }

    #[test]
    fn negations_and_port_ranges() {
        let text = "*filter\n:INPUT DROP [0:0]\n-A INPUT ! -s 10.0.0.0/8 -p tcp -m tcp --dport 5000:6000 -j DROP\n-A INPUT -p tcp -j ACCEPT\nCOMMIT\n";
        let rules = parse_iptables_save(text);
        assert!(!rules.accepts_from_internet(5432, &eth0()));
        assert!(rules.accepts_from_internet(3306, &eth0()));
        let not_port =
            "*filter\n:INPUT DROP [0:0]\n-A INPUT -p tcp -m tcp ! --dport 22 -j ACCEPT\nCOMMIT\n";
        let rules = parse_iptables_save(not_port);
        assert!(rules.accepts_from_internet(5432, &eth0()));
        assert!(!rules.accepts_from_internet(22, &eth0()));
    }

    #[test]
    fn goto_returns_past_the_caller() {
        let text = "*filter\n:INPUT DROP [0:0]\n:sub - [0:0]\n-A INPUT -g sub\n-A INPUT -j ACCEPT\n-A sub -j RETURN\nCOMMIT\n";
        // RETURN from a goto'd chain falls to INPUT's policy, skipping ACCEPT
        assert!(!parse_iptables_save(text).accepts_from_internet(5432, &eth0()));
        let jump = text.replace("-g sub", "-j sub");
        assert!(parse_iptables_save(&jump).accepts_from_internet(5432, &eth0()));
    }

    #[test]
    fn docker_published_port_bypasses_ufw() {
        let rules = docker_host("-A DOCKER-USER -j RETURN\n");
        // ufw would drop 5433 on INPUT, but DNAT sends it through FORWARD
        assert!(rules.accepts_from_internet(5433, &eth0()));
        // Published on loopback or on a private address only: not reachable
        assert!(!rules.accepts_from_internet(9121, &eth0()));
        assert!(!rules.accepts_from_internet(3307, &eth0()));
    }

    #[test]
    fn docker_user_can_close_a_published_port() {
        let by_container_port =
            docker_host("-A DOCKER-USER -i eth0 -p tcp -m tcp --dport 5432 -j DROP\n");
        assert!(!by_container_port.accepts_from_internet(5433, &eth0()));
        let by_original_port = docker_host(
            "-A DOCKER-USER -p tcp -m conntrack --ctorigdstport 5433 -j DROP\n-A DOCKER-USER -j RETURN\n",
        );
        assert!(!by_original_port.accepts_from_internet(5433, &eth0()));
        let private_only =
            docker_host("-A DOCKER-USER -s 10.0.0.0/8 -j RETURN\n-A DOCKER-USER -i eth0 -j DROP\n");
        assert!(!private_only.accepts_from_internet(5433, &eth0()));
    }

    #[test]
    fn exposure_needs_both_families_for_a_dual_stack_socket() {
        let closed = parse_iptables_save(UFW_DENY);
        let open = parse_iptables_save("*filter\n:INPUT ACCEPT [0:0]\nCOMMIT\n");
        let snapshot = FirewallSnapshot {
            v4: Some(closed.clone()),
            v6: Some(open),
            public_ifaces_v4: eth0(),
            public_ifaces_v6: eth0(),
        };
        assert_eq!(snapshot.exposure("0.0.0.0", 5432), Some(Exposure::Filtered));
        assert_eq!(snapshot.exposure("::", 5432), Some(Exposure::Public));
        assert!(snapshot.complete());

        let v4_only = FirewallSnapshot {
            v4: Some(closed),
            v6: None,
            ..Default::default()
        };
        assert_eq!(v4_only.exposure("0.0.0.0", 5432), Some(Exposure::Filtered));
        assert_eq!(v4_only.exposure("::", 5432), None, "IPv6 was not read");
        assert_eq!(v4_only.exposure("not-an-address", 5432), None);
        assert!(!v4_only.complete());
    }

    #[test]
    fn default_route_interfaces() {
        let route =
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t016433C6\t0003\t0\t0\t0\t00000000\t0\t0\t0\n\
eth0\t006433C6\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n\
wg0\t0000140A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_ipv4_default_ifaces(route), vec!["eth0"]);
        let route6 = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 eth0\n\
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo\n\
20010db8000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001 eth0\n";
        assert_eq!(parse_ipv6_default_ifaces(route6), vec!["eth0"]);
        assert!(parse_ipv4_default_ifaces("").is_empty());
    }

    #[test]
    fn address_helpers() {
        assert_eq!(match_source("0.0.0.0/0"), Tri::Yes);
        assert_eq!(match_source("10.20.0.0/24"), Tri::No);
        assert_eq!(match_source("10.0.0.0/255.0.0.0"), Tri::No);
        assert_eq!(
            match_source("172.0.0.0/8"),
            Tri::Maybe,
            "wider than 172.16/12"
        );
        assert_eq!(match_source("203.0.113.10"), Tri::Maybe);
        assert_eq!(match_source("fd00::/8"), Tri::No);
        assert_eq!(match_source("2001:db8::/32"), Tri::Maybe);
        assert_eq!(match_source("::/0"), Tri::Yes);
        assert_eq!(match_source("garbage"), Tri::Maybe);
        assert_eq!(destination_port("172.18.0.2:5432"), Some(5432));
        assert_eq!(destination_port("[fd00::2]:80"), Some(80));
        assert_eq!(destination_port("172.18.0.2:80-90"), Some(80));
        assert_eq!(destination_port("172.18.0.2"), None);
        assert_eq!(match_port_list("80,443,8000:8100", 8080), Tri::Yes);
        assert_eq!(match_port_list("80,x", 8080), Tri::Maybe);
        assert_eq!(match_port(":1024", 22), Tri::Yes);
    }

    /// Write `text` as the timer would and back-date its mtime by `age`.
    fn snapshot_file(
        dir: &std::path::Path,
        text: &str,
        age: std::time::Duration,
    ) -> std::path::PathBuf {
        let path = dir.join(SNAPSHOT_V4_FILE);
        std::fs::write(&path, text).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() - age)
            .unwrap();
        path
    }

    #[test]
    fn snapshot_file_parses_like_iptables_save_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = snapshot_file(dir.path(), UFW_DENY, std::time::Duration::from_secs(30));
        let rules = read_rules_file(&path, std::time::SystemTime::now(), SNAPSHOT_MAX_AGE).unwrap();
        // Same rule set as parsing the command output directly
        assert_eq!(rules, parse_iptables_save(UFW_DENY));
        assert!(!rules.accepts_from_internet(5432, &eth0()));
        assert!(rules.accepts_from_internet(22, &eth0()));

        // Docker's published-port path survives the round trip too
        let filter = UFW_DENY.replace(
            ":FORWARD DROP [0:0]\n",
            &format!(":FORWARD DROP [0:0]\n{DOCKER_FORWARD}"),
        );
        let docker = format!("{filter}{DOCKER_NAT}");
        let path = snapshot_file(dir.path(), &docker, std::time::Duration::ZERO);
        let rules = read_rules_file(&path, std::time::SystemTime::now(), SNAPSHOT_MAX_AGE).unwrap();
        assert_eq!(rules, docker_host(""));
        assert!(rules.accepts_from_internet(5433, &eth0()));
    }

    #[test]
    fn stale_snapshot_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let path = snapshot_file(dir.path(), UFW_DENY, std::time::Duration::from_secs(301));
        match read_rules_file(&path, std::time::SystemTime::now(), SNAPSHOT_MAX_AGE) {
            Err(SnapshotError::Stale { age_secs }) => assert!(age_secs >= 300),
            other => panic!("expected stale, got {:?}", other),
        }
        // Just inside the window it is still used
        let path = snapshot_file(dir.path(), UFW_DENY, std::time::Duration::from_secs(290));
        assert!(read_rules_file(&path, std::time::SystemTime::now(), SNAPSHOT_MAX_AGE).is_ok());
    }

    #[test]
    fn missing_snapshot_is_reported_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_rules_file(
                &dir.path().join(SNAPSHOT_V6_FILE),
                std::time::SystemTime::now(),
                SNAPSHOT_MAX_AGE
            ),
            Err(SnapshotError::Missing)
        );
        assert_eq!(SnapshotError::Missing.to_string(), "missing");
        assert_eq!(
            SnapshotError::Stale { age_secs: 400 }.to_string(),
            "stale (400s old)"
        );
    }

    #[test]
    fn snapshot_from_the_future_counts_as_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = snapshot_file(dir.path(), UFW_DENY, std::time::Duration::ZERO);
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        assert!(read_rules_file(&path, an_hour_ago, SNAPSHOT_MAX_AGE).is_ok());
    }

    #[test]
    fn unreadable_snapshot_is_an_error_not_an_empty_rule_set() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be: metadata works, reading does not
        let path = dir.path().join(SNAPSHOT_V4_FILE);
        std::fs::create_dir(&path).unwrap();
        let err =
            read_rules_file(&path, std::time::SystemTime::now(), SNAPSHOT_MAX_AGE).unwrap_err();
        assert!(matches!(err, SnapshotError::Unreadable(_)), "{:?}", err);
        assert!(err.to_string().starts_with("unreadable: "));
    }
}
