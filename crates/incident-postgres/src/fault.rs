//! Failure injection at each commit point of the flush (5B-5).
//!
//! The 5B-5 exit criterion is atomicity proven by injected failure, not by
//! observing success. Each [`FlushPoint`] is a place in one load–run–flush
//! transaction where a real failure could land: after each group of writes,
//! and just before `COMMIT`.
//!
//! Arming and injecting exist only with the `fault-injection` Cargo
//! feature. This crate's own dev-dependency on itself enables the feature,
//! so every `cargo test` of the crate has it. A normal build does not:
//! there, [`check`] is an always-`Ok` no-op and nothing can be armed.
//!
//! One fault is armed at a time, process-wide, and fires once. The
//! PostgreSQL tests that use it run in their own test binary, as a single
//! test function.

use crate::error::PersistError;

/// A point in the load–run–flush transaction where a failure can be
/// injected. Every call reaches every point, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlushPoint {
    /// After inserting and updating incidents and their child rows.
    Incidents,
    DetectionLinks,
    Timeline,
    Audit,
    Outbox,
    Idempotency,
    /// After writing the allocator's `next_value`, or after skipping it.
    Allocator,
    /// After the whole flush, just before `COMMIT`.
    BeforeCommit,
}

impl FlushPoint {
    pub const ALL: [FlushPoint; 8] = [
        FlushPoint::Incidents,
        FlushPoint::DetectionLinks,
        FlushPoint::Timeline,
        FlushPoint::Audit,
        FlushPoint::Outbox,
        FlushPoint::Idempotency,
        FlushPoint::Allocator,
        FlushPoint::BeforeCommit,
    ];
}

#[cfg(feature = "fault-injection")]
static ARMED: std::sync::Mutex<Option<(FlushPoint, bool)>> = std::sync::Mutex::new(None);

#[cfg(feature = "fault-injection")]
fn armed() -> std::sync::MutexGuard<'static, Option<(FlushPoint, bool)>> {
    ARMED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Arms one failure at `point`. `transient` decides whether the retry loop
/// may rerun the call.
#[cfg(feature = "fault-injection")]
pub fn arm(point: FlushPoint, transient: bool) {
    *armed() = Some((point, transient));
}

/// Whether a failure is still armed, that is, the flush never reached it.
#[cfg(feature = "fault-injection")]
pub fn is_armed() -> bool {
    armed().is_some()
}

#[cfg(feature = "fault-injection")]
pub fn disarm() {
    *armed() = None;
}

/// Fails once if a failure is armed at `point`.
pub(crate) fn check(point: FlushPoint) -> Result<(), PersistError> {
    #[cfg(feature = "fault-injection")]
    {
        let mut armed = armed();
        if let Some((armed_point, transient)) = *armed {
            if armed_point == point {
                *armed = None;
                return Err(PersistError::InjectedFault { point, transient });
            }
        }
    }
    #[cfg(not(feature = "fault-injection"))]
    let _ = point;
    Ok(())
}
