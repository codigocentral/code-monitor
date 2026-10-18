//! Scripted collaborator for gRPC service units. SystemMonitor itself remains
//! compiled and is covered separately against SDK doubles.
use anyhow::Result;
use shared::types::*;
use std::{
    any::Any,
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct State {
    pub replies: VecDeque<(&'static str, Result<Box<dyn Any + Send>>)>,
    pub calls: Vec<&'static str>,
    pub process_requests: Vec<(u32, Option<String>)>,
    pub starts: usize,
    pub tls_dirs: Vec<Vec<String>>,
    pub stops: usize,
}
pub type SharedState = Arc<Mutex<State>>;
impl State {
    pub fn respond<T: Send + 'static>(&mut self, name: &'static str, value: T) {
        self.replies.push_back((name, Ok(Box::new(value))));
    }
    pub fn reject(&mut self, name: &'static str) {
        self.replies
            .push_back((name, Err(anyhow::anyhow!("unit monitor refused {name}"))));
    }
}
thread_local! {static NEXT:RefCell<VecDeque<Result<SharedState>>>=const{RefCell::new(VecDeque::new())};}
pub fn next(state: SharedState) {
    NEXT.with(|q| q.borrow_mut().push_back(Ok(state)));
}
pub struct SystemMonitor(pub SharedState);
impl SystemMonitor {
    pub async fn new(
        _: u64,
        _: Vec<crate::config::PostgresClusterConfig>,
        _: Vec<crate::config::MariaDBClusterConfig>,
        _: Vec<String>,
    ) -> Result<Self> {
        Ok(Self(
            NEXT.with(|q| q.borrow_mut().pop_front())
                .unwrap_or_else(|| Ok(Default::default()))?,
        ))
    }
    pub fn with_tls_dirs(self, dirs: Vec<String>) -> Self {
        self.0.lock().unwrap().tls_dirs.push(dirs);
        self
    }
    pub fn start_background_monitoring(&self) {
        self.0.lock().unwrap().starts += 1;
    }
    pub fn stop_background_monitoring(&self) {
        self.0.lock().unwrap().stops += 1;
    }
    fn reply<T: Send + 'static>(&self, name: &'static str) -> Result<T> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(name);
        let (expected, reply) = state
            .replies
            .pop_front()
            .expect("unexpected monitor request");
        assert_eq!(expected, name);
        Ok(*reply?.downcast::<T>().expect("monitor response type"))
    }
    pub fn get_system_info(&self) -> Result<SystemInfo> {
        self.reply("system")
    }
    pub fn get_processes(&self, limit: u32, filter: Option<String>) -> Result<Vec<ProcessInfo>> {
        self.0
            .lock()
            .unwrap()
            .process_requests
            .push((limit, filter));
        self.reply("processes")
    }
    pub fn get_services(&self) -> Result<Vec<ServiceInfo>> {
        self.reply("services")
    }
    pub fn get_network_info(&self) -> Result<Vec<NetworkInfo>> {
        self.reply("networks")
    }
    pub async fn get_containers(&self) -> Result<Vec<ContainerInfo>> {
        self.reply("containers")
    }
    pub async fn get_postgres_clusters(&self) -> Result<Vec<PostgresClusterInfo>> {
        self.reply("postgres")
    }
    pub async fn get_mariadb_clusters(&self) -> Result<Vec<MariaDBClusterInfo>> {
        self.reply("mariadb")
    }
    pub async fn get_systemd_units(&self) -> Result<Vec<SystemdUnitInfo>> {
        self.reply("systemd")
    }
    pub async fn get_systemd_failed_units(&self) -> Result<Vec<SystemdFailedUnit>> {
        self.reply("failed")
    }
    pub async fn get_tls_info(&self) -> Result<TlsSnapshot> {
        self.reply("tls")
    }
    pub async fn get_listening_ports(&self) -> Result<Vec<ListeningPortInfo>> {
        self.reply("ports")
    }
}

pub fn next_error() {
    NEXT.with(|q| {
        q.borrow_mut()
            .push_back(Err(anyhow::anyhow!("unit monitor constructor refused")))
    });
}
