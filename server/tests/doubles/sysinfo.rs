//! Fixture-backed sysinfo SDK. It never samples the host or process table.
pub use ::sysinfo::{MacAddr, Pid, PidExt, ProcessStatus};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
thread_local! {static NEXT:RefCell<VecDeque<System>>=const{RefCell::new(VecDeque::new())};}
pub fn next(system: System) {
    NEXT.with(|queue| queue.borrow_mut().push_back(system));
}
#[derive(Clone, Default)]
pub struct Cpu {
    pub usage: f32,
}
pub trait CpuExt {
    fn cpu_usage(&self) -> f32;
}
impl CpuExt for Cpu {
    fn cpu_usage(&self) -> f32 {
        self.usage
    }
}
#[derive(Clone, Default)]
pub struct Disk {
    pub total: u64,
    pub available: u64,
    pub name: OsString,
    pub mount: PathBuf,
    pub filesystem: Vec<u8>,
}
pub trait DiskExt {
    fn total_space(&self) -> u64;
    fn available_space(&self) -> u64;
    fn name(&self) -> &OsStr;
    fn mount_point(&self) -> &Path;
    fn file_system(&self) -> &[u8];
}
impl DiskExt for Disk {
    fn total_space(&self) -> u64 {
        self.total
    }
    fn available_space(&self) -> u64 {
        self.available
    }
    fn name(&self) -> &OsStr {
        &self.name
    }
    fn mount_point(&self) -> &Path {
        &self.mount
    }
    fn file_system(&self) -> &[u8] {
        &self.filesystem
    }
}
#[derive(Clone)]
pub struct Process {
    pub cpu: f32,
    pub memory: u64,
    pub name: String,
    pub command: Vec<String>,
    pub runtime: u64,
    pub user: Option<u32>,
    pub status: ProcessStatus,
}
impl Default for Process {
    fn default() -> Self {
        Self {
            cpu: 0.,
            memory: 0,
            name: String::new(),
            command: vec![],
            runtime: 0,
            user: None,
            status: ProcessStatus::Run,
        }
    }
}
pub trait ProcessExt {
    fn cpu_usage(&self) -> f32;
    fn memory(&self) -> u64;
    fn name(&self) -> &str;
    fn cmd(&self) -> &[String];
    fn run_time(&self) -> u64;
    fn user_id(&self) -> Option<&u32>;
    fn status(&self) -> ProcessStatus;
}
impl ProcessExt for Process {
    fn cpu_usage(&self) -> f32 {
        self.cpu
    }
    fn memory(&self) -> u64 {
        self.memory
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn cmd(&self) -> &[String] {
        &self.command
    }
    fn run_time(&self) -> u64 {
        self.runtime
    }
    fn user_id(&self) -> Option<&u32> {
        self.user.as_ref()
    }
    fn status(&self) -> ProcessStatus {
        self.status
    }
}
#[derive(Clone, Default)]
pub struct Network {
    pub mac: [u8; 6],
    pub received: u64,
    pub sent: u64,
    pub received_packets: u64,
    pub sent_packets: u64,
}
pub trait NetworkExt {
    fn mac_address(&self) -> MacAddr;
    fn total_received(&self) -> u64;
    fn total_transmitted(&self) -> u64;
    fn total_packets_received(&self) -> u64;
    fn total_packets_transmitted(&self) -> u64;
}
impl NetworkExt for Network {
    fn mac_address(&self) -> MacAddr {
        MacAddr(self.mac)
    }
    fn total_received(&self) -> u64 {
        self.received
    }
    fn total_transmitted(&self) -> u64 {
        self.sent
    }
    fn total_packets_received(&self) -> u64 {
        self.received_packets
    }
    fn total_packets_transmitted(&self) -> u64 {
        self.sent_packets
    }
}
#[derive(Clone, Default)]
pub struct Networks(pub HashMap<String, Network>);
pub trait NetworksExt {
    fn iter(&self) -> std::collections::hash_map::Iter<'_, String, Network>;
}
impl NetworksExt for Networks {
    fn iter(&self) -> std::collections::hash_map::Iter<'_, String, Network> {
        self.0.iter()
    }
}
#[derive(Clone, Default)]
pub struct System {
    pub total_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub disks: Vec<Disk>,
    pub cpu: Cpu,
    pub host: Option<String>,
    pub os: Option<String>,
    pub version: Option<String>,
    pub kernel: Option<String>,
    pub uptime: u64,
    pub cores: Option<usize>,
    pub processes: HashMap<Pid, Process>,
    pub networks: Networks,
    pub refreshes: Arc<AtomicUsize>,
}
pub trait SystemExt: Sized {
    fn new_all() -> Self;
    fn refresh_all(&mut self);
    fn total_memory(&self) -> u64;
    fn used_memory(&self) -> u64;
    fn total_swap(&self) -> u64;
    fn used_swap(&self) -> u64;
    fn disks(&self) -> &[Disk];
    fn global_cpu_info(&self) -> &Cpu;
    fn host_name(&self) -> Option<String>;
    fn name(&self) -> Option<String>;
    fn long_os_version(&self) -> Option<String>;
    fn kernel_version(&self) -> Option<String>;
    fn uptime(&self) -> u64;
    fn physical_core_count(&self) -> Option<usize>;
    fn processes(&self) -> &HashMap<Pid, Process>;
    fn networks(&self) -> &Networks;
}
impl SystemExt for System {
    fn new_all() -> Self {
        NEXT.with(|queue| queue.borrow_mut().pop_front())
            .unwrap_or_default()
    }
    fn refresh_all(&mut self) {
        self.refreshes.fetch_add(1, Ordering::Relaxed);
    }
    fn total_memory(&self) -> u64 {
        self.total_memory
    }
    fn used_memory(&self) -> u64 {
        self.used_memory
    }
    fn total_swap(&self) -> u64 {
        self.total_swap
    }
    fn used_swap(&self) -> u64 {
        self.used_swap
    }
    fn disks(&self) -> &[Disk] {
        &self.disks
    }
    fn global_cpu_info(&self) -> &Cpu {
        &self.cpu
    }
    fn host_name(&self) -> Option<String> {
        self.host.clone()
    }
    fn name(&self) -> Option<String> {
        self.os.clone()
    }
    fn long_os_version(&self) -> Option<String> {
        self.version.clone()
    }
    fn kernel_version(&self) -> Option<String> {
        self.kernel.clone()
    }
    fn uptime(&self) -> u64 {
        self.uptime
    }
    fn physical_core_count(&self) -> Option<usize> {
        self.cores
    }
    fn processes(&self) -> &HashMap<Pid, Process> {
        &self.processes
    }
    fn networks(&self) -> &Networks {
        &self.networks
    }
}

static OFFSET: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
pub struct ClockGuard;
pub fn clock() -> ClockGuard {
    OFFSET.store(0, Ordering::SeqCst);
    ClockGuard
}
impl Drop for ClockGuard {
    fn drop(&mut self) {
        OFFSET.store(0, Ordering::SeqCst);
    }
}
pub fn advance(seconds: i64) {
    OFFSET.fetch_add(seconds, Ordering::SeqCst);
}
pub fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000 + OFFSET.load(Ordering::SeqCst), 0).unwrap()
}
