//! Audit findings: the conclusions drawn from one snapshot of a host
//!
//! The alert manager in [`crate::alerts`] is stateful by design: it samples
//! over windows, applies cooldowns and remembers what it already notified. That
//! suits a client sitting in front of a terminal, but it means every finding
//! lives only as long as somebody keeps the dashboard open.
//!
//! This module is the stateless counterpart. Given one snapshot of a host it
//! returns every finding that holds *right now*, each one keyed by a stable
//! `(check, target)` pair. Duration, deduplication and paging policy are left
//! to whoever consumes the list — typically Prometheus, with `for:` clauses on
//! its alerting rules — so the agent itself keeps no history.

use crate::alerts::{AlertManager, AlertSeverity};
use crate::commitment::{self, CommitmentLevel, MemoryCommitment};
use crate::firewall::{Exposure, FirewallSnapshot};
use crate::network::{self, NetworkState};
use crate::types::{
    ContainerInfo, HealthAssessment, ListeningPortInfo, MariaDBClusterInfo, PostgresClusterInfo,
    SystemInfo, SystemdFailedUnit, TlsSnapshot, VersionStatus,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::collections::BTreeSet;

/// Every check the audit knows how to run.
///
/// The string form is the `check` label exported to monitoring systems, so it
/// is part of the public contract: rename a variant's string and every alert
/// rule written against it silently stops matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    /// Postgres settings overridden by `ALTER SYSTEM` (`postgresql.auto.conf`)
    PostgresAlterSystemOverride,
    /// Running container without a memory limit
    ContainerNoMemoryLimit,
    /// Container whose healthcheck never worked: a configuration defect
    ContainerHealthcheckBroken,
    /// Running container whose healthcheck is failing now: the application is sick
    ContainerUnhealthy,
    /// Stopped containers left on the host (aggregated, one finding per host)
    ContainersStopped,
    /// Host paging above the thrashing rate
    SwapThrashing,
    /// Every task stalled on memory for a noticeable share of the last minute
    MemoryStalled,
    /// Container that would exceed its limit if the host's swap were cleared
    SwapoffWouldExceedLimit,
    /// systemd unit in the `failed` state
    SystemdUnitFailed,
    /// TLS certificate close to expiry, or already expired
    TlsCertificateExpiring,
    /// The unit that renews certificates is failing
    TlsRenewalBroken,
    /// Certificate still renewed although nothing serves it
    TlsCertificateOrphaned,
    /// Data store port reachable from the internet (or firewall unreadable)
    DatastoreExposed,
    /// Data store bound to every interface, but the firewall keeps the
    /// internet out
    DatastoreBoundAllInterfaces,
    /// Configuration promises more memory than the host has
    MemoryOvercommitted,
    /// Postgres database with no transactions and no backends
    PostgresDatabaseIdle,
    /// MariaDB instance without a single application schema
    MariadbInstanceEmpty,
    /// MariaDB schema without tables
    MariadbSchemaEmpty,
    /// Image whose tag now points to a newer digest in the registry
    ImageDrift,
    DiskPressure,
    ScheduledJobFailed,
    ScheduledJobOverdue,
    ScheduledJobDisabled,
    ScheduledJobUnavailable,
}

/// Which collector a check depends on.
///
/// When a source could not be read, its checks have no answer — which is not
/// the same as having zero findings, and must not be reported as such.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    System,
    Docker,
    Postgres,
    Mariadb,
    Systemd,
    Tls,
    Ports,
    /// Packet filter rules; no check depends on it alone, it refines `ports`
    Firewall,
    /// networkd state; it refines `systemd`, and is read only when needed
    Network,
    ScheduledJobs,
}

impl Source {
    pub const ALL: &'static [Source] = &[
        Source::System,
        Source::Docker,
        Source::Postgres,
        Source::Mariadb,
        Source::Systemd,
        Source::Tls,
        Source::Ports,
        Source::Firewall,
        Source::ScheduledJobs,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Source::System => "system",
            Source::Docker => "docker",
            Source::Postgres => "postgres",
            Source::Mariadb => "mariadb",
            Source::Systemd => "systemd",
            Source::Tls => "tls",
            Source::Ports => "ports",
            Source::Firewall => "firewall",
            Source::Network => "network",
            Source::ScheduledJobs => "scheduled_jobs",
        }
    }
}

impl Check {
    pub const ALL: &'static [Check] = &[
        Check::PostgresAlterSystemOverride,
        Check::ContainerNoMemoryLimit,
        Check::ContainerHealthcheckBroken,
        Check::ContainerUnhealthy,
        Check::ContainersStopped,
        Check::SwapThrashing,
        Check::MemoryStalled,
        Check::SwapoffWouldExceedLimit,
        Check::SystemdUnitFailed,
        Check::TlsCertificateExpiring,
        Check::TlsRenewalBroken,
        Check::TlsCertificateOrphaned,
        Check::DatastoreExposed,
        Check::DatastoreBoundAllInterfaces,
        Check::MemoryOvercommitted,
        Check::PostgresDatabaseIdle,
        Check::MariadbInstanceEmpty,
        Check::MariadbSchemaEmpty,
        Check::ImageDrift,
        Check::DiskPressure,
        Check::ScheduledJobFailed,
        Check::ScheduledJobOverdue,
        Check::ScheduledJobDisabled,
        Check::ScheduledJobUnavailable,
    ];

    /// Stable identifier, used as the `check` label.
    pub fn as_str(&self) -> &'static str {
        match self {
            Check::PostgresAlterSystemOverride => "postgres_alter_system_override",
            Check::ContainerNoMemoryLimit => "container_no_memory_limit",
            Check::ContainerHealthcheckBroken => "container_healthcheck_broken",
            Check::ContainerUnhealthy => "container_unhealthy",
            Check::ContainersStopped => "containers_stopped",
            Check::SwapThrashing => "swap_thrashing",
            Check::MemoryStalled => "memory_stalled",
            Check::SwapoffWouldExceedLimit => "swapoff_would_exceed_limit",
            Check::SystemdUnitFailed => "systemd_unit_failed",
            Check::TlsCertificateExpiring => "tls_certificate_expiring",
            Check::TlsRenewalBroken => "tls_renewal_broken",
            Check::TlsCertificateOrphaned => "tls_certificate_orphaned",
            Check::DatastoreExposed => "datastore_exposed",
            Check::DatastoreBoundAllInterfaces => "datastore_bound_all_interfaces",
            Check::MemoryOvercommitted => "memory_overcommitted",
            Check::PostgresDatabaseIdle => "postgres_database_idle",
            Check::MariadbInstanceEmpty => "mariadb_instance_empty",
            Check::MariadbSchemaEmpty => "mariadb_schema_empty",
            Check::ImageDrift => "image_drift",
            Check::DiskPressure => "disk_pressure",
            Check::ScheduledJobFailed => "scheduled_job_failed",
            Check::ScheduledJobOverdue => "scheduled_job_overdue",
            Check::ScheduledJobDisabled => "scheduled_job_disabled",
            Check::ScheduledJobUnavailable => "scheduled_job_unavailable",
        }
    }

    /// The collector whose data the check reads.
    pub fn source(&self) -> Source {
        match self {
            Check::PostgresAlterSystemOverride | Check::PostgresDatabaseIdle => Source::Postgres,
            Check::ContainerNoMemoryLimit
            | Check::ContainerHealthcheckBroken
            | Check::ContainerUnhealthy
            | Check::ContainersStopped
            | Check::SwapoffWouldExceedLimit
            | Check::ImageDrift => Source::Docker,
            Check::SwapThrashing | Check::MemoryStalled | Check::MemoryOvercommitted => {
                Source::System
            }
            Check::SystemdUnitFailed => Source::Systemd,
            Check::DiskPressure => Source::System,
            Check::ScheduledJobFailed => Source::ScheduledJobs,
            Check::ScheduledJobOverdue => Source::ScheduledJobs,
            Check::ScheduledJobDisabled => Source::ScheduledJobs,
            Check::ScheduledJobUnavailable => Source::ScheduledJobs,

            Check::TlsCertificateExpiring
            | Check::TlsRenewalBroken
            | Check::TlsCertificateOrphaned => Source::Tls,
            Check::DatastoreExposed | Check::DatastoreBoundAllInterfaces => Source::Ports,
            Check::MariadbInstanceEmpty | Check::MariadbSchemaEmpty => Source::Mariadb,
        }
    }

    /// Why the check matters and how to fix it.
    ///
    /// Phase 0 of "how to fix": covers the checks whose meaning changed with
    /// the firewall, networkd and stopped-container refinements; the others
    /// return `None` until they get their text.
    pub fn guidance(&self) -> Option<Guidance> {
        let g = match self {
            Check::DatastoreExposed => Guidance {
                why_pt_br: "O banco/cache aceita conexões vindas da internet: qualquer um pode tentar senha, explorar falha da versão ou derrubar o serviço.",
                fix_pt_br: "Feche a porta: ligue o serviço em 127.0.0.1 ou no IP da VPN (listen_addresses/bind-address), ou bloqueie no firewall (ufw deny <porta>; para porta publicada pelo Docker, publique só em 127.0.0.1:<porta> ou bloqueie em DOCKER-USER). Se aparecer como warning, o agente não conseguiu ler o firewall: confira se o code-monitor-firewall-snapshot.timer está ativo (systemctl status code-monitor-firewall-snapshot.timer).",
                why_en: "The data store accepts connections from the internet: anyone can try passwords, exploit its version or knock it over.",
                fix_en: "Close the port: bind the service to 127.0.0.1 or the VPN address (listen_addresses/bind-address), or drop it in the firewall (ufw deny <port>; for a Docker-published port, publish on 127.0.0.1:<port> or drop it in DOCKER-USER). As a warning it means the agent could not read the firewall: check that code-monitor-firewall-snapshot.timer is active (systemctl status code-monitor-firewall-snapshot.timer).",
            },
            Check::DatastoreBoundAllInterfaces => Guidance {
                why_pt_br: "O serviço escuta em todas as interfaces, mas o firewall barra a internet. Hoje está fechado; uma regra ufw errada ou o firewall desligado o expõe na hora.",
                fix_pt_br: "Nada urgente. Para defesa em profundidade, ligue o serviço só em 127.0.0.1 e no IP da VPN em vez de 0.0.0.0/::.",
                why_en: "The service listens on every interface, but the firewall keeps the internet out. It is closed today; one wrong ufw rule or a disabled firewall exposes it at once.",
                fix_en: "Nothing urgent. For defence in depth, bind it to 127.0.0.1 and the VPN address instead of 0.0.0.0/::.",
            },
            Check::SystemdUnitFailed => Guidance {
                why_pt_br: "Uma unidade do systemd terminou em erro: backup, renovação, montagem ou serviço que deveria estar rodando pode não estar. Como info, é o systemd-networkd-wait-online que estourou o tempo no boot com a rede já online agora (link preso em 'configuring' por RA IPv6 do provedor).",
                fix_pt_br: "Veja o motivo com journalctl -u <unidade> -b e systemctl status <unidade>; corrija e rode systemctl reset-failed <unidade>. Para o wait-online: limite a espera às interfaces que importam (--interface=eth0 ou --any num drop-in) ou desabilite-o se nada depende de network-online.target.",
                why_en: "A systemd unit ended in error: a backup, renewal, mount or service that should be running may not be. As info, it is systemd-networkd-wait-online timing out at boot while the network is online now (a link stuck in 'configuring' by the provider's IPv6 RAs).",
                fix_en: "Read the cause with journalctl -u <unit> -b and systemctl status <unit>; fix it and run systemctl reset-failed <unit>. For wait-online: limit the wait to the links that matter (--interface=eth0 or --any in a drop-in) or disable it if nothing needs network-online.target.",
            },
            Check::ContainerUnhealthy => Guidance {
                why_pt_br: "Um container em execução está falhando o próprio healthcheck agora: a aplicação provavelmente não está atendendo.",
                fix_pt_br: "Veja docker inspect --format '{{json .State.Health}}' <container> e docker logs <container>; corrija a aplicação ou a dependência (banco, rede) e reinicie se preciso.",
                why_en: "A running container is failing its own healthcheck now: the application is probably not serving.",
                fix_en: "Check docker inspect --format '{{json .State.Health}}' <container> and docker logs <container>; fix the application or its dependency (database, network) and restart if needed.",
            },
            Check::ContainersStopped => Guidance {
                why_pt_br: "Containers parados ficam ocupando disco (camadas, logs, volumes anônimos) e confundem quem lê docker ps -a; sobras de deploy blue/green são o caso comum.",
                fix_pt_br: "Confira com docker ps -a --filter status=exited e remova o que não volta mais (docker rm <container> ou docker container prune --filter until=72h). Se o deploy deixa '-previous-*', faça-o limpar as versões antigas.",
                why_en: "Stopped containers keep using disk (layers, logs, anonymous volumes) and clutter docker ps -a; blue/green deploy leftovers are the usual case.",
                fix_en: "List them with docker ps -a --filter status=exited and remove what will not come back (docker rm <container> or docker container prune --filter until=72h). If the deploy leaves '-previous-*' copies, make it clean old versions up.",
            },
            Check::MemoryOvercommitted => Guidance {
                why_pt_br: "A configuração (work_mem x max_connections, shared_buffers, limites de containers, -Xmx) promete mais memória do que o host tem. Só vira warning/critical quando o host também está sem memória (menos de 10% disponível, PSI some >= 10% ou paginação intensa); fora disso é info.",
                fix_pt_br: "Reduza o maior contribuinte: max_connections/work_mem do Postgres (use pgbouncer), limites de containers super dimensionados ou -Xmx; ou aumente a RAM.",
                why_en: "The configuration (work_mem x max_connections, shared_buffers, container limits, -Xmx) promises more memory than the host has. It is a warning/critical only while the host is also short of memory (under 10% available, PSI some >= 10% or heavy paging); otherwise info.",
                fix_en: "Shrink the largest contributor: Postgres max_connections/work_mem (put pgbouncer in front), oversized container limits or -Xmx; or add RAM.",
            },
            Check::SwapoffWouldExceedLimit => Guidance {
                why_pt_br: "O container tem páginas em swap; se o swap do host for desligado (swapoff), ele passa do limite e o kernel mata o processo (OOM). É checagem prévia: info normalmente, warning se o container já está no limite de RAM vivendo do swap.",
                fix_pt_br: "Antes de swapoff, aumente o limite do container (mem_limit) ou reduza o uso dele. Se for warning, aumente o limite ou corrija o consumo agora.",
                why_en: "The container has pages in swap; if the host's swap is cleared (swapoff) it goes over its limit and the kernel kills it (OOM). A pre-flight check: info normally, warning if the container already sits at its RAM limit living on swap.",
                fix_en: "Before swapoff, raise the container's limit (mem_limit) or cut its usage. As a warning, raise the limit or fix the consumption now.",
            },
            _ => return None,
        };
        Some(g)
    }
}

impl std::fmt::Display for Check {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One conclusion about one object on the host.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub check: Check,
    pub severity: AlertSeverity,
    /// The object the finding is about: a unit, a container, a certificate,
    /// `cluster/database`, `address:port`, or `host` for host-wide checks.
    ///
    /// Always an identifier that survives restarts — never a PID, a container
    /// ID or a measured value — so the same problem keeps the same series.
    pub target: String,
    /// The figure behind the finding (days left, ratio, pages/s, count...),
    /// for humans reading the alert. Not part of the identity.
    pub value: f64,
    /// Why this particular finding got its severity, when that is not obvious
    /// from the check alone (e.g. "firewall drops public sources"). English,
    /// for logs and the JSON view. Not part of the identity.
    pub detail: Option<String>,
    /// Why the check matters and how to fix it, in pt-BR and English.
    pub guidance: Option<Guidance>,
}

/// What a finding means and what to do about it.
///
/// Static per check, so it costs nothing per finding and never reaches the
/// Prometheus labels (whose values must stay short and stable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Guidance {
    pub why_pt_br: &'static str,
    pub fix_pt_br: &'static str,
    pub why_en: &'static str,
    pub fix_en: &'static str,
}

/// Everything collected from a host in one pass.
///
/// Each source is `None` when its collector failed or is not available, so a
/// check can tell "nothing found" from "could not look".
#[derive(Debug, Clone, Default)]
pub struct HostSnapshot {
    pub system: Option<SystemInfo>,
    pub disk_policy: crate::operations::DiskPolicy,
    pub scheduled_jobs: Option<Vec<crate::operations::ScheduledJob>>,
    pub containers: Option<Vec<ContainerInfo>>,
    /// `(name, command line)` of running processes, for JVM heap commitment
    pub processes: Vec<(String, String)>,
    pub postgres: Option<Vec<PostgresClusterInfo>>,
    pub mariadb: Option<Vec<MariaDBClusterInfo>>,
    pub systemd_failed: Option<Vec<SystemdFailedUnit>>,
    pub tls: Option<TlsSnapshot>,
    pub ports: Option<Vec<ListeningPortInfo>>,
    /// Packet filter rules, to tell a bound port from an exposed one
    pub firewall: Option<FirewallSnapshot>,
    /// networkd state, read only when a wait-online unit has failed
    pub network: Option<NetworkState>,
}

impl HostSnapshot {
    /// Whether the data a source provides was collected.
    pub fn has(&self, source: Source) -> bool {
        match source {
            Source::System => self.system.is_some(),
            Source::Docker => self.containers.is_some(),
            Source::Postgres => self.postgres.is_some(),
            Source::Mariadb => self.mariadb.is_some(),
            Source::Systemd => self.systemd_failed.is_some(),
            Source::Tls => self.tls.is_some(),
            Source::Ports => self.ports.is_some(),
            Source::Firewall => self.firewall.as_ref().is_some_and(|f| f.complete()),
            Source::Network => self.network.is_some(),
            Source::ScheduledJobs => self.scheduled_jobs.is_some(),
        }
    }

    /// Memory commitment, or `None` when physical memory is unknown.
    pub fn memory_commitment(&self) -> Option<MemoryCommitment> {
        let system = self.system.as_ref()?;
        Some(commitment::assess(
            system.memory_total_bytes,
            self.postgres.as_deref().unwrap_or(&[]),
            self.containers.as_deref().unwrap_or(&[]),
            &self.processes,
        ))
    }
}

/// Databases every postgres cluster carries, idle by nature.
const POSTGRES_SYSTEM_DATABASES: &[&str] = &["postgres", "template0", "template1"];

/// How long a database must have been quiet before idleness means anything.
///
/// Counters reset on `pg_stat_reset()` and on some upgrades; a database that
/// has been idle since this morning's reset is not an orphan.
pub const POSTGRES_IDLE_MIN_DAYS: i64 = 7;

/// Target used for findings that concern the whole host.
pub const HOST_TARGET: &str = "host";

/// Evaluate every check against a snapshot taken at `now`.
///
/// The result is sorted by check, then target, and holds at most one finding
/// per `(check, target)`.
pub fn evaluate(snapshot: &HostSnapshot, now: DateTime<Utc>) -> Vec<Finding> {
    let mut findings = Vec::new();

    if let Some(system) = &snapshot.system {
        system_findings(system, &mut findings);
        for disk in &system.disk_info {
            if let Some(severity) = snapshot.disk_policy.severity(disk) {
                push_detailed(
                    &mut findings,
                    Check::DiskPressure,
                    severity,
                    &disk.mount_point,
                    disk.usage_percent,
                    Some(format!("{} bytes available", disk.available_bytes)),
                );
            }
        }
        if let Some(commitment) = snapshot.memory_commitment() {
            commitment_findings(&commitment, system, &mut findings);
        }
    }
    if let Some(containers) = &snapshot.containers {
        container_findings(containers, &mut findings);
    }
    if let Some(clusters) = &snapshot.postgres {
        postgres_findings(clusters, now, &mut findings);
    }
    if let Some(clusters) = &snapshot.mariadb {
        mariadb_findings(clusters, &mut findings);
    }
    if let Some(failed) = &snapshot.systemd_failed {
        systemd_findings(failed, snapshot.network.as_ref(), &mut findings);
    }
    if let Some(tls) = &snapshot.tls {
        tls_findings(tls, &mut findings);
    }
    if let Some(ports) = &snapshot.ports {
        datastore_findings(ports, snapshot.firewall.as_ref(), &mut findings);
    }

    if let Some(jobs) = &snapshot.scheduled_jobs {
        for job in jobs {
            if !job.collection_up {
                push(
                    &mut findings,
                    Check::ScheduledJobUnavailable,
                    AlertSeverity::Critical,
                    &job.name,
                    1.0,
                );
                continue;
            }
            if !job.timer_active {
                push(
                    &mut findings,
                    Check::ScheduledJobDisabled,
                    AlertSeverity::Critical,
                    &job.name,
                    1.0,
                );
            }
            if job.failed() {
                push_detailed(
                    &mut findings,
                    Check::ScheduledJobFailed,
                    AlertSeverity::Critical,
                    &job.name,
                    job.exit_code as f64,
                    Some(job.result.clone()),
                );
            }
            if job.last_run <= 0 || job.overdue_seconds(now.timestamp()) > 0 {
                push(
                    &mut findings,
                    Check::ScheduledJobOverdue,
                    AlertSeverity::Critical,
                    &job.name,
                    job.overdue_seconds(now.timestamp()).max(0) as f64,
                );
            }
        }
    }
    findings.sort_by(|a, b| (a.check, &a.target).cmp(&(b.check, &b.target)));
    findings.dedup_by(|a, b| a.check == b.check && a.target == b.target);
    findings
}

fn push(findings: &mut Vec<Finding>, check: Check, severity: AlertSeverity, target: &str, v: f64) {
    push_detailed(findings, check, severity, target, v, None);
}

fn push_detailed(
    findings: &mut Vec<Finding>,
    check: Check,
    severity: AlertSeverity,
    target: &str,
    value: f64,
    detail: Option<String>,
) {
    findings.push(Finding {
        check,
        severity,
        target: target.to_string(),
        value,
        detail,
        guidance: check.guidance(),
    });
}

fn system_findings(system: &SystemInfo, findings: &mut Vec<Finding>) {
    let paging = system.swap.activity_pages_per_sec();
    if paging > AlertManager::THRASHING_PAGES_PER_SEC {
        push(
            findings,
            Check::SwapThrashing,
            AlertSeverity::Warning,
            HOST_TARGET,
            paging,
        );
    }

    if let Some(pressure) = &system.memory_pressure {
        if pressure.full_avg60 > AlertManager::PSI_FULL_WARNING_PERCENT {
            push(
                findings,
                Check::MemoryStalled,
                AlertSeverity::Warning,
                HOST_TARGET,
                pressure.full_avg60,
            );
        }
    }
}

/// Share of physical memory still available below which the host counts as
/// short of memory.
pub const MEMORY_STRESS_AVAILABLE_RATIO: f64 = 0.10;

/// PSI `some` avg60 (% of the last minute in which at least one task waited
/// on memory) from which the host counts as short of memory.
pub const MEMORY_STRESS_PSI_SOME_PERCENT: f64 = 10.0;

/// Whether the host shows real memory trouble now, as opposed to a
/// configuration that merely promises a lot.
///
/// Any one of: less than 10% of RAM available, tasks waiting on memory for
/// 10% of the last minute, or paging at the thrashing rate.
pub fn memory_stress(system: &SystemInfo) -> Option<String> {
    if system.memory_total_bytes > 0 {
        let available = system.memory_available_bytes as f64 / system.memory_total_bytes as f64;
        if available < MEMORY_STRESS_AVAILABLE_RATIO {
            return Some(format!("only {:.0}% of RAM available", available * 100.0));
        }
    }
    if let Some(pressure) = &system.memory_pressure {
        if pressure.some_avg60 >= MEMORY_STRESS_PSI_SOME_PERCENT {
            return Some(format!(
                "tasks waited on memory {:.0}% of the last minute",
                pressure.some_avg60
            ));
        }
    }
    let paging = system.swap.activity_pages_per_sec();
    if paging > AlertManager::THRASHING_PAGES_PER_SEC {
        return Some(format!("paging at {paging:.0} pages/s"));
    }
    None
}

/// Overcommit is a promise, not a shortage: `work_mem x max_connections` and
/// container limits are ceilings that are rarely all reached together. On a
/// host with plenty of memory available the ratio is reported as info; it
/// becomes a warning (>= 1.5x) or critical (>= 3x) only while the host is also
/// short of memory, which is when the promise starts being collected.
fn commitment_findings(
    commitment: &MemoryCommitment,
    system: &SystemInfo,
    findings: &mut Vec<Finding>,
) {
    let level_severity = match commitment.level() {
        CommitmentLevel::Ok => return,
        CommitmentLevel::Warning => AlertSeverity::Warning,
        CommitmentLevel::Critical => AlertSeverity::Critical,
    };
    let culprit = commitment
        .top_contributors(1)
        .first()
        .map(|c| format!("; largest: {} {}", c.source, c.basis))
        .unwrap_or_default();
    let (severity, detail) = match memory_stress(system) {
        Some(stress) => (
            level_severity,
            format!(
                "promises {:.2}x physical memory and the host is short of memory ({stress}){culprit}",
                commitment.ratio()
            ),
        ),
        None => (
            AlertSeverity::Info,
            format!(
                "promises {:.2}x physical memory, but memory is not under pressure{culprit}",
                commitment.ratio()
            ),
        ),
    };
    push_detailed(
        findings,
        Check::MemoryOvercommitted,
        severity,
        HOST_TARGET,
        commitment.ratio(),
        Some(detail),
    );
}

/// A failed `systemd-networkd-wait-online` on a host that is online now is a
/// boot-time timeout (typically a link stuck in `configuring` because the
/// provider keeps sending IPv6 router advertisements), not an outage: info.
/// Every other failed unit, and wait-online while the network is down or
/// unknown, stays a warning.
fn systemd_findings(
    failed: &[SystemdFailedUnit],
    network: Option<&NetworkState>,
    findings: &mut Vec<Finding>,
) {
    for unit in failed {
        let benign_wait = network
            .filter(|_| network::is_wait_online_unit(&unit.name))
            .filter(|n| n.is_online());
        match benign_wait {
            Some(state) => push_detailed(
                findings,
                Check::SystemdUnitFailed,
                AlertSeverity::Info,
                &unit.name,
                1.0,
                Some(format!(
                    "timed out at boot, but the network is online now ({})",
                    state.summary()
                )),
            ),
            None => push(
                findings,
                Check::SystemdUnitFailed,
                AlertSeverity::Warning,
                &unit.name,
                1.0,
            ),
        }
    }
}

/// A data store bound to every interface is only exposed if the packet
/// filter lets the internet in:
///
/// - accepted from some public source (including through a Docker-published
///   port, which bypasses ufw): `datastore_exposed`, critical
/// - dropped for every public source: `datastore_bound_all_interfaces`, info
/// - rules unreadable (snapshot missing or stale): `datastore_exposed`,
///   warning, as before: unverified, but not assumed safe
fn datastore_findings(
    ports: &[ListeningPortInfo],
    firewall: Option<&FirewallSnapshot>,
    findings: &mut Vec<Finding>,
) {
    for port in ports.iter().filter(|p| p.is_exposed_datastore()) {
        let target = format!("{}:{}", port.address, port.port);
        let exposure = firewall.and_then(|f| f.exposure(&port.address, port.port));
        let (check, severity, detail) = match exposure {
            Some(Exposure::Public) => (
                Check::DatastoreExposed,
                AlertSeverity::Critical,
                "the firewall accepts new connections from public addresses",
            ),
            Some(Exposure::Filtered) => (
                Check::DatastoreBoundAllInterfaces,
                AlertSeverity::Info,
                "bound to every interface, but the firewall drops public sources",
            ),
            None => (
                Check::DatastoreExposed,
                AlertSeverity::Warning,
                "firewall rules could not be read; exposure not verified",
            ),
        };
        push_detailed(
            findings,
            check,
            severity,
            &target,
            port.port as f64,
            Some(detail.to_string()),
        );
    }
}

fn container_findings(containers: &[ContainerInfo], findings: &mut Vec<Finding>) {
    // Several containers often run the same image; drift is a property of the
    // image, so it is reported once per image rather than once per replica.
    let mut drifted_images = BTreeSet::new();

    let mut stopped = 0usize;

    for container in containers {
        let running = container.state == "running";
        if matches!(container.state.as_str(), "exited" | "dead") {
            stopped += 1;
        }

        if running && !container.memory_limit_set {
            push(
                findings,
                Check::ContainerNoMemoryLimit,
                AlertSeverity::Info,
                &container.name,
                container.memory_usage_bytes as f64,
            );
        }

        // A stopped container keeps the last health state it had; that is
        // history, not a sick application (blue/green leftovers, old jobs)
        if let Some(health) = container.health_detail.as_ref().filter(|_| running) {
            match health.assess() {
                HealthAssessment::Passing => {}
                HealthAssessment::BrokenCheck => push(
                    findings,
                    Check::ContainerHealthcheckBroken,
                    AlertSeverity::Info,
                    &container.name,
                    health.failing_streak as f64,
                ),
                HealthAssessment::FailingNow => push(
                    findings,
                    Check::ContainerUnhealthy,
                    AlertSeverity::Warning,
                    &container.name,
                    health.failing_streak as f64,
                ),
            }
        }

        if container.would_exceed_limit_on_swapoff() {
            swapoff_finding(container, findings);
        }

        if let Some(version) = &container.image_version {
            if version.status == VersionStatus::Drifted {
                let image = if version.repository.is_empty() {
                    version.image_ref.clone()
                } else {
                    format!("{}:{}", version.repository, version.tag)
                };
                drifted_images.insert(image);
            }
        }
    }

    if stopped > 0 {
        push_detailed(
            findings,
            Check::ContainersStopped,
            AlertSeverity::Info,
            HOST_TARGET,
            stopped as f64,
            Some(format!("{stopped} stopped container(s) kept on the host")),
        );
    }

    for image in drifted_images {
        push(
            findings,
            Check::ImageDrift,
            AlertSeverity::Info,
            &image,
            1.0,
        );
    }
}

/// RAM use, as a share of the limit, from which a container that also holds
/// swap is considered pinned at its limit now.
pub const SWAPOFF_PINNED_RATIO: f64 = 0.95;

/// `swapoff_would_exceed_limit` is a pre-flight answer ("do not run swapoff
/// yet"), not an incident: a container with pages in swap and room under its
/// limit is fine until someone clears swap. It is info by default and a
/// warning only when the container is already at >= 95% of its limit in RAM,
/// i.e. it is living on swap now and will meet the OOM killer, swapoff or not.
fn swapoff_finding(container: &ContainerInfo, findings: &mut Vec<Finding>) {
    let swap = container.swap_bytes.unwrap_or(0);
    let demand = container.memory_usage_bytes.saturating_add(swap);
    let over = demand.saturating_sub(container.memory_limit_bytes);
    let pinned = container.memory_limit_bytes > 0
        && container.memory_usage_bytes as f64
            >= container.memory_limit_bytes as f64 * SWAPOFF_PINNED_RATIO;
    let (severity, detail) = if pinned {
        (
            AlertSeverity::Warning,
            "at its memory limit in RAM and holding swap: it is living on swap now",
        )
    } else {
        (
            AlertSeverity::Info,
            "would exceed its limit only if the host's swap were cleared",
        )
    };
    push_detailed(
        findings,
        Check::SwapoffWouldExceedLimit,
        severity,
        &container.name,
        over as f64,
        Some(detail.to_string()),
    );
}

fn postgres_findings(
    clusters: &[PostgresClusterInfo],
    now: DateTime<Utc>,
    findings: &mut Vec<Finding>,
) {
    for cluster in clusters {
        let overrides = cluster.auto_conf_settings().len();
        if overrides > 0 {
            push(
                findings,
                Check::PostgresAlterSystemOverride,
                AlertSeverity::Info,
                &cluster.name,
                overrides as f64,
            );
        }

        for db in &cluster.databases {
            if POSTGRES_SYSTEM_DATABASES.contains(&db.name.as_str()) || !db.is_idle() {
                continue;
            }
            // A missing reset time means the counters run since the cluster
            // was created, which is the longest window there is.
            let quiet_long_enough = db
                .stats_reset_at
                .map(|reset| now - reset >= Duration::days(POSTGRES_IDLE_MIN_DAYS))
                .unwrap_or(true);
            if quiet_long_enough {
                push(
                    findings,
                    Check::PostgresDatabaseIdle,
                    AlertSeverity::Info,
                    &format!("{}/{}", cluster.name, db.name),
                    db.size_bytes as f64,
                );
            }
        }
    }
}

fn mariadb_findings(clusters: &[MariaDBClusterInfo], findings: &mut Vec<Finding>) {
    for cluster in clusters {
        if cluster.has_no_application_schemas() {
            push(
                findings,
                Check::MariadbInstanceEmpty,
                AlertSeverity::Info,
                &cluster.name,
                0.0,
            );
        }
        for schema in cluster.schemas.iter().filter(|s| s.is_empty()) {
            push(
                findings,
                Check::MariadbSchemaEmpty,
                AlertSeverity::Info,
                &format!("{}/{}", cluster.name, schema.name),
                0.0,
            );
        }
    }
}

fn tls_findings(tls: &TlsSnapshot, findings: &mut Vec<Finding>) {
    for cert in &tls.certificates {
        let days = cert.days_until_expiry;
        if cert.orphaned {
            // Not served by anything: its expiry cannot take a site down, so
            // it is reported as a leftover and never as expiring
            push(
                findings,
                Check::TlsCertificateOrphaned,
                AlertSeverity::Info,
                &cert.name,
                days as f64,
            );
            continue;
        }
        if days <= AlertManager::TLS_WARNING_DAYS {
            let severity = if days <= AlertManager::TLS_CRITICAL_DAYS {
                AlertSeverity::Critical
            } else {
                AlertSeverity::Warning
            };
            push(
                findings,
                Check::TlsCertificateExpiring,
                severity,
                &cert.name,
                days as f64,
            );
        }
    }

    if let (Some(unit), Some(status)) = (&tls.renewal_unit_name, &tls.renewal_unit_status) {
        if status.contains("failed") {
            push(
                findings,
                Check::TlsRenewalBroken,
                AlertSeverity::Warning,
                unit,
                tls.certificates.len() as f64,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn now() -> DateTime<Utc> {
        "2026-10-29T12:00:00Z".parse().unwrap()
    }

    fn system(total: u64) -> SystemInfo {
        SystemInfo {
            hostname: "h".into(),
            os: "linux".into(),
            kernel_version: "6".into(),
            uptime_seconds: 1,
            cpu_count: 1,
            cpu_usage_percent: 1.0,
            memory_total_bytes: total,
            memory_used_bytes: 0,
            memory_available_bytes: total,
            disk_info: vec![],
            timestamp: now(),
            swap: SwapInfo::default(),
            memory_pressure: None,
        }
    }

    fn container(name: &str) -> ContainerInfo {
        ContainerInfo {
            id: format!("id-{name}"),
            name: name.into(),
            image: "img".into(),
            status: "Up".into(),
            state: "running".into(),
            health: "none".into(),
            cpu_percent: 0.0,
            memory_usage_bytes: 100,
            memory_limit_bytes: 1000,
            memory_percent: None,
            restart_count: 0,
            network_rx_bytes: 0,
            network_tx_bytes: 0,
            networks: vec![],
            memory_limit_set: true,
            health_detail: None,
            swap_bytes: None,
            image_version: None,
        }
    }

    fn checks(findings: &[Finding]) -> Vec<(&'static str, &str)> {
        findings
            .iter()
            .map(|f| (f.check.as_str(), f.target.as_str()))
            .collect()
    }

    #[test]
    fn scheduled_jobs_distinguish_failure_overdue_disabled_and_unknown() {
        use crate::operations::ScheduledJob;
        let healthy = ScheduledJob {
            name: "backup".into(),
            collection_up: true,
            timer_active: true,
            last_run: now().timestamp() - 60,
            last_success: now().timestamp() - 50,
            max_interval_seconds: 3600,
            result: "success".into(),
            ..Default::default()
        };
        let checks = |job| {
            evaluate(
                &HostSnapshot {
                    scheduled_jobs: Some(vec![job]),
                    ..Default::default()
                },
                now(),
            )
            .into_iter()
            .map(|f| f.check)
            .collect::<Vec<_>>()
        };
        assert!(checks(healthy.clone()).is_empty());
        assert_eq!(
            checks(ScheduledJob {
                result: "timeout".into(),
                ..healthy.clone()
            }),
            vec![Check::ScheduledJobFailed]
        );
        assert_eq!(
            checks(ScheduledJob {
                timer_active: false,
                ..healthy.clone()
            }),
            vec![Check::ScheduledJobDisabled]
        );
        assert_eq!(
            checks(ScheduledJob {
                last_run: now().timestamp() - 3601,
                ..healthy.clone()
            }),
            vec![Check::ScheduledJobOverdue]
        );
        assert_eq!(
            checks(ScheduledJob {
                last_run: 0,
                ..healthy.clone()
            }),
            vec![Check::ScheduledJobOverdue]
        );
        assert_eq!(
            checks(ScheduledJob {
                collection_up: false,
                ..healthy
            }),
            vec![Check::ScheduledJobUnavailable]
        );
    }

    #[test]
    fn empty_snapshot_has_no_findings() {
        assert!(evaluate(&HostSnapshot::default(), now()).is_empty());
    }

    #[test]
    fn check_identifiers_are_unique_and_snake_case() {
        let ids: BTreeSet<&str> = Check::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(ids.len(), Check::ALL.len());
        for id in ids {
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{id}"
            );
        }
    }

    #[test]
    fn source_availability_follows_snapshot() {
        let snapshot = HostSnapshot {
            ports: Some(vec![]),
            ..Default::default()
        };
        assert!(snapshot.has(Source::Ports));
        assert!(!snapshot.has(Source::Docker));
        for check in Check::ALL {
            let _ = check.source().as_str();
        }
    }

    #[test]
    fn exposed_datastore_is_reported_per_bind() {
        let port = |addr: &str, port: u16| ListeningPortInfo {
            address: addr.into(),
            port,
            protocol: "tcp".into(),
            bind_scope: BindScope::classify(addr),
            pid: Some(1),
            process_name: "postgres".into(),
            sensitive: ListeningPortInfo::is_sensitive_port(port),
        };
        let snapshot = HostSnapshot {
            ports: Some(vec![
                port("0.0.0.0", 5432),
                port("127.0.0.1", 6379),
                port("0.0.0.0", 443),
                port("10.0.0.5", 3306),
            ]),
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        assert_eq!(
            checks(&findings),
            vec![("datastore_exposed", "0.0.0.0:5432")]
        );
        assert_eq!(findings[0].severity, AlertSeverity::Warning);
    }

    #[test]
    fn container_checks() {
        let mut no_limit = container("no-limit");
        no_limit.memory_limit_set = false;
        let mut stopped_no_limit = container("stopped");
        stopped_no_limit.memory_limit_set = false;
        stopped_no_limit.state = "exited".into();

        let mut broken = container("broken");
        broken.health_detail = Some(ContainerHealthDetail {
            failing_streak: 10,
            last_output: "curl: not found".into(),
            last_exit_code: 127,
            last_checked_at: None,
        });
        let mut sick = container("sick");
        sick.health_detail = Some(ContainerHealthDetail {
            failing_streak: 3,
            last_output: "HTTP 500".into(),
            last_exit_code: 1,
            last_checked_at: None,
        });
        let mut swapper = container("swapper");
        swapper.memory_usage_bytes = 900;
        swapper.swap_bytes = Some(300);

        let drift = |name: &str| {
            let mut c = container(name);
            c.image_version = Some(ImageVersionInfo {
                image_ref: "nginx:1".into(),
                registry: "docker.io".into(),
                repository: "library/nginx".into(),
                tag: "1".into(),
                local_digest: None,
                remote_digest: None,
                status: VersionStatus::Drifted,
                checked_at: None,
                error: None,
            });
            c
        };

        let snapshot = HostSnapshot {
            containers: Some(vec![
                no_limit,
                stopped_no_limit,
                broken,
                sick,
                swapper,
                drift("web-1"),
                drift("web-2"),
            ]),
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        assert_eq!(
            checks(&findings),
            vec![
                ("container_no_memory_limit", "no-limit"),
                ("container_healthcheck_broken", "broken"),
                ("container_unhealthy", "sick"),
                ("containers_stopped", "host"),
                ("swapoff_would_exceed_limit", "swapper"),
                ("image_drift", "library/nginx:1"),
            ]
        );
        let swap = findings
            .iter()
            .find(|f| f.check == Check::SwapoffWouldExceedLimit)
            .unwrap();
        assert_eq!(swap.value, 200.0);
        // 90% of its limit in RAM: a pre-flight note, not an alert
        assert_eq!(swap.severity, AlertSeverity::Info);
    }

    /// `docker inspect` of a blue/green leftover: stopped, last health
    /// unhealthy. Reduced to the fields the collector maps.
    const INSPECT_EXITED_UNHEALTHY: &str = r#"{
        "Name": "/api-development-previous-f14067e0",
        "State": {
            "Status": "exited",
            "Running": false,
            "ExitCode": 0,
            "Health": {
                "Status": "unhealthy",
                "FailingStreak": 3,
                "Log": [{"ExitCode": 1, "Output": "curl: (7) Failed to connect to localhost port 8080"}]
            }
        }
    }"#;

    #[test]
    fn stopped_container_is_never_unhealthy() {
        let inspect: serde_json::Value = serde_json::from_str(INSPECT_EXITED_UNHEALTHY).unwrap();
        let state = &inspect["State"];
        let health = &state["Health"];
        let mut leftover = container(inspect["Name"].as_str().unwrap().trim_start_matches('/'));
        leftover.state = state["Status"].as_str().unwrap().into();
        leftover.health = health["Status"].as_str().unwrap().into();
        leftover.health_detail = Some(ContainerHealthDetail {
            failing_streak: health["FailingStreak"].as_u64().unwrap() as u32,
            last_output: health["Log"][0]["Output"].as_str().unwrap().into(),
            last_exit_code: health["Log"][0]["ExitCode"].as_i64().unwrap() as i32,
            last_checked_at: None,
        });
        let mut other = leftover.clone();
        other.name = "api-development-previous-9cba2461".into();
        other.state = "dead".into();
        let mut sick = leftover.clone();
        sick.name = "api-development".into();
        sick.state = "running".into();

        let findings = evaluate(
            &HostSnapshot {
                containers: Some(vec![leftover, other, sick]),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(
            checks(&findings),
            vec![
                ("container_unhealthy", "api-development"),
                ("containers_stopped", "host"),
            ]
        );
        let stopped = &findings[1];
        assert_eq!(stopped.severity, AlertSeverity::Info);
        assert_eq!(stopped.value, 2.0, "aggregated: one finding per host");
    }

    #[test]
    fn swapoff_is_a_warning_only_when_the_container_lives_on_swap() {
        let mut pinned = container("pinned");
        pinned.memory_usage_bytes = 990;
        pinned.swap_bytes = Some(500);
        let findings = evaluate(
            &HostSnapshot {
                containers: Some(vec![pinned]),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(findings[0].check, Check::SwapoffWouldExceedLimit);
        assert_eq!(findings[0].severity, AlertSeverity::Warning);
        assert_eq!(findings[0].value, 490.0);
    }

    #[test]
    fn memory_pressure_and_thrashing() {
        let mut info = system(8 << 30);
        info.swap.in_pages_per_sec = 150.0;
        info.swap.out_pages_per_sec = 100.0;
        info.memory_pressure = Some(MemoryPressure {
            some_avg60: 20.0,
            full_avg60: 6.0,
            full_total_seconds: 10.0,
        });
        let findings = evaluate(
            &HostSnapshot {
                system: Some(info.clone()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(
            checks(&findings),
            vec![("swap_thrashing", "host"), ("memory_stalled", "host")]
        );

        info.swap = SwapInfo::default();
        info.memory_pressure = None;
        assert!(evaluate(
            &HostSnapshot {
                system: Some(info),
                ..Default::default()
            },
            now()
        )
        .is_empty());
    }

    #[test]
    fn overcommit_uses_container_limits_and_jvm_heaps() {
        let mut big = container("big");
        big.memory_limit_bytes = 2 << 30;
        let snapshot = HostSnapshot {
            system: Some(system(1 << 30)),
            containers: Some(vec![big]),
            processes: vec![("java".into(), "java -Xmx2g -jar app.jar".into())],
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        let over = findings
            .iter()
            .find(|f| f.check == Check::MemoryOvercommitted)
            .unwrap();
        // Plenty of memory available: the promise is noted, nobody is paged
        assert_eq!(over.severity, AlertSeverity::Info);
        assert_eq!(over.target, HOST_TARGET);
        assert!(over.value >= 4.0);
        assert!(over
            .detail
            .as_deref()
            .unwrap()
            .contains("not under pressure"));

        // Same promise on a host that is short of memory: critical
        let mut short = system(1 << 30);
        short.memory_available_bytes = (1 << 30) / 20;
        let stressed = HostSnapshot {
            system: Some(short),
            ..snapshot.clone()
        };
        let over = evaluate(&stressed, now())
            .into_iter()
            .find(|f| f.check == Check::MemoryOvercommitted)
            .unwrap();
        assert_eq!(over.severity, AlertSeverity::Critical);
        assert!(over.detail.as_deref().unwrap().contains("5% of RAM"));

        let mut modest = container("modest");
        modest.memory_limit_bytes = 1 << 29;
        let fine = HostSnapshot {
            system: Some(system(1 << 30)),
            containers: Some(vec![modest]),
            ..Default::default()
        };
        assert!(evaluate(&fine, now()).is_empty());
    }

    #[test]
    fn memory_stress_criteria() {
        let mut info = system(100);
        assert_eq!(memory_stress(&info), None);
        info.memory_pressure = Some(MemoryPressure {
            some_avg60: 12.0,
            full_avg60: 1.0,
            full_total_seconds: 0.0,
        });
        assert!(memory_stress(&info).unwrap().contains("12%"));
        info.memory_pressure = None;
        info.swap.in_pages_per_sec = 300.0;
        assert!(memory_stress(&info).unwrap().contains("paging"));
        assert_eq!(memory_stress(&system(0)), None);
    }

    fn failed(name: &str) -> SystemdFailedUnit {
        SystemdFailedUnit {
            name: name.into(),
            description: String::new(),
            since: None,
        }
    }

    /// `networkctl list` / `networkctl status` on a host whose uplink stays
    /// "configuring" because of the provider's IPv6 router advertisements.
    const NETWORKCTL_LIST: &str = "  1 lo     loopback  carrier     unmanaged
  2 eth0   ether     routable    configuring
  3 wg0    wireguard routable    unmanaged
";
    const NETWORKCTL_STATUS: &str = "       State: routable
Online state: online
";

    #[test]
    fn wait_online_failure_on_an_online_host_is_info() {
        let online = NetworkState {
            online_state: crate::network::parse_online_state(NETWORKCTL_STATUS),
            links: crate::network::parse_networkctl_list(NETWORKCTL_LIST),
        };
        let units = vec![
            failed("systemd-networkd-wait-online.service"),
            failed("networking.service"),
        ];
        let findings = evaluate(
            &HostSnapshot {
                systemd_failed: Some(units),
                network: Some(online.clone()),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(findings.len(), 2);
        let wait = findings
            .iter()
            .find(|f| f.target == "systemd-networkd-wait-online.service")
            .unwrap();
        assert_eq!(wait.check, Check::SystemdUnitFailed);
        assert_eq!(wait.severity, AlertSeverity::Info);
        assert!(wait
            .detail
            .as_deref()
            .unwrap()
            .contains("eth0 routable (configuring)"));
        // Other units are never hidden
        let other = findings
            .iter()
            .find(|f| f.target == "networking.service")
            .unwrap();
        assert_eq!(other.severity, AlertSeverity::Warning);

        // Offline, or networkd not readable: still a warning
        let offline = NetworkState {
            online_state: Some("offline".into()),
            ..online
        };
        for network in [Some(offline), None] {
            let findings = evaluate(
                &HostSnapshot {
                    systemd_failed: Some(vec![failed("systemd-networkd-wait-online.service")]),
                    network,
                    ..Default::default()
                },
                now(),
            );
            assert_eq!(findings[0].severity, AlertSeverity::Warning);
        }
    }

    fn db_port(addr: &str, port: u16) -> ListeningPortInfo {
        ListeningPortInfo {
            address: addr.into(),
            port,
            protocol: "tcp".into(),
            bind_scope: BindScope::classify(addr),
            pid: None,
            process_name: String::new(),
            sensitive: ListeningPortInfo::is_sensitive_port(port),
        }
    }

    /// ufw with default deny; postgres open to a private network only.
    const IPTABLES_UFW: &str = "*filter
:INPUT DROP [0:0]
:FORWARD DROP [0:0]
:ufw-before-input - [0:0]
:ufw-user-input - [0:0]
-A INPUT -j ufw-before-input
-A ufw-before-input -i lo -j ACCEPT
-A ufw-before-input -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
-A ufw-before-input -j ufw-user-input
-A ufw-user-input -p tcp -m tcp --dport 22 -j ACCEPT
-A ufw-user-input -s 10.20.0.0/24 -p tcp -m tcp --dport 5432 -j ACCEPT
-A ufw-user-input -s 10.20.0.0/24 -p tcp -m tcp --dport 6432 -j ACCEPT
COMMIT
";

    fn firewall(v4: &str, v6: Option<&str>) -> FirewallSnapshot {
        FirewallSnapshot {
            v4: Some(crate::firewall::parse_iptables_save(v4)),
            v6: v6.map(crate::firewall::parse_iptables_save),
            public_ifaces_v4: vec!["eth0".into()],
            public_ifaces_v6: vec!["eth0".into()],
        }
    }

    #[test]
    fn datastore_behind_the_firewall_is_info() {
        let snapshot = HostSnapshot {
            ports: Some(vec![
                db_port("0.0.0.0", 5432),
                db_port("::", 5432),
                db_port("0.0.0.0", 6432),
            ]),
            firewall: Some(firewall(IPTABLES_UFW, Some(IPTABLES_UFW))),
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        assert_eq!(
            checks(&findings),
            vec![
                ("datastore_bound_all_interfaces", "0.0.0.0:5432"),
                ("datastore_bound_all_interfaces", "0.0.0.0:6432"),
                ("datastore_bound_all_interfaces", ":::5432"),
            ]
        );
        assert!(findings.iter().all(|f| f.severity == AlertSeverity::Info));
        assert!(snapshot.has(Source::Firewall));
    }

    #[test]
    fn datastore_open_in_the_firewall_is_critical() {
        let open_v6 = "*filter\n:INPUT ACCEPT [0:0]\nCOMMIT\n";
        let snapshot = HostSnapshot {
            ports: Some(vec![db_port("0.0.0.0", 3306), db_port("::", 3306)]),
            firewall: Some(firewall(
                &IPTABLES_UFW.replace("--dport 22", "--dport 3306"),
                Some(open_v6),
            )),
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        assert_eq!(
            checks(&findings),
            vec![
                ("datastore_exposed", "0.0.0.0:3306"),
                ("datastore_exposed", ":::3306")
            ]
        );
        assert!(findings
            .iter()
            .all(|f| f.severity == AlertSeverity::Critical));
    }

    #[test]
    fn docker_published_datastore_is_exposed_despite_ufw() {
        let docker = format!(
            "{}*nat\n:PREROUTING ACCEPT [0:0]\n:DOCKER - [0:0]\n-A PREROUTING -m addrtype --dst-type LOCAL -j DOCKER\n-A DOCKER ! -i br-0a1b2c -p tcp -m tcp --dport 5432 -j DNAT --to-destination 172.18.0.2:5432\nCOMMIT\n",
            IPTABLES_UFW.replace(":FORWARD DROP [0:0]", ":FORWARD ACCEPT [0:0]")
        );
        let snapshot = HostSnapshot {
            ports: Some(vec![db_port("0.0.0.0", 5432)]),
            firewall: Some(firewall(&docker, None)),
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        assert_eq!(
            checks(&findings),
            vec![("datastore_exposed", "0.0.0.0:5432")]
        );
        assert_eq!(findings[0].severity, AlertSeverity::Critical);
    }

    #[test]
    fn unreadable_firewall_keeps_the_unverified_warning() {
        let partial = HostSnapshot {
            ports: Some(vec![db_port("::", 5432)]),
            // IPv4 readable, IPv6 not: a dual-stack socket cannot be judged
            firewall: Some(firewall(IPTABLES_UFW, None)),
            ..Default::default()
        };
        let findings = evaluate(&partial, now());
        assert_eq!(checks(&findings), vec![("datastore_exposed", ":::5432")]);
        assert_eq!(findings[0].severity, AlertSeverity::Warning);
        assert!(findings[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("not verified"));
        assert!(!partial.has(Source::Firewall));
    }

    #[test]
    fn guidance_covers_the_refined_checks_in_both_languages() {
        for check in [
            Check::DatastoreExposed,
            Check::DatastoreBoundAllInterfaces,
            Check::SystemdUnitFailed,
            Check::ContainerUnhealthy,
            Check::ContainersStopped,
            Check::MemoryOvercommitted,
            Check::SwapoffWouldExceedLimit,
        ] {
            let g = check.guidance().unwrap_or_else(|| panic!("{check}"));
            for text in [g.why_pt_br, g.fix_pt_br, g.why_en, g.fix_en] {
                assert!(text.len() > 40, "{check}: {text}");
            }
        }
        assert!(Check::ImageDrift.guidance().is_none());

        let findings = evaluate(
            &HostSnapshot {
                ports: Some(vec![db_port("0.0.0.0", 5432)]),
                ..Default::default()
            },
            now(),
        );
        assert_eq!(findings[0].guidance, Check::DatastoreExposed.guidance());
        let json = serde_json::to_value(findings[0].guidance.unwrap()).unwrap();
        assert!(json["fix_pt_br"].as_str().unwrap().contains("firewall"));
    }

    #[test]
    fn systemd_and_tls() {
        let cert = |name: &str, days: i64, orphaned: bool| TlsCertificateInfo {
            name: name.into(),
            domains: vec![],
            issuer: "LE".into(),
            not_after: None,
            days_until_expiry: days,
            source: "/etc/letsencrypt".into(),
            orphaned,
        };
        let snapshot = HostSnapshot {
            systemd_failed: Some(vec![SystemdFailedUnit {
                name: "backup.service".into(),
                description: "Backup".into(),
                since: None,
            }]),
            tls: Some(TlsSnapshot {
                certificates: vec![
                    cert("ok.example", 80, false),
                    cert("soon.example", 15, false),
                    cert("burning.example", 3, false),
                    cert("dead.example", -2, true),
                ],
                renewal_unit_name: Some("certbot.service".into()),
                renewal_unit_status: Some("failed".into()),
            }),
            ..Default::default()
        };
        let findings = evaluate(&snapshot, now());
        assert_eq!(
            checks(&findings),
            vec![
                ("systemd_unit_failed", "backup.service"),
                ("tls_certificate_expiring", "burning.example"),
                ("tls_certificate_expiring", "soon.example"),
                ("tls_renewal_broken", "certbot.service"),
                ("tls_certificate_orphaned", "dead.example"),
            ]
        );
        assert_eq!(findings[1].severity, AlertSeverity::Critical);
        assert_eq!(findings[2].severity, AlertSeverity::Warning);
        // An expired orphan is a leftover, never an expiry alert
        let orphan = findings
            .iter()
            .find(|f| f.target == "dead.example")
            .unwrap();
        assert_eq!(orphan.severity, AlertSeverity::Info);
    }

    #[test]
    fn database_checks() {
        let db = |name: &str, tx: u64, reset: Option<DateTime<Utc>>| PostgresDatabaseInfo {
            name: name.into(),
            size_bytes: 42,
            num_backends: 0,
            cache_hit_ratio: 0.0,
            transactions: tx,
            stats_reset_at: reset,
        };
        let pg = PostgresClusterInfo {
            name: "main".into(),
            host: "localhost".into(),
            port: 5432,
            databases: vec![
                db("postgres", 0, None),
                db("live", 10, None),
                db("orphan", 0, None),
                db("just_reset", 0, Some(now() - Duration::hours(2))),
                db("old_reset", 0, Some(now() - Duration::days(30))),
            ],
            connections_total: 0,
            connections_by_state: vec![],
            cache_hit_ratio: 0.0,
            top_queries: vec![],
            timestamp: now(),
            settings: vec![PostgresSetting {
                name: "work_mem".into(),
                value: "4096".into(),
                unit: Some("kB".into()),
                source: "configuration file".into(),
                source_file: Some("/var/lib/pg/postgresql.auto.conf".into()),
                source_line: Some(3),
            }],
        };
        let maria = |name: &str, schemas: Vec<MariaDBSchemaInfo>| MariaDBClusterInfo {
            name: name.into(),
            host: "localhost".into(),
            port: 3306,
            schemas,
            connections_active: 0,
            connections_total: 0,
            innodb_status: None,
            processes: vec![],
            timestamp: now(),
        };
        let schema = |name: &str, tables: u32| MariaDBSchemaInfo {
            name: name.into(),
            size_bytes: 0,
            table_count: tables,
            last_write_at: None,
            write_time_available: false,
        };
        let snapshot = HostSnapshot {
            postgres: Some(vec![pg]),
            mariadb: Some(vec![
                maria("empty", vec![]),
                maria("app", vec![schema("shop", 12), schema("ghost", 0)]),
            ]),
            ..Default::default()
        };
        assert_eq!(
            checks(&evaluate(&snapshot, now())),
            vec![
                ("postgres_alter_system_override", "main"),
                ("postgres_database_idle", "main/old_reset"),
                ("postgres_database_idle", "main/orphan"),
                ("mariadb_instance_empty", "empty"),
                ("mariadb_schema_empty", "app/ghost"),
            ]
        );
    }
}
