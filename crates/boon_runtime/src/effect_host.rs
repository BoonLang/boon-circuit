use boon_plan::EffectId;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::{TransientEffectCallId, TransientEffectCreditGrant, TransientEffectInvocation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectHostCoreError {
    detail: String,
}

impl EffectHostCoreError {
    fn new(detail: impl ToString) -> Self {
        Self {
            detail: detail.to_string(),
        }
    }
}

impl fmt::Display for EffectHostCoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for EffectHostCoreError {}

const EFFECT_CANCEL_REQUESTED: u8 = 1 << 0;
const EFFECT_TIMED_OUT: u8 = 1 << 1;
const EFFECT_DISCARDED: u8 = 1 << 2;
const EFFECT_COMMITTING: u8 = 1 << 3;
const EFFECT_COMMITTED: u8 = 1 << 4;
const EFFECT_TERMINAL_RESERVED: u8 = 1 << 5;

/// Shared linearization point for host-effect cancellation and publication.
///
/// A host worker must acquire the commit permit before making an irreversible
/// external change. Cancellation or timeout accepted before that acquisition
/// prevents the change. Once commit begins it wins, while a later owner discard
/// still suppresses delivery of the now-stale result.
#[derive(Clone, Debug, Default)]
pub struct EffectCommitPermit {
    state: Arc<AtomicU8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectStopReason {
    Cancelled,
    TimedOut,
    Discarded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectStopDisposition {
    Accepted(EffectStopReason),
    AlreadyStopped(EffectStopReason),
    CommitAlreadyStarted,
    TerminalAlreadyReserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectCommitDenied {
    Stopped(EffectStopReason),
    CommitAlreadyStarted,
    TerminalAlreadyReserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectTerminalReservation {
    Deliver,
    Cancelled,
    TimedOut,
    Discarded,
    AlreadyReserved,
}

#[derive(Debug)]
pub struct EffectCommitGuard {
    permit: EffectCommitPermit,
    settled: bool,
}

impl EffectCommitGuard {
    pub fn finish(mut self) {
        self.settle();
    }

    fn settle(&mut self) {
        if self.settled {
            return;
        }
        self.permit.finish_commit_state();
        self.settled = true;
    }
}

impl Drop for EffectCommitGuard {
    fn drop(&mut self) {
        self.settle();
    }
}

impl EffectCommitPermit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_cancel(&self) -> EffectStopDisposition {
        self.request_stop(EFFECT_CANCEL_REQUESTED, EffectStopReason::Cancelled)
    }

    pub fn request_timeout(&self) -> EffectStopDisposition {
        self.request_stop(EFFECT_TIMED_OUT, EffectStopReason::TimedOut)
    }

    fn request_stop(&self, requested: u8, reason: EffectStopReason) -> EffectStopDisposition {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & EFFECT_TERMINAL_RESERVED != 0 {
                return EffectStopDisposition::TerminalAlreadyReserved;
            }
            if state & (EFFECT_COMMITTING | EFFECT_COMMITTED) != 0 {
                return EffectStopDisposition::CommitAlreadyStarted;
            }
            if state & EFFECT_DISCARDED != 0 {
                return EffectStopDisposition::AlreadyStopped(EffectStopReason::Discarded);
            }
            if state & requested != 0 {
                return EffectStopDisposition::AlreadyStopped(reason);
            }
            if state & EFFECT_TIMED_OUT != 0 {
                return EffectStopDisposition::AlreadyStopped(EffectStopReason::TimedOut);
            }
            if state & EFFECT_CANCEL_REQUESTED != 0 {
                return EffectStopDisposition::AlreadyStopped(EffectStopReason::Cancelled);
            }
            match self.state.compare_exchange_weak(
                state,
                state | requested,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return EffectStopDisposition::Accepted(reason),
                Err(actual) => state = actual,
            }
        }
    }

    /// Discards an effect whose owning runtime scope no longer exists.
    ///
    /// This always suppresses later result delivery. It prevents publication
    /// unless the worker had already acquired the commit permit.
    pub fn discard(&self) {
        self.state.fetch_or(EFFECT_DISCARDED, Ordering::AcqRel);
    }

    pub fn stop_reason(&self) -> Option<EffectStopReason> {
        let state = self.state.load(Ordering::Acquire);
        if state & EFFECT_DISCARDED != 0 {
            Some(EffectStopReason::Discarded)
        } else if state & EFFECT_TIMED_OUT != 0 {
            Some(EffectStopReason::TimedOut)
        } else if state & EFFECT_CANCEL_REQUESTED != 0 {
            Some(EffectStopReason::Cancelled)
        } else {
            None
        }
    }

    pub fn begin_commit(&self) -> Result<EffectCommitGuard, EffectCommitDenied> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & EFFECT_DISCARDED != 0 {
                return Err(EffectCommitDenied::Stopped(EffectStopReason::Discarded));
            }
            if state & EFFECT_TIMED_OUT != 0 {
                return Err(EffectCommitDenied::Stopped(EffectStopReason::TimedOut));
            }
            if state & EFFECT_CANCEL_REQUESTED != 0 {
                return Err(EffectCommitDenied::Stopped(EffectStopReason::Cancelled));
            }
            if state & EFFECT_TERMINAL_RESERVED != 0 {
                return Err(EffectCommitDenied::TerminalAlreadyReserved);
            }
            if state & (EFFECT_COMMITTING | EFFECT_COMMITTED) != 0 {
                return Err(EffectCommitDenied::CommitAlreadyStarted);
            }
            match self.state.compare_exchange_weak(
                state,
                state | EFFECT_COMMITTING,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(EffectCommitGuard {
                        permit: self.clone(),
                        settled: false,
                    });
                }
                Err(actual) => state = actual,
            }
        }
    }

    fn finish_commit_state(&self) {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            assert!(
                state & EFFECT_COMMITTING != 0,
                "effect publication finished without an acquired commit permit"
            );
            let next = (state & !EFFECT_COMMITTING) | EFFECT_COMMITTED;
            match self
                .state
                .compare_exchange_weak(state, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(actual) => state = actual,
            }
        }
    }

    /// Reserves the one terminal result and resolves any concurrent stop.
    pub fn reserve_terminal(&self) -> EffectTerminalReservation {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            if state & EFFECT_TERMINAL_RESERVED != 0 {
                return EffectTerminalReservation::AlreadyReserved;
            }
            let reservation = if state & EFFECT_DISCARDED != 0 {
                EffectTerminalReservation::Discarded
            } else if state & EFFECT_TIMED_OUT != 0 {
                EffectTerminalReservation::TimedOut
            } else if state & EFFECT_CANCEL_REQUESTED != 0 {
                EffectTerminalReservation::Cancelled
            } else {
                EffectTerminalReservation::Deliver
            };
            match self.state.compare_exchange_weak(
                state,
                state | EFFECT_TERMINAL_RESERVED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return reservation,
                Err(actual) => state = actual,
            }
        }
    }

    pub fn is_commit_started(&self) -> bool {
        self.state.load(Ordering::Acquire) & (EFFECT_COMMITTING | EFFECT_COMMITTED) != 0
    }

    pub fn is_discarded(&self) -> bool {
        self.state.load(Ordering::Acquire) & EFFECT_DISCARDED != 0
    }
}

#[derive(Clone, Debug)]
pub struct ExactCallHostCore<L>
where
    L: Copy + Debug + Eq,
{
    authorized: BTreeMap<EffectId, L>,
    owners: BTreeMap<TransientEffectCallId, L>,
    max_active: usize,
}

impl<L> ExactCallHostCore<L>
where
    L: Copy + Debug + Eq,
{
    pub fn new(
        authorized: BTreeMap<EffectId, L>,
        max_active: usize,
    ) -> Result<Self, EffectHostCoreError> {
        if max_active == 0 {
            return Err(EffectHostCoreError::new(
                "exact-call host active-call limit must be positive",
            ));
        }
        Ok(Self {
            authorized,
            owners: BTreeMap::new(),
            max_active,
        })
    }

    pub fn authorized_lane(&self, effect_id: EffectId) -> Option<L> {
        self.authorized.get(&effect_id).copied()
    }

    pub fn authorized_entries(&self) -> impl Iterator<Item = (EffectId, L)> + '_ {
        self.authorized
            .iter()
            .map(|(effect_id, lane)| (*effect_id, *lane))
    }

    pub fn admit(
        &mut self,
        calls: Vec<TransientEffectInvocation>,
    ) -> Result<Vec<(L, TransientEffectInvocation)>, EffectHostCoreError> {
        let mut candidate = self.owners.clone();
        let mut batch = BTreeSet::new();
        let mut admitted = Vec::with_capacity(calls.len());
        for call in calls {
            if candidate.contains_key(&call.call_id) || !batch.insert(call.call_id) {
                return Err(EffectHostCoreError::new(format_args!(
                    "exact-call host received duplicate active call {}",
                    call.call_id
                )));
            }
            let lane = self.authorized_lane(call.effect_id).ok_or_else(|| {
                EffectHostCoreError::new(format_args!(
                    "exact-call host is not authorized for effect {}",
                    call.effect_id
                ))
            })?;
            if candidate.len() >= self.max_active {
                return Err(EffectHostCoreError::new(format_args!(
                    "exact-call host exceeded its active-call limit {}",
                    self.max_active
                )));
            }
            candidate.insert(call.call_id, lane);
            admitted.push((lane, call));
        }
        self.owners = candidate;
        Ok(admitted)
    }

    pub fn credit_lanes(
        &self,
        grants: &[TransientEffectCreditGrant],
    ) -> Result<Vec<(L, TransientEffectCreditGrant)>, EffectHostCoreError> {
        grants
            .iter()
            .map(|grant| {
                self.owners
                    .get(&grant.call_id)
                    .copied()
                    .map(|lane| (lane, *grant))
                    .ok_or_else(|| {
                        EffectHostCoreError::new(format_args!(
                            "stream credit targets unowned call {}",
                            grant.call_id
                        ))
                    })
            })
            .collect()
    }

    pub fn cancel_calls(
        &mut self,
        calls: &[TransientEffectCallId],
    ) -> Vec<(L, TransientEffectCallId)> {
        calls
            .iter()
            .filter_map(|call_id| self.owners.remove(call_id).map(|lane| (lane, *call_id)))
            .collect()
    }

    pub fn accept_result(
        &mut self,
        call_id: TransientEffectCallId,
        lane: L,
        terminal: bool,
    ) -> Result<(), EffectHostCoreError> {
        if self.owners.get(&call_id) != Some(&lane) {
            return Err(EffectHostCoreError::new(format_args!(
                "effect adapter {:?} completed stale or foreign call {}",
                lane, call_id
            )));
        }
        if terminal {
            self.owners.remove(&call_id);
        }
        Ok(())
    }

    pub fn complete_single(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> Result<L, EffectHostCoreError> {
        self.owners.remove(&call_id).ok_or_else(|| {
            EffectHostCoreError::new(format_args!(
                "effect adapter completed stale or foreign call {call_id}"
            ))
        })
    }

    pub fn rollback_admitted(&mut self, calls: &[TransientEffectCallId]) {
        for call_id in calls {
            self.owners.remove(call_id);
        }
    }

    pub fn active_in_lane(&self, lane: L) -> bool {
        self.owners.values().any(|owner| *owner == lane)
    }

    pub fn active_call_ids(&self) -> Vec<TransientEffectCallId> {
        self.owners.keys().copied().collect()
    }

    pub fn active_count(&self) -> usize {
        self.owners.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_wins_before_publication_and_reserves_one_terminal() {
        let permit = EffectCommitPermit::new();
        assert_eq!(
            permit.request_cancel(),
            EffectStopDisposition::Accepted(EffectStopReason::Cancelled)
        );
        assert_eq!(
            permit.begin_commit().unwrap_err(),
            EffectCommitDenied::Stopped(EffectStopReason::Cancelled)
        );
        assert_eq!(
            permit.reserve_terminal(),
            EffectTerminalReservation::Cancelled
        );
        assert_eq!(
            permit.reserve_terminal(),
            EffectTerminalReservation::AlreadyReserved
        );
    }

    #[test]
    fn publication_wins_before_late_cancellation_but_owner_discard_suppresses_result() {
        let permit = EffectCommitPermit::new();
        let commit = permit.begin_commit().unwrap();
        assert_eq!(
            permit.request_cancel(),
            EffectStopDisposition::CommitAlreadyStarted
        );
        permit.discard();
        commit.finish();
        assert!(permit.is_commit_started());
        assert!(permit.is_discarded());
        assert_eq!(
            permit.reserve_terminal(),
            EffectTerminalReservation::Discarded
        );
        assert_eq!(
            permit.reserve_terminal(),
            EffectTerminalReservation::AlreadyReserved
        );
    }

    #[test]
    fn timeout_prevents_publication_and_overrides_the_terminal_result() {
        let permit = EffectCommitPermit::new();
        assert_eq!(
            permit.request_timeout(),
            EffectStopDisposition::Accepted(EffectStopReason::TimedOut)
        );
        assert_eq!(
            permit.begin_commit().unwrap_err(),
            EffectCommitDenied::Stopped(EffectStopReason::TimedOut)
        );
        assert_eq!(
            permit.reserve_terminal(),
            EffectTerminalReservation::TimedOut
        );
    }

    #[test]
    fn dropped_commit_guard_settles_after_a_fallible_publication_path() {
        let permit = EffectCommitPermit::new();
        {
            let _commit = permit.begin_commit().unwrap();
        }
        assert!(permit.is_commit_started());
        assert_eq!(
            permit.request_cancel(),
            EffectStopDisposition::CommitAlreadyStarted
        );
        assert_eq!(
            permit.reserve_terminal(),
            EffectTerminalReservation::Deliver
        );
    }

    #[test]
    fn first_stop_reason_wins_and_is_reported_to_later_requesters() {
        let permit = EffectCommitPermit::new();
        assert_eq!(
            permit.request_cancel(),
            EffectStopDisposition::Accepted(EffectStopReason::Cancelled)
        );
        assert_eq!(
            permit.request_timeout(),
            EffectStopDisposition::AlreadyStopped(EffectStopReason::Cancelled)
        );
    }
}
