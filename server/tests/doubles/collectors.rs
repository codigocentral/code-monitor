//! Collector collaborators for SystemMonitor units. Real collectors are tested
//! separately; these objects only capture calls and return scripted data.
use anyhow::Result;
use shared::types::*;
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct State {
    pub calls: Vec<String>,
    pub postgres_names: Vec<String>,
    pub mariadb_names: Vec<String>,
    pub configured_units: Vec<Vec<String>>,
    pub tls_dirs: Vec<Vec<String>>,
    pub containers: VecDeque<Result<Vec<ContainerInfo>>>,
    pub postgres: VecDeque<Result<PostgresClusterInfo>>,
    pub mariadb: VecDeque<Result<MariaDBClusterInfo>>,
    pub units: VecDeque<Result<Vec<SystemdUnitInfo>>>,
    pub failed: VecDeque<Result<Vec<SystemdFailedUnit>>>,
    pub tls: VecDeque<Result<TlsSnapshot>>,
    pub ports: VecDeque<Result<Vec<ListeningPortInfo>>>,
    pub statuses: HashMap<String, Option<String>>,
    pub pressure: Option<MemoryPressure>,
    pub process_swap: HashMap<u32, u64>,
    pub container_swap: HashMap<String, u64>,
    pub seen_container_swap: Vec<HashMap<String, u64>>,
    pub paging: VecDeque<Option<crate::collectors::memory::PagingCounters>>,
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! {static CURRENT:RefCell<SharedState>=RefCell::new(Arc::new(Mutex::new(State::default())));}
pub struct Guard(SharedState);
pub fn scope(state: SharedState) -> Guard {
    Guard(CURRENT.with(|current| current.replace(state)))
}
impl Drop for Guard {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            current.replace(self.0.clone());
        });
    }
}
fn current() -> SharedState {
    CURRENT.with(|current| current.borrow().clone())
}
#[derive(Clone)]
pub struct DockerCollector(SharedState);
impl DockerCollector {
    pub fn new() -> Self {
        Self(current())
    }
    pub fn new_none() -> Self {
        Self(current())
    }
    pub async fn collect_containers(
        &self,
        swap: &HashMap<String, u64>,
    ) -> Result<Vec<ContainerInfo>> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("containers".into());
        state.seen_container_swap.push(swap.clone());
        state.containers.pop_front().unwrap_or_else(|| Ok(vec![]))
    }
}
pub struct PostgresCollector(SharedState);
impl PostgresCollector {
    pub fn new(config: crate::config::PostgresClusterConfig) -> Self {
        let state = current();
        state.lock().unwrap().postgres_names.push(config.name);
        Self(state)
    }
    pub async fn collect(&self) -> Result<PostgresClusterInfo> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("postgres".into());
        state
            .postgres
            .pop_front()
            .unwrap_or_else(|| Err(anyhow::anyhow!("unit postgres unconfigured")))
    }
}
pub struct MariaDBCollector(SharedState);
impl MariaDBCollector {
    pub fn new(config: crate::config::MariaDBClusterConfig) -> Self {
        let state = current();
        state.lock().unwrap().mariadb_names.push(config.name);
        Self(state)
    }
    pub async fn collect(&self) -> Result<MariaDBClusterInfo> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("mariadb".into());
        state
            .mariadb
            .pop_front()
            .unwrap_or_else(|| Err(anyhow::anyhow!("unit MariaDB unconfigured")))
    }
}
pub struct SystemdCollector(SharedState);
impl SystemdCollector {
    pub fn new(units: Vec<String>) -> Self {
        let state = current();
        state.lock().unwrap().configured_units.push(units);
        Self(state)
    }
    pub async fn collect(&self) -> Result<Vec<SystemdUnitInfo>> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("systemd".into());
        state.units.pop_front().unwrap_or_else(|| Ok(vec![]))
    }
    pub async fn collect_failed(&self) -> Result<Vec<SystemdFailedUnit>> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("systemd-failed".into());
        state.failed.pop_front().unwrap_or_else(|| Ok(vec![]))
    }
    pub async fn unit_status(&self, unit: &str) -> Option<String> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(format!("status:{unit}"));
        state.statuses.get(unit).cloned().flatten()
    }
}
pub struct TlsCollector(SharedState);
impl Default for TlsCollector {
    fn default() -> Self {
        Self::new(vec![])
    }
}
impl TlsCollector {
    pub fn new(dirs: Vec<String>) -> Self {
        let state = current();
        state.lock().unwrap().tls_dirs.push(dirs);
        Self(state)
    }
    pub async fn collect(&self) -> Result<TlsSnapshot> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("tls".into());
        state.tls.pop_front().unwrap_or_else(|| {
            Ok(TlsSnapshot {
                certificates: vec![],
                renewal_unit_name: None,
                renewal_unit_status: None,
            })
        })
    }
}
pub struct NetPortsCollector(SharedState);
impl Default for NetPortsCollector {
    fn default() -> Self {
        Self(current())
    }
}
impl NetPortsCollector {
    pub fn with_proc_root(_root: &str) -> Self {
        Self(current())
    }
    pub async fn collect(&self) -> Result<Vec<ListeningPortInfo>> {
        let mut state = self.0.lock().unwrap();
        state.calls.push("ports".into());
        state.ports.pop_front().unwrap_or_else(|| Ok(vec![]))
    }
}
#[derive(Clone)]
pub struct MemoryCollector(SharedState);
impl Default for MemoryCollector {
    fn default() -> Self {
        Self::new()
    }
}
impl MemoryCollector {
    pub fn new() -> Self {
        Self(current())
    }
    pub fn with_proc_root(_root: &str) -> Self {
        Self(current())
    }
    pub fn pressure(&self) -> Option<MemoryPressure> {
        self.0.lock().unwrap().pressure.clone()
    }
    pub fn process_swap(&self, pid: u32) -> Option<u64> {
        self.0.lock().unwrap().process_swap.get(&pid).copied()
    }
    pub fn swap_by_container(&self) -> HashMap<String, u64> {
        self.0.lock().unwrap().container_swap.clone()
    }
    pub fn paging_counters(&self) -> Option<crate::collectors::memory::PagingCounters> {
        self.0.lock().unwrap().paging.pop_front().flatten()
    }
}
