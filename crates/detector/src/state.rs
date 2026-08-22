//! The detection state machine.
//!
//! A threshold comparison alone is not a detection. Traffic crosses a
//! line and falls back below it constantly, and an engine that paged on
//! every crossing would be useless within an hour. What turns a
//! comparison into a detection is time: the crossing has to persist
//! (`triggerFor`), the recovery has to persist (`clearFor`), a detection
//! has to stay open for a minimum period once opened (`holdDown`), and
//! the same scope must not immediately re-open (`cooldown`).
//!
//! Those four timers are the whole of this module. Everything else here
//! exists to make the resulting behaviour auditable: every transition
//! records where it came from, where it went, why, under which policy
//! version, and what the traffic looked like at that instant.
//!
//! # Restart behaviour
//!
//! State lives in memory and is deliberately not persisted. On restart
//! every scope begins at [`DetectionState::Idle`], so a detection that
//! was open before a restart is re-derived from live traffic rather than
//! resumed from a stale record. The trade-off is explicit: a restart
//! during an attack costs one `triggerFor` of re-detection latency and
//! emits a second start event, which is recoverable; resuming a
//! persisted `Active` state that no longer reflects reality is not. See
//! docs/architecture/detection-engine.md.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use crate::evaluate::{Evaluation, MatchedReason};
use crate::input::{DetectionSnapshot, ScopeKey};
use crate::policy::DetectionPolicy;

/// Where one scope stands under its winning policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DetectionState {
    /// Nothing is happening, and nothing recently was.
    Idle,
    /// Thresholds are being exceeded, but not yet for `triggerFor`.
    PendingTrigger,
    /// A detection is open.
    Active,
    /// An open detection has fallen to its clear level, but not yet for
    /// `clearFor`.
    PendingClear,
    /// A detection closed recently; new detections for this scope are
    /// suppressed until `cooldown` elapses.
    Cooldown,
}

impl DetectionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectionState::Idle => "idle",
            DetectionState::PendingTrigger => "pendingTrigger",
            DetectionState::Active => "active",
            DetectionState::PendingClear => "pendingClear",
            DetectionState::Cooldown => "cooldown",
        }
    }

    /// Whether a detection is open in this state. `PendingClear` counts:
    /// the detection has not ended, it is only recovering.
    pub fn is_open(&self) -> bool {
        matches!(self, DetectionState::Active | DetectionState::PendingClear)
    }

    /// Whether the machine may move from `self` to `next`.
    ///
    /// This is a real guard, not documentation. The transitions this
    /// module generates are all legal by construction, so the guard's
    /// job is to catch a future edit that introduces an edge nobody
    /// reasoned about — the machine refuses it rather than silently
    /// entering a state whose entry time and timers mean nothing.
    ///
    /// Every state may move to `Idle`: that is the reset edge, used when
    /// a policy changes underneath a scope or a stale entry is closed.
    pub fn can_transition_to(&self, next: DetectionState) -> bool {
        use DetectionState::{Active, Cooldown, Idle, PendingClear, PendingTrigger};
        if next == Idle {
            return *self != Idle;
        }
        matches!(
            (self, next),
            (Idle, PendingTrigger)
                | (Idle, Active)
                | (PendingTrigger, Active)
                | (Active, PendingClear)
                | (Active, Cooldown)
                | (PendingClear, Active)
                | (PendingClear, Cooldown)
        )
    }
}

/// Why the machine moved.
///
/// These strings end up in events and logs, so they are the vocabulary
/// an operator uses to explain an alert. They name the *cause*, never
/// the destination — "the trigger held for long enough", not "went
/// active", because the destination is already recorded separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TransitionReason {
    /// A threshold was crossed while the scope was idle.
    ThresholdCrossed,
    /// The crossing persisted for `triggerFor`.
    TriggerSustained,
    /// The crossing stopped before `triggerFor` elapsed.
    TriggerAborted,
    /// An open detection fell to its clear level.
    FellBelowClear,
    /// The recovery persisted for `clearFor`.
    ClearSustained,
    /// Traffic rose again before `clearFor` elapsed.
    ClearAborted,
    /// `cooldown` elapsed with no new detection.
    CooldownExpired,
    /// A different policy now wins this scope, or the winning policy's
    /// version changed. Existing state is not carried across a policy
    /// change, because the timers it was accumulated under no longer
    /// apply.
    PolicyChanged,
    /// No policy matches this scope any more.
    PolicyWithdrawn,
    /// No snapshot arrived for this scope for longer than the stale
    /// timeout, so an open detection was closed rather than left open
    /// forever. Traffic stopping is one end of an attack, but it is also
    /// what a failed exporter looks like — the reason says which of the
    /// two produced the close, so nobody has to guess.
    Stale,
    /// An operator closed the detection by hand.
    ManualReset,
}

impl TransitionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransitionReason::ThresholdCrossed => "thresholdCrossed",
            TransitionReason::TriggerSustained => "triggerSustained",
            TransitionReason::TriggerAborted => "triggerAborted",
            TransitionReason::FellBelowClear => "fellBelowClear",
            TransitionReason::ClearSustained => "clearSustained",
            TransitionReason::ClearAborted => "clearAborted",
            TransitionReason::CooldownExpired => "cooldownExpired",
            TransitionReason::PolicyChanged => "policyChanged",
            TransitionReason::PolicyWithdrawn => "policyWithdrawn",
            TransitionReason::Stale => "stale",
            TransitionReason::ManualReset => "manualReset",
        }
    }
}

/// One recorded move, carrying everything needed to explain it later
/// without re-deriving anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransition {
    pub key: ScopeKey,
    pub policy_id: String,
    pub policy_version: u32,
    pub from: DetectionState,
    pub to: DetectionState,
    pub reason: TransitionReason,
    /// Monotonic instant, used for every duration comparison.
    pub at: Instant,
    /// Wall clock, used only for display. Never for arithmetic — a wall
    /// clock can step backwards.
    pub at_wall: SystemTime,
    /// Thresholds crossed at this instant. Empty for time-driven
    /// transitions, which is itself informative.
    pub matched: Vec<MatchedReason>,
    /// How long the machine had been in `from`.
    pub time_in_previous: Duration,
}

/// What the engine downstream should do about a step, in event terms.
///
/// The state machine decides *when* an event exists; it does not build
/// one. Keeping the two apart means the event schema can change without
/// touching a single timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    /// A detection opened.
    Started,
    /// An open detection is still open, and `eventUpdateInterval` has
    /// elapsed since the last emitted event.
    Updated,
    /// A detection closed.
    Ended,
}

/// Why a step did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepIgnored {
    /// The snapshot's window does not match the winning policy's window.
    WindowMismatch,
    /// A snapshot older than the newest one already applied to this
    /// scope. Applying it would move timers backwards, so it is dropped
    /// and counted.
    OutOfOrder,
    /// A snapshot bearing the same instant as the last one applied.
    /// Treated as a replay: the recorded state is returned unchanged, so
    /// re-delivering a snapshot cannot double-advance a timer or emit a
    /// second start event.
    Duplicate,
    /// The scope is not tracked and the state table is full.
    TableFull,
    /// The winning policy's execution mode does not evaluate.
    ModeDisabled,
}

impl StepIgnored {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepIgnored::WindowMismatch => "windowMismatch",
            StepIgnored::OutOfOrder => "outOfOrder",
            StepIgnored::Duplicate => "duplicate",
            StepIgnored::TableFull => "tableFull",
            StepIgnored::ModeDisabled => "modeDisabled",
        }
    }
}

/// A threshold crossing that deliberately produced no detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Suppression {
    /// Crossed while in `cooldown`.
    Cooldown,
    /// Recovered while inside `holdDown`, so the detection stayed open.
    HoldDown,
}

impl Suppression {
    pub fn as_str(&self) -> &'static str {
        match self {
            Suppression::Cooldown => "cooldown",
            Suppression::HoldDown => "holdDown",
        }
    }
}

/// The result of feeding one snapshot to one scope.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StepOutcome {
    /// In order. At most two: a timer expiry followed by an
    /// evaluation-driven move.
    pub transitions: Vec<StateTransition>,
    pub signals: Vec<Signal>,
    /// The state the scope is in after the step, or `None` if the scope
    /// is not tracked at all.
    pub state: Option<DetectionState>,
    pub ignored: Option<StepIgnored>,
    pub suppressed: Option<Suppression>,
}

impl StepOutcome {
    fn skipped(reason: StepIgnored, state: Option<DetectionState>) -> Self {
        StepOutcome {
            state,
            ignored: Some(reason),
            ..Default::default()
        }
    }

    /// Whether the step opened, updated, or closed a detection.
    pub fn has_signals(&self) -> bool {
        !self.signals.is_empty()
    }
}

/// Everything remembered about one scope between snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeState {
    pub key: ScopeKey,
    pub policy_id: String,
    pub policy_version: u32,
    pub state: DetectionState,
    /// When the machine entered `state`. Preserved across every step
    /// that does not change `state` — this is what the `triggerFor`,
    /// `clearFor`, and `cooldown` comparisons measure from.
    pub entered_at: Instant,
    /// When the current detection opened. `None` unless a detection is
    /// open. Survives an `Active -> PendingClear -> Active` round trip,
    /// so `holdDown` measures from the real start rather than from the
    /// most recent wobble.
    pub active_since: Option<Instant>,
    /// Newest snapshot applied, or `None` until the first one is.
    /// Guards ordering — it must stay distinct from `entered_at`,
    /// because a transition also lands on the current instant and would
    /// otherwise make a genuine replay indistinguishable from a first
    /// evaluation.
    pub last_snapshot_at: Option<Instant>,
    pub last_snapshot_wall: Option<SystemTime>,
    /// Last emitted event, for `eventUpdateInterval` pacing.
    pub last_event_at: Option<Instant>,
    /// Thresholds crossed on the most recent evaluated snapshot.
    pub last_matched: Vec<MatchedReason>,
    /// Highest observed value seen for each crossed metric during the
    /// current detection, in the order the metrics were first crossed.
    pub peak_matched: Vec<MatchedReason>,
    /// Snapshots applied since the detection opened.
    pub snapshots_in_detection: u64,
}

impl ScopeState {
    fn new(key: ScopeKey, policy: &DetectionPolicy, at: Instant) -> Self {
        ScopeState {
            key,
            policy_id: policy.id.clone(),
            policy_version: policy.version,
            state: DetectionState::Idle,
            entered_at: at,
            active_since: None,
            last_snapshot_at: None,
            last_snapshot_wall: None,
            last_event_at: None,
            last_matched: Vec::new(),
            peak_matched: Vec::new(),
            snapshots_in_detection: 0,
        }
    }

    /// How long the machine has been in its current state.
    pub fn time_in_state(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.entered_at)
    }

    /// How long the current detection has been open, if one is.
    pub fn detection_age(&self, now: Instant) -> Option<Duration> {
        self.active_since
            .map(|since| now.saturating_duration_since(since))
    }

    /// Applies a transition, refusing an edge the machine does not have.
    ///
    /// Returns the recorded transition, or `None` if the edge is
    /// illegal. `None` is never expected from this module's own call
    /// sites; a caller that sees it has found a bug, not a traffic
    /// condition.
    fn transition(
        &mut self,
        to: DetectionState,
        reason: TransitionReason,
        at: Instant,
        at_wall: SystemTime,
        matched: Vec<MatchedReason>,
    ) -> Option<StateTransition> {
        if !self.state.can_transition_to(to) {
            return None;
        }
        let from = self.state;
        let time_in_previous = at.saturating_duration_since(self.entered_at);
        self.state = to;
        self.entered_at = at;
        match to {
            DetectionState::Active => {
                if self.active_since.is_none() {
                    self.active_since = Some(at);
                    self.snapshots_in_detection = 0;
                    self.peak_matched.clear();
                }
            }
            DetectionState::Idle | DetectionState::Cooldown => {
                self.active_since = None;
            }
            DetectionState::PendingTrigger | DetectionState::PendingClear => {}
        }
        Some(StateTransition {
            key: self.key.clone(),
            policy_id: self.policy_id.clone(),
            policy_version: self.policy_version,
            from,
            to,
            reason,
            at,
            at_wall,
            matched,
            time_in_previous,
        })
    }

    /// Keeps the highest value seen for each crossed metric.
    fn record_peaks(&mut self, matched: &[MatchedReason]) {
        for reason in matched {
            match self
                .peak_matched
                .iter_mut()
                .find(|existing| existing.metric == reason.metric)
            {
                Some(existing) if reason.observed > existing.observed => *existing = reason.clone(),
                Some(_) => {}
                None => self.peak_matched.push(reason.clone()),
            }
        }
    }
}

/// How the state table is bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateTableConfig {
    /// Hard cap on tracked scopes. Reaching it rejects *new* scopes; it
    /// never evicts a tracked one. That asymmetry is deliberate: an
    /// LRU eviction would silently discard an open detection and its
    /// end event, turning a memory bound into a correctness bug.
    pub max_entries: usize,
    /// An `Idle` scope untouched for this long is forgotten. Purely a
    /// memory reclaim — an idle scope carries no detection to lose.
    pub idle_ttl: Duration,
    /// An open or cooling scope untouched for this long is force-closed
    /// with [`TransitionReason::Stale`]. This is the safety valve for an
    /// exporter that stops sending mid-attack: without it the detection
    /// would stay open forever.
    pub stale_after: Duration,
}

impl Default for StateTableConfig {
    fn default() -> Self {
        StateTableConfig {
            max_entries: 100_000,
            idle_ttl: Duration::from_secs(300),
            stale_after: Duration::from_secs(180),
        }
    }
}

/// What a sweep did to one scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expiry {
    pub key: ScopeKey,
    /// Present when the scope had an open detection that the sweep
    /// closed, so the engine can emit its end event.
    pub transition: Option<StateTransition>,
    pub signal: Option<Signal>,
}

/// Counters the engine turns into metrics. Kept here so the state
/// machine can be tested for its bookkeeping without a metrics backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateTableStats {
    pub rejected_table_full: u64,
    pub ignored_out_of_order: u64,
    pub ignored_duplicate: u64,
    pub ignored_window_mismatch: u64,
    pub expired_idle: u64,
    pub expired_stale: u64,
    pub policy_changes: u64,
}

/// Every tracked scope, bounded, with the timers applied to each.
#[derive(Debug, Clone)]
pub struct StateTable {
    config: StateTableConfig,
    entries: HashMap<ScopeKey, ScopeState>,
    stats: StateTableStats,
}

impl StateTable {
    pub fn new(config: StateTableConfig) -> Self {
        StateTable {
            config,
            entries: HashMap::new(),
            stats: StateTableStats::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn config(&self) -> StateTableConfig {
        self.config
    }

    pub fn stats(&self) -> StateTableStats {
        self.stats
    }

    pub fn get(&self, key: &ScopeKey) -> Option<&ScopeState> {
        self.entries.get(key)
    }

    /// Scopes with an open detection, in deterministic key order.
    pub fn open_detections(&self) -> Vec<&ScopeState> {
        let mut open: Vec<&ScopeState> = self
            .entries
            .values()
            .filter(|entry| entry.state.is_open())
            .collect();
        open.sort_by(|a, b| a.key.cmp(&b.key));
        open
    }

    /// How many scopes are in each state, for gauges.
    pub fn count_in(&self, state: DetectionState) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.state == state)
            .count()
    }

    /// Feeds one evaluated snapshot to one scope.
    ///
    /// `policy` must be the winner selected for `snapshot.key`, and
    /// `evaluation` must be that policy applied to that snapshot. The
    /// state machine does not re-derive either: pairing them is the
    /// engine's job, and doing it twice would let the two drift.
    pub fn step(
        &mut self,
        policy: &DetectionPolicy,
        snapshot: &DetectionSnapshot,
        evaluation: &Evaluation,
    ) -> StepOutcome {
        if !policy.execution_mode.evaluates() {
            return StepOutcome::skipped(
                StepIgnored::ModeDisabled,
                self.entries.get(&snapshot.key).map(|entry| entry.state),
            );
        }
        if snapshot.window != policy.window {
            self.stats.ignored_window_mismatch += 1;
            return StepOutcome::skipped(
                StepIgnored::WindowMismatch,
                self.entries.get(&snapshot.key).map(|entry| entry.state),
            );
        }

        let now = snapshot.observed_at;
        let wall = snapshot.observed_wall;

        // Admission. A scope only becomes tracked when the table has
        // room; an untracked scope over threshold is a dropped
        // detection, so the rejection is counted rather than ignored.
        if !self.entries.contains_key(&snapshot.key) {
            if self.entries.len() >= self.config.max_entries {
                self.stats.rejected_table_full += 1;
                return StepOutcome::skipped(StepIgnored::TableFull, None);
            }
            let entry = ScopeState::new(snapshot.key.clone(), policy, now);
            self.entries.insert(snapshot.key.clone(), entry);
        }

        let mut outcome = StepOutcome::default();
        let entry = self
            .entries
            .get_mut(&snapshot.key)
            .expect("entry inserted above");

        // Ordering. Both guards run before any timer moves, so a replay
        // or a late snapshot can neither advance nor rewind the machine.
        if let Some(last) = entry.last_snapshot_at {
            if now < last {
                self.stats.ignored_out_of_order += 1;
                return StepOutcome::skipped(StepIgnored::OutOfOrder, Some(entry.state));
            }
            if now == last {
                self.stats.ignored_duplicate += 1;
                return StepOutcome::skipped(StepIgnored::Duplicate, Some(entry.state));
            }
        }

        // A policy swap resets the machine: the accumulated timers were
        // measured against thresholds that no longer apply.
        if entry.policy_id != policy.id || entry.policy_version != policy.version {
            self.stats.policy_changes += 1;
            let was_open = entry.state.is_open();
            if let Some(transition) = entry.transition(
                DetectionState::Idle,
                TransitionReason::PolicyChanged,
                now,
                wall,
                Vec::new(),
            ) {
                outcome.transitions.push(transition);
                if was_open {
                    outcome.signals.push(Signal::Ended);
                }
            }
            entry.policy_id = policy.id.clone();
            entry.policy_version = policy.version;
            entry.last_event_at = None;
            entry.peak_matched.clear();
        }

        entry.last_snapshot_at = Some(now);
        entry.last_snapshot_wall = Some(wall);
        entry.last_matched = evaluation.matched.clone();

        // Pass one: the only purely time-driven exit. Running it before
        // the evaluation means a snapshot arriving exactly as cooldown
        // ends can open a detection in the same step, rather than
        // waiting a further window.
        if entry.state == DetectionState::Cooldown && entry.time_in_state(now) >= policy.cooldown {
            if let Some(transition) = entry.transition(
                DetectionState::Idle,
                TransitionReason::CooldownExpired,
                now,
                wall,
                Vec::new(),
            ) {
                outcome.transitions.push(transition);
            }
        }

        // Pass two: the evaluation.
        let over = evaluation.is_over_threshold();
        let matched = evaluation.matched.clone();
        if entry.state.is_open() {
            entry.snapshots_in_detection = entry.snapshots_in_detection.saturating_add(1);
            entry.record_peaks(&matched);
        }

        match entry.state {
            DetectionState::Idle => {
                if over {
                    let to = if policy.trigger_for.is_zero() {
                        DetectionState::Active
                    } else {
                        DetectionState::PendingTrigger
                    };
                    if let Some(transition) = entry.transition(
                        to,
                        TransitionReason::ThresholdCrossed,
                        now,
                        wall,
                        matched.clone(),
                    ) {
                        outcome.transitions.push(transition);
                    }
                    if to == DetectionState::Active {
                        entry.record_peaks(&matched);
                        entry.last_event_at = Some(now);
                        outcome.signals.push(Signal::Started);
                    }
                }
            }
            DetectionState::PendingTrigger => {
                if over {
                    if entry.time_in_state(now) >= policy.trigger_for {
                        if let Some(transition) = entry.transition(
                            DetectionState::Active,
                            TransitionReason::TriggerSustained,
                            now,
                            wall,
                            matched.clone(),
                        ) {
                            outcome.transitions.push(transition);
                        }
                        entry.record_peaks(&matched);
                        entry.last_event_at = Some(now);
                        outcome.signals.push(Signal::Started);
                    }
                } else if let Some(transition) = entry.transition(
                    DetectionState::Idle,
                    TransitionReason::TriggerAborted,
                    now,
                    wall,
                    Vec::new(),
                ) {
                    // A pending trigger requires the *trigger* threshold
                    // continuously. Hysteresis governs leaving `Active`,
                    // not aborting a trigger that never completed.
                    outcome.transitions.push(transition);
                }
            }
            DetectionState::Active => {
                if evaluation.all_below_clear {
                    let held = entry
                        .detection_age(now)
                        .map(|age| age >= policy.hold_down)
                        .unwrap_or(true);
                    if !held {
                        outcome.suppressed = Some(Suppression::HoldDown);
                    } else if policy.clear_for.is_zero() {
                        Self::close(entry, policy, now, wall, &mut outcome);
                    } else if let Some(transition) = entry.transition(
                        DetectionState::PendingClear,
                        TransitionReason::FellBelowClear,
                        now,
                        wall,
                        Vec::new(),
                    ) {
                        outcome.transitions.push(transition);
                    }
                }
            }
            DetectionState::PendingClear => {
                if evaluation.all_below_clear {
                    if entry.time_in_state(now) >= policy.clear_for {
                        Self::close(entry, policy, now, wall, &mut outcome);
                    }
                } else if let Some(transition) = entry.transition(
                    DetectionState::Active,
                    TransitionReason::ClearAborted,
                    now,
                    wall,
                    matched.clone(),
                ) {
                    // Anything not at or below the clear level ends the
                    // recovery, including the hysteresis band. Traffic
                    // in the band is not recovering.
                    outcome.transitions.push(transition);
                }
            }
            DetectionState::Cooldown => {
                if over {
                    outcome.suppressed = Some(Suppression::Cooldown);
                }
            }
        }

        // A still-open detection emits a paced update. Checked after the
        // transitions so a detection that just opened does not also
        // report an update in the same step.
        if entry.state.is_open() && outcome.signals.is_empty() && over {
            let due = entry
                .last_event_at
                .map(|last| now.saturating_duration_since(last) >= policy.event_update_interval)
                .unwrap_or(true);
            if due {
                entry.last_event_at = Some(now);
                outcome.signals.push(Signal::Updated);
            }
        }

        outcome.state = Some(entry.state);
        outcome
    }

    /// Ends an open detection, landing in `Cooldown` or straight back in
    /// `Idle` when the policy sets no cooldown.
    fn close(
        entry: &mut ScopeState,
        policy: &DetectionPolicy,
        now: Instant,
        wall: SystemTime,
        outcome: &mut StepOutcome,
    ) {
        let to = if policy.cooldown.is_zero() {
            DetectionState::Idle
        } else {
            DetectionState::Cooldown
        };
        if let Some(transition) =
            entry.transition(to, TransitionReason::ClearSustained, now, wall, Vec::new())
        {
            outcome.transitions.push(transition);
            entry.last_event_at = Some(now);
            outcome.signals.push(Signal::Ended);
        }
    }

    /// Closes a scope by operator request. Returns the transition, or
    /// `None` if the scope is untracked or already idle.
    pub fn manual_reset(
        &mut self,
        key: &ScopeKey,
        now: Instant,
        wall: SystemTime,
    ) -> Option<StateTransition> {
        let entry = self.entries.get_mut(key)?;
        entry.last_event_at = Some(now);
        entry.transition(
            DetectionState::Idle,
            TransitionReason::ManualReset,
            now,
            wall,
            Vec::new(),
        )
    }

    /// Drops every scope whose winning policy has gone away.
    ///
    /// `still_selected` answers "does any policy still win this scope?".
    /// Open detections are closed with a transition the caller can turn
    /// into an end event; idle ones are simply removed.
    pub fn withdraw_unselected<F>(
        &mut self,
        now: Instant,
        wall: SystemTime,
        mut still_selected: F,
    ) -> Vec<Expiry>
    where
        F: FnMut(&ScopeKey) -> bool,
    {
        let doomed: Vec<ScopeKey> = self
            .entries
            .keys()
            .filter(|key| !still_selected(key))
            .cloned()
            .collect();
        let mut expiries = Vec::new();
        for key in doomed {
            let Some(mut entry) = self.entries.remove(&key) else {
                continue;
            };
            let was_open = entry.state.is_open();
            let transition = entry.transition(
                DetectionState::Idle,
                TransitionReason::PolicyWithdrawn,
                now,
                wall,
                Vec::new(),
            );
            expiries.push(Expiry {
                key,
                transition,
                signal: was_open.then_some(Signal::Ended),
            });
        }
        expiries.sort_by(|a, b| a.key.cmp(&b.key));
        expiries
    }

    /// Reclaims idle scopes and force-closes scopes whose snapshots
    /// stopped arriving.
    ///
    /// Returns one entry per closed detection, in deterministic key
    /// order, so the engine can emit the end events. Idle reclaims
    /// produce an `Expiry` with no transition and no signal: there was
    /// nothing to close.
    pub fn sweep(&mut self, now: Instant, wall: SystemTime) -> Vec<Expiry> {
        let idle_ttl = self.config.idle_ttl;
        let stale_after = self.config.stale_after;

        let mut doomed: Vec<ScopeKey> = Vec::new();
        for (key, entry) in &self.entries {
            // A scope that has never produced a snapshot is measured
            // from when it was admitted, so a scope that appears and
            // immediately goes quiet is still reclaimed.
            let since = entry.last_snapshot_at.unwrap_or(entry.entered_at);
            let quiet = now.saturating_duration_since(since);
            let ttl = if entry.state == DetectionState::Idle {
                idle_ttl
            } else {
                stale_after
            };
            if quiet >= ttl {
                doomed.push(key.clone());
            }
        }
        doomed.sort();

        let mut expiries = Vec::new();
        for key in doomed {
            let Some(mut entry) = self.entries.remove(&key) else {
                continue;
            };
            if entry.state == DetectionState::Idle {
                self.stats.expired_idle += 1;
                expiries.push(Expiry {
                    key,
                    transition: None,
                    signal: None,
                });
                continue;
            }
            self.stats.expired_stale += 1;
            let was_open = entry.state.is_open();
            let transition = entry.transition(
                DetectionState::Idle,
                TransitionReason::Stale,
                now,
                wall,
                Vec::new(),
            );
            expiries.push(Expiry {
                key,
                transition,
                signal: was_open.then_some(Signal::Ended),
            });
        }
        expiries
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::*;
    use crate::evaluate::evaluate;
    use crate::input::{
        AddressFamily, DataCompleteness, MetricKind, MetricRates, SamplingStatus, ScopeId,
        ScopeType, TrafficDirection,
    };
    use crate::policy::{
        ExecutionMode, PolicyDraft, PolicySelector, Severity, TenantPrefixes, Thresholds,
    };

    fn key() -> ScopeKey {
        ScopeKey {
            tenant: "acme".to_string(),
            scope_type: ScopeType::Host,
            scope_id: ScopeId::Host {
                addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
            },
            direction: TrafficDirection::Incoming,
            address_family: AddressFamily::Ipv4,
        }
    }

    fn other_key() -> ScopeKey {
        ScopeKey {
            scope_id: ScopeId::Host {
                addr: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8)),
            },
            ..key()
        }
    }

    /// 1 Mbps trigger, 1s window, 2s trigger, 2s clear, 10s cooldown.
    fn draft() -> PolicyDraft {
        PolicyDraft {
            id: "p-host-bps".to_string(),
            name: "host bps".to_string(),
            description: None,
            enabled: true,
            tenant: "acme".to_string(),
            scope_type: ScopeType::Host,
            selector: PolicySelector::Any,
            address_family: None,
            direction: TrafficDirection::Incoming,
            window: Duration::from_secs(1),
            thresholds: Thresholds::new().with(MetricKind::Bps, 1_000_000),
            clear_percent: 80,
            trigger_for: Duration::from_secs(2),
            clear_for: Duration::from_secs(2),
            cooldown: Duration::from_secs(10),
            hold_down: Duration::ZERO,
            event_update_interval: Duration::from_secs(30),
            severity: Severity::Major,
            execution_mode: ExecutionMode::AlertOnly,
            priority: 0,
            labels: std::collections::BTreeMap::new(),
            version: 1,
        }
    }

    fn policy() -> DetectionPolicy {
        draft().validate(&TenantPrefixes::new()).expect("valid")
    }

    fn snapshot(at: Instant, bps: u64) -> DetectionSnapshot {
        DetectionSnapshot {
            key: key(),
            window: Duration::from_secs(1),
            observed_at: at,
            observed_wall: SystemTime::UNIX_EPOCH,
            rates: MetricRates {
                bps,
                ..MetricRates::default()
            },
            completeness: DataCompleteness {
                protocol_seen: true,
                tcp_flags_seen: true,
                fragmentation_seen: true,
                forwarding_status_seen: true,
            },
            sampling: SamplingStatus::default(),
            flows_observed: 10,
            exporters_observed: 1,
        }
    }

    /// Feeds one snapshot through evaluation into the table.
    fn feed(
        table: &mut StateTable,
        policy: &DetectionPolicy,
        at: Instant,
        bps: u64,
    ) -> StepOutcome {
        let snap = snapshot(at, bps);
        let evaluation = evaluate(policy, &snap);
        table.step(policy, &snap, &evaluation)
    }

    fn table() -> StateTable {
        StateTable::new(StateTableConfig::default())
    }

    #[test]
    fn legal_edges_are_accepted() {
        use DetectionState::*;
        for (from, to) in [
            (Idle, PendingTrigger),
            (Idle, Active),
            (PendingTrigger, Active),
            (PendingTrigger, Idle),
            (Active, PendingClear),
            (Active, Cooldown),
            (Active, Idle),
            (PendingClear, Active),
            (PendingClear, Cooldown),
            (PendingClear, Idle),
            (Cooldown, Idle),
        ] {
            assert!(
                from.can_transition_to(to),
                "{:?} -> {:?} should be legal",
                from,
                to
            );
        }
    }

    #[test]
    fn impossible_edges_are_rejected() {
        use DetectionState::*;
        for (from, to) in [
            (Idle, PendingClear),
            (Idle, Cooldown),
            (Idle, Idle),
            (PendingTrigger, PendingClear),
            (PendingTrigger, Cooldown),
            (Active, PendingTrigger),
            (PendingClear, PendingTrigger),
            (Cooldown, PendingTrigger),
            (Cooldown, PendingClear),
            (Cooldown, Active),
        ] {
            assert!(
                !from.can_transition_to(to),
                "{:?} -> {:?} should be impossible",
                from,
                to
            );
        }
    }

    #[test]
    fn a_rejected_edge_leaves_the_entry_untouched() {
        let policy = policy();
        let start = Instant::now();
        let mut entry = ScopeState::new(key(), &policy, start);
        let before = entry.clone();
        let refused = entry.transition(
            DetectionState::PendingClear,
            TransitionReason::FellBelowClear,
            start + Duration::from_secs(1),
            SystemTime::UNIX_EPOCH,
            Vec::new(),
        );
        assert!(refused.is_none());
        assert_eq!(entry, before);
    }

    #[test]
    fn quiet_traffic_never_leaves_idle() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        for tick in 0..10 {
            let outcome = feed(&mut table, &policy, start + Duration::from_secs(tick), 10);
            assert_eq!(outcome.state, Some(DetectionState::Idle));
            assert!(outcome.transitions.is_empty());
            assert!(outcome.signals.is_empty());
        }
    }

    #[test]
    fn a_crossing_shorter_than_trigger_for_never_opens() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();

        let first = feed(&mut table, &policy, start, 5_000_000);
        assert_eq!(first.state, Some(DetectionState::PendingTrigger));
        assert_eq!(
            first.transitions[0].reason,
            TransitionReason::ThresholdCrossed
        );
        assert!(first.signals.is_empty());

        let back = feed(&mut table, &policy, start + Duration::from_secs(1), 10);
        assert_eq!(back.state, Some(DetectionState::Idle));
        assert_eq!(back.transitions[0].reason, TransitionReason::TriggerAborted);
        assert!(back.signals.is_empty());
    }

    #[test]
    fn a_sustained_crossing_opens_a_detection() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();

        feed(&mut table, &policy, start, 5_000_000);
        let still = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(1),
            5_000_000,
        );
        assert_eq!(still.state, Some(DetectionState::PendingTrigger));

        let opened = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            6_000_000,
        );
        assert_eq!(opened.state, Some(DetectionState::Active));
        assert_eq!(
            opened.transitions[0].reason,
            TransitionReason::TriggerSustained
        );
        assert_eq!(opened.signals, vec![Signal::Started]);
        assert_eq!(opened.transitions[0].matched.len(), 1);
        assert_eq!(opened.transitions[0].matched[0].metric, MetricKind::Bps);
        assert_eq!(
            opened.transitions[0].time_in_previous,
            Duration::from_secs(2)
        );
    }

    #[test]
    fn the_hysteresis_band_neither_opens_nor_closes() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();

        // Open first.
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        assert_eq!(
            table.get(&key()).map(|entry| entry.state),
            Some(DetectionState::Active)
        );

        // 900 kbps: below the 1 Mbps trigger, above the 800 kbps clear.
        let band = feed(&mut table, &policy, start + Duration::from_secs(3), 900_000);
        assert_eq!(band.state, Some(DetectionState::Active));
        assert!(band.transitions.is_empty());
        assert!(band.signals.is_empty());
    }

    #[test]
    fn a_sustained_recovery_closes_into_cooldown() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        let falling = feed(&mut table, &policy, start + Duration::from_secs(3), 100);
        assert_eq!(falling.state, Some(DetectionState::PendingClear));
        assert_eq!(
            falling.transitions[0].reason,
            TransitionReason::FellBelowClear
        );
        assert!(falling.signals.is_empty());

        let still = feed(&mut table, &policy, start + Duration::from_secs(4), 100);
        assert_eq!(still.state, Some(DetectionState::PendingClear));

        let closed = feed(&mut table, &policy, start + Duration::from_secs(5), 100);
        assert_eq!(closed.state, Some(DetectionState::Cooldown));
        assert_eq!(
            closed.transitions[0].reason,
            TransitionReason::ClearSustained
        );
        assert_eq!(closed.signals, vec![Signal::Ended]);
    }

    #[test]
    fn traffic_returning_during_clear_reopens_without_a_second_start() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        let opened_at = table.get(&key()).and_then(|entry| entry.active_since);

        feed(&mut table, &policy, start + Duration::from_secs(3), 100);
        let back = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(4),
            9_000_000,
        );
        assert_eq!(back.state, Some(DetectionState::Active));
        assert_eq!(back.transitions[0].reason, TransitionReason::ClearAborted);
        assert!(
            back.signals.is_empty(),
            "a detection that never ended must not start again"
        );
        assert_eq!(
            table.get(&key()).and_then(|entry| entry.active_since),
            opened_at,
            "holdDown must keep measuring from the original start"
        );
    }

    #[test]
    fn cooldown_suppresses_a_new_detection_then_expires() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        feed(&mut table, &policy, start + Duration::from_secs(3), 100);
        feed(&mut table, &policy, start + Duration::from_secs(5), 100);
        assert_eq!(
            table.get(&key()).map(|entry| entry.state),
            Some(DetectionState::Cooldown)
        );

        let suppressed = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(6),
            9_000_000,
        );
        assert_eq!(suppressed.state, Some(DetectionState::Cooldown));
        assert_eq!(suppressed.suppressed, Some(Suppression::Cooldown));
        assert!(suppressed.signals.is_empty());

        // Cooldown was entered at t+5 and lasts 10s.
        let expired = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(15),
            9_000_000,
        );
        assert_eq!(
            expired.transitions[0].reason,
            TransitionReason::CooldownExpired
        );
        assert_eq!(
            expired.transitions[1].reason,
            TransitionReason::ThresholdCrossed
        );
        assert_eq!(expired.state, Some(DetectionState::PendingTrigger));
    }

    #[test]
    fn hold_down_keeps_a_detection_open_through_an_early_recovery() {
        let mut draft = draft();
        draft.hold_down = Duration::from_secs(60);
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        let held = feed(&mut table, &policy, start + Duration::from_secs(3), 100);
        assert_eq!(held.state, Some(DetectionState::Active));
        assert_eq!(held.suppressed, Some(Suppression::HoldDown));
        assert!(held.transitions.is_empty());

        // Hold-down measured from the open at t+2, so it lapses at t+62.
        let allowed = feed(&mut table, &policy, start + Duration::from_secs(63), 100);
        assert_eq!(allowed.state, Some(DetectionState::PendingClear));
    }

    #[test]
    fn zero_cooldown_returns_straight_to_idle() {
        let mut draft = draft();
        draft.cooldown = Duration::ZERO;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        feed(&mut table, &policy, start + Duration::from_secs(3), 100);
        let closed = feed(&mut table, &policy, start + Duration::from_secs(5), 100);
        assert_eq!(closed.state, Some(DetectionState::Idle));
        assert_eq!(closed.signals, vec![Signal::Ended]);
    }

    #[test]
    fn a_replayed_snapshot_changes_nothing() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        let at = start + Duration::from_secs(2);
        let first = feed(&mut table, &policy, at, 5_000_000);
        assert_eq!(first.signals, vec![Signal::Started]);
        let before = table.get(&key()).cloned();

        let replay = feed(&mut table, &policy, at, 5_000_000);
        assert_eq!(replay.ignored, Some(StepIgnored::Duplicate));
        assert!(replay.signals.is_empty());
        assert_eq!(table.get(&key()).cloned(), before);
        assert_eq!(table.stats().ignored_duplicate, 1);
    }

    #[test]
    fn a_late_snapshot_cannot_rewind_a_timer() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(5),
            5_000_000,
        );
        let before = table.get(&key()).cloned();

        let late = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(1),
            5_000_000,
        );
        assert_eq!(late.ignored, Some(StepIgnored::OutOfOrder));
        assert_eq!(table.get(&key()).cloned(), before);
        assert_eq!(table.stats().ignored_out_of_order, 1);
    }

    #[test]
    fn a_mismatched_window_is_refused() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        let mut snap = snapshot(start, 9_000_000);
        snap.window = Duration::from_secs(5);
        let evaluation = evaluate(&policy, &snap);
        let outcome = table.step(&policy, &snap, &evaluation);
        assert_eq!(outcome.ignored, Some(StepIgnored::WindowMismatch));
        assert!(table.is_empty());
    }

    #[test]
    fn a_disabled_mode_is_not_evaluated() {
        let mut draft = draft();
        draft.execution_mode = ExecutionMode::Disabled;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let mut table = table();
        let outcome = feed(&mut table, &policy, Instant::now(), 9_000_000);
        assert_eq!(outcome.ignored, Some(StepIgnored::ModeDisabled));
        assert!(table.is_empty());
    }

    #[test]
    fn observe_mode_advances_state_and_still_signals() {
        // Observe evaluates; whether the event is *published* is the
        // engine's decision, not the state machine's.
        let mut draft = draft();
        draft.execution_mode = ExecutionMode::Observe;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        let opened = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        assert_eq!(opened.state, Some(DetectionState::Active));
        assert_eq!(opened.signals, vec![Signal::Started]);
    }

    #[test]
    fn the_table_rejects_new_scopes_when_full_but_keeps_serving_tracked_ones() {
        let policy = policy();
        let mut table = StateTable::new(StateTableConfig {
            max_entries: 1,
            ..StateTableConfig::default()
        });
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        assert_eq!(table.len(), 1);

        let mut other = snapshot(start, 9_000_000);
        other.key = other_key();
        let evaluation = evaluate(&policy, &other);
        let rejected = table.step(&policy, &other, &evaluation);
        assert_eq!(rejected.ignored, Some(StepIgnored::TableFull));
        assert_eq!(rejected.state, None);
        assert_eq!(table.stats().rejected_table_full, 1);

        // The tracked scope is unaffected.
        let opened = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        assert_eq!(opened.signals, vec![Signal::Started]);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn a_full_table_never_evicts_an_open_detection() {
        let policy = policy();
        let mut table = StateTable::new(StateTableConfig {
            max_entries: 1,
            ..StateTableConfig::default()
        });
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        for tick in 3..20 {
            let mut other = snapshot(start + Duration::from_secs(tick), 9_000_000);
            other.key = other_key();
            let evaluation = evaluate(&policy, &other);
            table.step(&policy, &other, &evaluation);
        }
        assert_eq!(
            table.get(&key()).map(|entry| entry.state),
            Some(DetectionState::Active)
        );
        assert_eq!(table.stats().rejected_table_full, 17);
    }

    #[test]
    fn an_idle_scope_is_reclaimed_without_an_event() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 10);
        let expiries = table.sweep(start + Duration::from_secs(301), SystemTime::UNIX_EPOCH);
        assert_eq!(expiries.len(), 1);
        assert!(expiries[0].transition.is_none());
        assert!(expiries[0].signal.is_none());
        assert!(table.is_empty());
        assert_eq!(table.stats().expired_idle, 1);
    }

    #[test]
    fn an_open_detection_whose_exporter_went_quiet_is_force_closed() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        // Under the idle TTL, so this only fires because the entry is open.
        let expiries = table.sweep(start + Duration::from_secs(200), SystemTime::UNIX_EPOCH);
        assert_eq!(expiries.len(), 1);
        let transition = expiries[0].transition.as_ref().expect("closed");
        assert_eq!(transition.reason, TransitionReason::Stale);
        assert_eq!(transition.from, DetectionState::Active);
        assert_eq!(transition.to, DetectionState::Idle);
        assert_eq!(expiries[0].signal, Some(Signal::Ended));
        assert!(table.is_empty());
        assert_eq!(table.stats().expired_stale, 1);
    }

    #[test]
    fn a_sweep_leaves_a_scope_that_is_still_reporting() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        let expiries = table.sweep(start + Duration::from_secs(3), SystemTime::UNIX_EPOCH);
        assert!(expiries.is_empty());
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn a_policy_version_bump_resets_and_closes() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        let mut next = draft();
        next.version = 2;
        let next = next.validate(&TenantPrefixes::new()).expect("valid");
        let outcome = feed(&mut table, &next, start + Duration::from_secs(3), 9_000_000);
        assert_eq!(
            outcome.transitions[0].reason,
            TransitionReason::PolicyChanged
        );
        assert_eq!(outcome.transitions[0].policy_version, 1);
        assert_eq!(outcome.signals[0], Signal::Ended);
        assert_eq!(outcome.state, Some(DetectionState::PendingTrigger));
        assert_eq!(table.stats().policy_changes, 1);
    }

    #[test]
    fn withdrawing_a_policy_closes_its_open_detections() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        let expiries = table.withdraw_unselected(
            start + Duration::from_secs(3),
            SystemTime::UNIX_EPOCH,
            |_| false,
        );
        assert_eq!(expiries.len(), 1);
        assert_eq!(
            expiries[0].transition.as_ref().map(|t| t.reason),
            Some(TransitionReason::PolicyWithdrawn)
        );
        assert_eq!(expiries[0].signal, Some(Signal::Ended));
        assert!(table.is_empty());
    }

    #[test]
    fn a_manual_reset_closes_an_open_detection() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );

        let transition = table
            .manual_reset(
                &key(),
                start + Duration::from_secs(3),
                SystemTime::UNIX_EPOCH,
            )
            .expect("reset");
        assert_eq!(transition.reason, TransitionReason::ManualReset);
        assert_eq!(transition.to, DetectionState::Idle);
        assert!(table
            .manual_reset(&key(), start, SystemTime::UNIX_EPOCH)
            .is_none());
        assert!(table
            .manual_reset(&other_key(), start, SystemTime::UNIX_EPOCH)
            .is_none());
    }

    #[test]
    fn updates_are_paced_by_the_event_update_interval() {
        let mut draft = draft();
        draft.event_update_interval = Duration::from_secs(10);
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        let opened = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        assert_eq!(opened.signals, vec![Signal::Started]);

        for tick in 3..12 {
            let outcome = feed(
                &mut table,
                &policy,
                start + Duration::from_secs(tick),
                5_000_000,
            );
            assert!(
                outcome.signals.is_empty(),
                "no update expected at t+{tick}s"
            );
        }
        let update = feed(
            &mut table,
            &policy,
            start + Duration::from_secs(12),
            5_000_000,
        );
        assert_eq!(update.signals, vec![Signal::Updated]);
    }

    #[test]
    fn peaks_track_the_worst_value_of_the_detection() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            5_000_000,
        );
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(3),
            40_000_000,
        );
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(4),
            2_000_000,
        );

        let peaks = &table.get(&key()).expect("tracked").peak_matched;
        assert_eq!(peaks.len(), 1);
        assert_eq!(peaks[0].metric, MetricKind::Bps);
        assert_eq!(peaks[0].observed, 40_000_000);
    }

    #[test]
    fn a_closed_detection_starts_its_peaks_over() {
        let mut draft = draft();
        draft.cooldown = Duration::ZERO;
        let policy = draft.validate(&TenantPrefixes::new()).expect("valid");
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 40_000_000);
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(2),
            40_000_000,
        );
        feed(&mut table, &policy, start + Duration::from_secs(3), 100);
        feed(&mut table, &policy, start + Duration::from_secs(5), 100);

        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(6),
            3_000_000,
        );
        feed(
            &mut table,
            &policy,
            start + Duration::from_secs(8),
            3_000_000,
        );
        let peaks = &table.get(&key()).expect("tracked").peak_matched;
        assert_eq!(peaks[0].observed, 3_000_000);
    }

    #[test]
    fn state_entry_time_survives_a_step_that_changes_nothing() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        feed(&mut table, &policy, start, 5_000_000);
        let entered = table.get(&key()).expect("tracked").entered_at;
        feed(
            &mut table,
            &policy,
            start + Duration::from_millis(500),
            5_000_000,
        );
        assert_eq!(table.get(&key()).expect("tracked").entered_at, entered);
    }

    #[test]
    fn counts_and_open_detections_are_reported_in_key_order() {
        let policy = policy();
        let mut table = table();
        let start = Instant::now();
        for (index, scope) in [key(), other_key()].into_iter().enumerate() {
            let offset = Duration::from_millis(index as u64);
            for tick in [0u64, 2] {
                let mut snap = snapshot(start + Duration::from_secs(tick) + offset, 5_000_000);
                snap.key = scope.clone();
                let evaluation = evaluate(&policy, &snap);
                table.step(&policy, &snap, &evaluation);
            }
        }
        assert_eq!(table.count_in(DetectionState::Active), 2);
        let open = table.open_detections();
        assert_eq!(open.len(), 2);
        assert!(open[0].key < open[1].key);
    }
}
