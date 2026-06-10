//! Global Prometheus-style counters for the gRPC service
//!
//! Exposed by the /metrics endpoint of the health HTTP server.

use std::sync::atomic::{AtomicU64, Ordering};

/// Total number of authenticated gRPC requests handled
pub static GRPC_REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Total number of requests rejected due to invalid/missing credentials
pub static GRPC_AUTH_FAILURES_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Total number of streaming connections rejected by the max_clients limit
pub static GRPC_STREAMS_REJECTED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Currently open streaming connections (gauge)
pub static GRPC_ACTIVE_STREAMS: AtomicU64 = AtomicU64::new(0);

pub fn inc(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn dec(counter: &AtomicU64) {
    counter.fetch_sub(1, Ordering::Relaxed);
}

pub fn get(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counters_inc_dec() {
        let before = get(&GRPC_ACTIVE_STREAMS);
        inc(&GRPC_ACTIVE_STREAMS);
        assert_eq!(get(&GRPC_ACTIVE_STREAMS), before + 1);
        dec(&GRPC_ACTIVE_STREAMS);
        assert_eq!(get(&GRPC_ACTIVE_STREAMS), before);
    }
}
