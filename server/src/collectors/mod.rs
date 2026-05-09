//! Collectors module for gathering system and application metrics
//!
//! This module provides traits and implementations for collecting
//! various types of metrics from the host system.

pub mod docker;
pub mod mariadb;
pub mod postgres;
pub mod systemd;

use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Trait for metric collectors
#[allow(dead_code)]
#[async_trait]
pub trait Collector: Send + Sync {
    /// Name of the collector
    fn name(&self) -> &'static str;

    /// Whether this collector is enabled
    fn is_enabled(&self) -> bool;

    /// Collect metrics
    async fn collect(&self) -> Result<()>;

    /// Update interval for this collector
    fn interval(&self) -> Duration {
        Duration::from_secs(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockCollector;

    #[async_trait]
    impl Collector for MockCollector {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn is_enabled(&self) -> bool {
            true
        }

        async fn collect(&self) -> Result<()> {
            Ok(())
        }

        // Use default interval() implementation
    }

    #[test]
    fn test_collector_default_interval() {
        let collector = MockCollector;
        assert_eq!(collector.interval(), Duration::from_secs(5));
    }
}
