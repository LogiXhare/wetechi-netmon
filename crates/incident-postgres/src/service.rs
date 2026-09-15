//! One load–run–flush transaction per `IncidentUnitOfWork` entry point
//! (ADR 0034).
//!
//! Each method opens a Read Committed transaction on the caller's client,
//! loads the working set ([`crate::load`]), runs the unchanged domain call
//! over a [`StagingStore`], flushes what it changed ([`crate::flush`]), and
//! commits.
//!
//! The return type separates the two ways a call ends:
//!
//! - `Err(PersistError)`: nothing was committed. The load or flush failed,
//!   the call looked up a key the load did not fetch, or the domain
//!   reported a broken invariant. Rerunning from a fresh load is 5B-3(c).
//! - `Ok(domain_result)`: the transaction committed. A domain `Err` still
//!   commits what the call recorded about it, such as a denied-permission
//!   audit entry or a failed-outcome idempotency record, just as the
//!   in-memory store keeps them.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use tokio_postgres::{Client, IsolationLevel};
use wetechinetmon_detector::DetectionEvent;
use wetechinetmon_incident::authorization::AuthorizationContext;
use wetechinetmon_incident::clock::Clock;
use wetechinetmon_incident::closure::ClosurePolicy;
use wetechinetmon_incident::command::Command;
use wetechinetmon_incident::correlation::TenantId;
use wetechinetmon_incident::error::IncidentError;
use wetechinetmon_incident::id::{IncidentGenerator, IncidentId};
use wetechinetmon_incident::idempotency::IdempotencyKey;
use wetechinetmon_incident::number::{IncidentNumber, NumberAllocator};
use wetechinetmon_incident::reopen::ReopenPolicy;
use wetechinetmon_incident::transition::DetectionEndReason;
use wetechinetmon_incident::unit_of_work::{IncidentUnitOfWork, IngestResult};

use crate::error::PersistError;
use crate::flush::flush;
use crate::load::{load_for_incident, load_for_ingest};
use crate::staging::StagingStore;

/// The outer `Result` says whether the call committed; the inner one is
/// the domain's own result. See the module doc.
pub type Outcome<T> = Result<Result<T, IncidentError>, PersistError>;

/// The shared, long-lived half of every call: id generation, the clock, and
/// the unit of work's configuration.
pub struct IncidentPersistence {
    generator: Arc<dyn IncidentGenerator>,
    clock: Arc<dyn Clock>,
    number_allocation_year: Option<u32>,
    policies: Option<(ClosurePolicy, ReopenPolicy)>,
}

#[derive(Clone, Copy)]
enum Load<'a> {
    Ingest(&'a DetectionEvent),
    Incident {
        id: IncidentId,
        idempotency_key: Option<&'a IdempotencyKey>,
    },
}

impl IncidentPersistence {
    pub fn new(generator: Arc<dyn IncidentGenerator>, clock: Arc<dyn Clock>) -> Self {
        IncidentPersistence {
            generator,
            clock,
            number_allocation_year: None,
            policies: None,
        }
    }

    pub fn with_number_allocation_year(mut self, year: u32) -> Self {
        self.number_allocation_year = Some(year);
        self
    }

    pub fn with_policies(
        mut self,
        closure_policy: ClosurePolicy,
        reopen_policy: ReopenPolicy,
    ) -> Self {
        self.policies = Some((closure_policy, reopen_policy));
        self
    }

    pub async fn ingest_detection_event(
        &self,
        client: &mut Client,
        auth: &AuthorizationContext,
        event: &DetectionEvent,
    ) -> Outcome<IngestResult> {
        self.run(client, auth.tenant(), Load::Ingest(event), |uow| {
            uow.ingest_detection_event(auth, event)
        })
        .await
    }

    pub async fn handle_command(
        &self,
        client: &mut Client,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        command: Command,
        idempotency_key: Option<IdempotencyKey>,
    ) -> Outcome<u64> {
        let load = Load::Incident {
            id: incident_id,
            idempotency_key: idempotency_key.as_ref(),
        };
        let key_for_call = idempotency_key.clone();
        self.run(client, auth.tenant(), load, move |uow| {
            uow.handle_command(auth, incident_id, command, key_for_call)
        })
        .await
    }

    pub async fn enter_recovering(
        &self,
        client: &mut Client,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        reason: DetectionEndReason,
    ) -> Outcome<()> {
        self.run(client, auth.tenant(), by_id(incident_id), move |uow| {
            uow.enter_recovering(auth, incident_id, reason)
        })
        .await
    }

    pub async fn confirm_recovery_if_due(
        &self,
        client: &mut Client,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
        recovery_confirmation: Duration,
    ) -> Outcome<bool> {
        self.run(client, auth.tenant(), by_id(incident_id), move |uow| {
            uow.confirm_recovery_if_due(auth, incident_id, recovery_confirmation)
        })
        .await
    }

    pub async fn abort_recovery(
        &self,
        client: &mut Client,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
    ) -> Outcome<()> {
        self.run(client, auth.tenant(), by_id(incident_id), move |uow| {
            uow.abort_recovery(auth, incident_id)
        })
        .await
    }

    pub async fn attempt_automatic_closure(
        &self,
        client: &mut Client,
        auth: &AuthorizationContext,
        incident_id: IncidentId,
    ) -> Outcome<bool> {
        self.run(client, auth.tenant(), by_id(incident_id), move |uow| {
            uow.attempt_automatic_closure(auth, incident_id)
        })
        .await
    }

    async fn run<T>(
        &self,
        client: &mut Client,
        tenant: &TenantId,
        load: Load<'_>,
        call: impl FnOnce(&mut IncidentUnitOfWork) -> Result<T, IncidentError>,
    ) -> Outcome<T> {
        let transaction = client
            .build_transaction()
            .isolation_level(IsolationLevel::ReadCommitted)
            .start()
            .await?;

        let event = match load {
            Load::Ingest(event) => Some(event),
            Load::Incident { .. } => None,
        };
        let (store, next_number) = match load {
            Load::Ingest(event) if event.target.tenant == tenant.as_str() => {
                let loaded = load_for_ingest(&transaction, tenant, event).await?;
                (loaded.store, loaded.next_number)
            }
            // The domain refuses a cross-tenant event before any lookup, so
            // there is nothing to load for it.
            Load::Ingest(_) => (StagingStore::new(), None),
            Load::Incident {
                id,
                idempotency_key,
            } => (
                load_for_incident(&transaction, tenant, id, idempotency_key).await?,
                None,
            ),
        };

        // The domain call is synchronous and awaits nothing.
        let numbers = Arc::new(Mutex::new(next_number));
        let (outcome, store, final_number) = {
            let mut uow = IncidentUnitOfWork::new(
                Box::new(SharedGenerator(Arc::clone(&self.generator))),
                Box::new(StagedNumbers(Arc::clone(&numbers))),
                Box::new(SharedClock(Arc::clone(&self.clock))),
            )
            .with_store(Box::new(store));
            if let Some(year) = self.number_allocation_year {
                uow = uow.with_number_allocation_year(year);
            }
            if let Some((closure_policy, reopen_policy)) = self.policies {
                uow = uow.with_policies(closure_policy, reopen_policy);
            }
            let outcome = call(&mut uow);
            let store = StagingStore::recover(uow.into_store());
            let final_number = *numbers.lock().expect("number allocator lock poisoned");
            (outcome, store, final_number)
        };

        if let Err(IncidentError::InternalInvariantViolation(detail)) = &outcome {
            return Err(PersistError::DomainInvariant(detail));
        }
        let store = store.ok_or(PersistError::DomainInvariant(
            "the unit of work did not hand back its staging store",
        ))?;
        let changes = store.into_changes().map_err(PersistError::UnloadedLookup)?;
        let consumed = match (next_number, final_number) {
            (Some(loaded), Some(now)) if now != loaded => Some(now),
            _ => None,
        };

        flush(&transaction, tenant, &changes, event, consumed).await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

fn by_id(id: IncidentId) -> Load<'static> {
    Load::Incident {
        id,
        idempotency_key: None,
    }
}

struct SharedGenerator(Arc<dyn IncidentGenerator>);

impl IncidentGenerator for SharedGenerator {
    fn generate(&self) -> Result<IncidentId, IncidentError> {
        self.0.generate()
    }
}

struct SharedClock(Arc<dyn Clock>);

impl Clock for SharedClock {
    fn monotonic(&self) -> Instant {
        self.0.monotonic()
    }

    fn wall(&self) -> SystemTime {
        self.0.wall()
    }
}

/// The allocator row the load step locked, as its `next_value`. `None` when
/// the load did not lock it: the domain then gets an internal error instead
/// of a number, and the call is rolled back.
struct StagedNumbers(Arc<Mutex<Option<u64>>>);

impl NumberAllocator for StagedNumbers {
    fn allocate(
        &self,
        _tenant: &str,
        allocation_year: u32,
    ) -> Result<IncidentNumber, IncidentError> {
        let mut next = self.0.lock().expect("number allocator lock poisoned");
        let value = next.ok_or(IncidentError::InternalInvariantViolation(
            "the number allocator row was not loaded for this call",
        ))?;
        let following = value.checked_add(1).ok_or(IncidentError::CapacityExceeded(
            "incident number counter exhausted",
        ))?;
        *next = Some(following);
        Ok(IncidentNumber::from_sequence(allocation_year, value))
    }
}
