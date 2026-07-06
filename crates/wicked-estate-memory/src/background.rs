//! Background consolidation loop (issue #6 / AC-8).
//!
//! **Concurrency model: single logical writer.** The engine is synchronous and lives behind the host's
//! `Arc<Mutex<MemoryEngine>>`; all writers and readers serialize through that mutex. This loop runs
//! consolidation on a host thread and holds the lock **only for each batch**, releasing it between
//! rounds (and sleeping unlocked), so recalls interleave and are not starved. True parallel reads
//! would need a read-replica/connection-pool (a documented scale-up, not built here) — the
//! single-writer model is the deliberate choice, and `recalls_not_starved_during_loop` proves it
//! keeps readers responsive.
//!
//! The loop is restartable (spawn again) and idempotent (consolidation produces no new facts once a
//! scope is distilled). `now` is injected because the core stays clock-free.

use crate::MemoryEngine;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use wicked_estate_memory_core::Scope;

/// Handle to a running background consolidation loop. Call [`ConsolidationHandle::stop`] (or drop it)
/// to end the loop and join the thread.
pub struct ConsolidationHandle {
    stop: Arc<AtomicBool>,
    rounds: Arc<AtomicU64>,
    handle: Option<JoinHandle<()>>,
}

impl ConsolidationHandle {
    /// Signal the loop to stop, join its thread, and return how many rounds it completed.
    pub fn stop(mut self) -> u64 {
        self.signal_and_join();
        self.rounds.load(Ordering::Relaxed)
    }

    /// Rounds completed so far (without stopping).
    pub fn rounds(&self) -> u64 {
        self.rounds.load(Ordering::Relaxed)
    }

    fn signal_and_join(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for ConsolidationHandle {
    fn drop(&mut self) {
        self.signal_and_join();
    }
}

/// Spawn a background loop that consolidates `scope` every `interval`. Holds the engine lock only per
/// batch (releases between rounds so readers interleave). Survives a transient lock-poison rather than
/// aborting. `now` supplies the clock.
pub fn spawn_consolidation<F>(
    engine: Arc<Mutex<MemoryEngine>>,
    scope: Scope,
    interval: Duration,
    now: F,
) -> ConsolidationHandle
where
    F: Fn() -> i64 + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let rounds = Arc::new(AtomicU64::new(0));
    let (s, r) = (stop.clone(), rounds.clone());
    let handle = std::thread::spawn(move || {
        while !s.load(Ordering::Relaxed) {
            {
                // Lock ONLY for the batch; on poison, skip this round so the loop survives.
                if let Ok(mut eng) = engine.lock() {
                    let _ = eng.consolidate(&scope, now());
                }
            } // lock released here — readers/writers interleave between rounds
            r.fetch_add(1, Ordering::Relaxed);
            // Sleep the interval in small slices so stop() stays responsive.
            let mut slept = Duration::ZERO;
            while slept < interval && !s.load(Ordering::Relaxed) {
                let step = Duration::from_millis(20).min(interval - slept);
                std::thread::sleep(step);
                slept += step;
            }
        }
    });
    ConsolidationHandle {
        stop,
        rounds,
        handle: Some(handle),
    }
}
