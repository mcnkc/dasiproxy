use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

struct Counters {
    started: Instant,
    total_requests: AtomicU64,
    completed_requests: AtomicU64,
    active_requests: AtomicU64,
    ai_bot_blocks: AtomicU64,
    total_response_micros: AtomicU64,
}

static COUNTERS: OnceLock<Counters> = OnceLock::new();

fn counters() -> &'static Counters {
    COUNTERS.get_or_init(|| Counters {
        started: Instant::now(),
        total_requests: AtomicU64::new(0),
        completed_requests: AtomicU64::new(0),
        active_requests: AtomicU64::new(0),
        ai_bot_blocks: AtomicU64::new(0),
        total_response_micros: AtomicU64::new(0),
    })
}

pub struct RequestGuard {
    started: Instant,
}

pub fn begin_request() -> RequestGuard {
    let metrics = counters();
    metrics.total_requests.fetch_add(1, Ordering::Relaxed);
    metrics.active_requests.fetch_add(1, Ordering::Relaxed);
    RequestGuard { started: Instant::now() }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        let metrics = counters();
        metrics.completed_requests.fetch_add(1, Ordering::Relaxed);
        metrics.total_response_micros.fetch_add(
            self.started.elapsed().as_micros().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        metrics.active_requests.fetch_sub(1, Ordering::Relaxed);
    }
}

pub fn record_ai_bot_block() {
    counters().ai_bot_blocks.fetch_add(1, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub uptime_seconds: u64,
    pub total_requests: u64,
    pub completed_requests: u64,
    pub active_requests: u64,
    pub ai_bot_blocks: u64,
    pub average_response_ms: u64,
}

pub fn snapshot() -> Snapshot {
    let metrics = counters();
    let total = metrics.completed_requests.load(Ordering::Relaxed);
    let elapsed = metrics.total_response_micros.load(Ordering::Relaxed);
    Snapshot {
        uptime_seconds: metrics.started.elapsed().as_secs(),
        total_requests: metrics.total_requests.load(Ordering::Relaxed),
        completed_requests: total,
        active_requests: metrics.active_requests.load(Ordering::Relaxed),
        ai_bot_blocks: metrics.ai_bot_blocks.load(Ordering::Relaxed),
        average_response_ms: if total == 0 { 0 } else { elapsed / total / 1_000 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_request_lifecycle() {
        let before = snapshot().total_requests;
        {
            let _request = begin_request();
            assert!(snapshot().active_requests >= 1);
        }
        assert!(snapshot().total_requests >= before + 1);
    }
}
