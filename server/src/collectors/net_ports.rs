//! Listening socket collector
//!
//! Reports which services listen on which interfaces. On a host with a public
//! address, a database bound to `0.0.0.0` is an open door that nobody notices,
//! because the service works perfectly either way.
//!
//! Reads `/proc/net/tcp` directly rather than shelling out to `ss` or
//! `netstat`, neither of which is guaranteed to exist on a minimal image.
//!
//! Not implemented yet: this is the passthrough that keeps the contract, the
//! transport and the UI wiring in place so the collection itself lands as a
//! self-contained change.

use anyhow::Result;
use shared::types::ListeningPortInfo;

/// Collector for sockets in the listening state
pub struct NetPortsCollector {
    /// Root to read `net/tcp` from, overridable for testing
    #[allow(dead_code)]
    proc_root: String,
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
    /// Returns an empty list until the collection is implemented. Callers must
    /// treat that as "nothing known", never as "nothing exposed".
    pub async fn collect(&self) -> Result<Vec<ListeningPortInfo>> {
        Ok(Vec::new())
    }
}

impl Default for NetPortsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_net_ports_collector_returns_empty() {
        let collector = NetPortsCollector::default();
        assert!(collector.collect().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_net_ports_collector_never_errors_on_missing_proc() {
        let collector = NetPortsCollector::with_proc_root("/nonexistent/proc");
        assert!(collector.collect().await.is_ok());
    }
}
