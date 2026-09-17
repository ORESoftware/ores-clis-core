use crate::shutdown::ShutdownAction;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

const RUNNING: u8 = 0;
const DRAINING: u8 = 1;
const FORCED: u8 = 2;

/// Monotonic process-shutdown intent shared by long-running CLI consumers.
///
/// This is deliberately process intent rather than HTTP/server lifecycle state.
/// Consumers may mirror an accepted transition into `ores-middleware`, stop a
/// listener, flush telemetry, or close resources, but those responsibilities do
/// not move into `ores-clis-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ShutdownLifecyclePhase {
    /// No shutdown intent has been accepted yet.
    #[default]
    Running,
    /// Graceful draining has begun.
    Draining,
    /// An already-started drain has been explicitly escalated.
    Forced,
}

impl ShutdownLifecyclePhase {
    /// Canonical lowercase diagnostic/wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Draining => "draining",
            Self::Forced => "forced",
        }
    }

    const fn from_raw_fail_closed(value: u8) -> Self {
        match value {
            RUNNING => Self::Running,
            DRAINING => Self::Draining,
            FORCED => Self::Forced,
            _ => Self::Forced,
        }
    }
}

impl fmt::Display for ShutdownLifecyclePhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Lock-free gate for combining multiple process-shutdown intent sources.
///
/// The gate admits only `Running -> Draining -> Forced`. Duplicate requests,
/// force-before-drain, and regressions after force are ignored. This is useful
/// when a consumer combines `setup_signal_handlers_with_lifecycle(...)` with an
/// admin endpoint, orchestrator hook, or another process-event source and wants
/// exactly-once lifecycle orchestration without installing another signal
/// handler or inventing a second HTTP lifecycle authority.
#[derive(Debug)]
pub struct ShutdownLifecycleGate {
    phase: AtomicU8,
}

impl Default for ShutdownLifecycleGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownLifecycleGate {
    /// Construct a gate in the running phase.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: AtomicU8::new(RUNNING),
        }
    }

    /// Current monotonic process-shutdown phase.
    #[must_use]
    pub fn phase(&self) -> ShutdownLifecyclePhase {
        ShutdownLifecyclePhase::from_raw_fail_closed(self.phase.load(Ordering::Acquire))
    }

    /// Whether new application work may still be admitted by the consumer.
    ///
    /// This is only an intent observation; actual request admission remains the
    /// consumer/middleware responsibility.
    #[must_use]
    pub fn is_accepting_work(&self) -> bool {
        self.phase() == ShutdownLifecyclePhase::Running
    }

    /// Whether explicit force escalation has been accepted.
    #[must_use]
    pub fn is_forced(&self) -> bool {
        self.phase() == ShutdownLifecyclePhase::Forced
    }

    /// Accept the first request to begin graceful drain.
    ///
    /// Returns `true` only for the single `Running -> Draining` transition.
    pub fn try_begin_drain(&self) -> bool {
        self.phase
            .compare_exchange(RUNNING, DRAINING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Accept the first force request after draining has begun.
    ///
    /// A force request while still running is rejected rather than skipping the
    /// grace phase. Returns `true` only for `Draining -> Forced`.
    pub fn try_force(&self) -> bool {
        self.phase
            .compare_exchange(DRAINING, FORCED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Apply a lifecycle action emitted by the shared signal layer.
    ///
    /// The action's reason remains owned by the caller for logging/exit policy;
    /// this gate only deduplicates and orders the phase transition.
    pub fn try_apply(&self, action: ShutdownAction) -> bool {
        match action {
            ShutdownAction::Drain(_) => self.try_begin_drain(),
            ShutdownAction::Force(_) => self.try_force(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shutdown::ShutdownReason;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn phases_have_stable_lowercase_spellings() {
        assert_eq!(ShutdownLifecyclePhase::Running.as_str(), "running");
        assert_eq!(ShutdownLifecyclePhase::Draining.as_str(), "draining");
        assert_eq!(ShutdownLifecyclePhase::Forced.as_str(), "forced");
        assert_eq!(ShutdownLifecyclePhase::Forced.to_string(), "forced");
    }

    #[test]
    fn default_gate_is_running_and_accepting_work() {
        let gate = ShutdownLifecycleGate::default();
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Running);
        assert!(gate.is_accepting_work());
        assert!(!gate.is_forced());
    }

    #[test]
    fn lifecycle_is_monotonic() {
        let gate = ShutdownLifecycleGate::new();
        assert!(gate.try_begin_drain());
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Draining);
        assert!(!gate.is_accepting_work());
        assert!(!gate.try_begin_drain());
        assert!(gate.try_force());
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Forced);
        assert!(gate.is_forced());
        assert!(!gate.try_force());
        assert!(!gate.try_begin_drain());
    }

    #[test]
    fn force_before_drain_is_rejected_without_changing_phase() {
        let gate = ShutdownLifecycleGate::new();
        assert!(!gate.try_force());
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Running);
        assert!(gate.is_accepting_work());
    }

    #[test]
    fn signal_actions_map_to_the_same_monotonic_gate() {
        let gate = ShutdownLifecycleGate::new();
        assert!(gate.try_apply(ShutdownAction::Drain(ShutdownReason::SigTerm)));
        assert!(!gate.try_apply(ShutdownAction::Drain(ShutdownReason::SigInt)));
        assert!(gate.try_apply(ShutdownAction::Force(ShutdownReason::CtrlD)));
        assert!(!gate.try_apply(ShutdownAction::Force(ShutdownReason::CtrlD)));
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Forced);
    }

    #[test]
    fn concurrent_drain_requests_admit_exactly_one_transition() {
        let gate = Arc::new(ShutdownLifecycleGate::new());
        let barrier = Arc::new(Barrier::new(33));
        let mut workers = Vec::new();
        for _ in 0..32 {
            let gate = Arc::clone(&gate);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                gate.try_begin_drain()
            }));
        }
        barrier.wait();
        let admitted = workers
            .into_iter()
            .map(|worker| worker.join().expect("drain worker"))
            .filter(|admitted| *admitted)
            .count();
        assert_eq!(admitted, 1);
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Draining);
    }

    #[test]
    fn concurrent_force_requests_after_drain_admit_exactly_one_transition() {
        let gate = Arc::new(ShutdownLifecycleGate::new());
        assert!(gate.try_begin_drain());
        let barrier = Arc::new(Barrier::new(33));
        let mut workers = Vec::new();
        for _ in 0..32 {
            let gate = Arc::clone(&gate);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                gate.try_force()
            }));
        }
        barrier.wait();
        let admitted = workers
            .into_iter()
            .map(|worker| worker.join().expect("force worker"))
            .filter(|admitted| *admitted)
            .count();
        assert_eq!(admitted, 1);
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Forced);
    }

    #[test]
    fn corrupted_internal_phase_fails_closed() {
        let gate = ShutdownLifecycleGate {
            phase: AtomicU8::new(u8::MAX),
        };
        assert_eq!(gate.phase(), ShutdownLifecyclePhase::Forced);
        assert!(!gate.is_accepting_work());
        assert!(gate.is_forced());
        assert!(!gate.try_begin_drain());
        assert!(!gate.try_force());
    }
}
