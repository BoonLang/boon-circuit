use crate::TransientEffectHostError;
use boon_plan::EffectId;
use boon_runtime::{TransientEffectCallId, TransientEffectCreditGrant, TransientEffectInvocation};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

/// Platform-neutral ownership state for one exact-call transient effect host.
///
/// Adapter mechanics remain in their platform crate. This core owns admission,
/// duplicate rejection, cancellation, stream-credit correlation, and terminal
/// completion release without inspecting application values or example names.
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
    ) -> Result<Self, TransientEffectHostError> {
        if max_active == 0 {
            return Err(TransientEffectHostError::new(
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

    /// Atomically admits a batch and returns its exact adapter lanes.
    pub fn admit(
        &mut self,
        calls: Vec<TransientEffectInvocation>,
    ) -> Result<Vec<(L, TransientEffectInvocation)>, TransientEffectHostError> {
        let mut candidate = self.owners.clone();
        let mut batch = BTreeSet::new();
        let mut admitted = Vec::with_capacity(calls.len());
        for call in calls {
            if candidate.contains_key(&call.call_id) || !batch.insert(call.call_id) {
                return Err(TransientEffectHostError::new(format_args!(
                    "exact-call host received duplicate active call {}",
                    call.call_id
                )));
            }
            let lane = self.authorized_lane(call.effect_id).ok_or_else(|| {
                TransientEffectHostError::new(format_args!(
                    "exact-call host is not authorized for effect {}",
                    call.effect_id
                ))
            })?;
            if candidate.len() >= self.max_active {
                return Err(TransientEffectHostError::new(format_args!(
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

    /// Correlates credits without changing call ownership.
    pub fn credit_lanes(
        &self,
        grants: &[TransientEffectCreditGrant],
    ) -> Result<Vec<(L, TransientEffectCreditGrant)>, TransientEffectHostError> {
        grants
            .iter()
            .map(|grant| {
                self.owners
                    .get(&grant.call_id)
                    .copied()
                    .map(|lane| (lane, *grant))
                    .ok_or_else(|| {
                        TransientEffectHostError::new(format_args!(
                            "stream credit targets unowned call {}",
                            grant.call_id
                        ))
                    })
            })
            .collect()
    }

    /// Releases known calls and returns their exact adapter lanes. Unknown
    /// cancellation IDs are idempotently ignored.
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
    ) -> Result<(), TransientEffectHostError> {
        if self.owners.get(&call_id) != Some(&lane) {
            return Err(TransientEffectHostError::new(format_args!(
                "effect adapter {:?} completed stale or foreign call {}",
                lane, call_id
            )));
        }
        if terminal {
            self.owners.remove(&call_id);
        }
        Ok(())
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
