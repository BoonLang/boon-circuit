//! Replaceable summary effects, separate from permanent inference equalities.
//!
//! Sites belong to an exact invocation occurrence, not a resolved input type.
//! Register sites in bytecode order before evaluating their owner. Evaluation
//! stages a complete replacement; commit withdraws sites not visited this time.
//! No contributor is unioned with its destination here. The component solver
//! must fold current facts together with its separately retained base binding.
//!
//! An owner is one top-level summary activation. Nested invocation/effect paths
//! receive distinct sites under that same owner, not independent transactions:
//! skipping a child withdraws its effects, and a parent error publishes none of
//! its children's staged effects. Registration follows deterministic bytecode
//! occurrence order, never first runtime visitation order.

use crate::{TypeTermId, TypeVariableId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RequirementOwnerId(u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RequirementSiteId(u32);

#[derive(Clone, Copy)]
pub(super) struct RequirementProjection {
    owner: RequirementOwnerId,
    forward: RequirementSiteId,
    backward: RequirementSiteId,
    hole: TypeTermId,
}

#[derive(Default)]
struct Owner {
    sites: Vec<RequirementSiteId>,
    evaluating: bool,
    sealed: bool,
}

struct Site {
    owner: RequirementOwnerId,
    target: TypeVariableId,
    value: Option<TypeTermId>,
    staged: Option<TypeTermId>,
    next_target: Option<RequirementSiteId>,
}

#[derive(Clone, Default)]
struct Target {
    first: Option<RequirementSiteId>,
    last: Option<RequirementSiteId>,
    dirty: bool,
    /// Only structural field ordering is read from this receipt, never types.
    order: Option<TypeTermId>,
}

/// Dense IDs and retained buffers; reevaluation allocates no per-effect rows.
/// Targets retain original variable identities. Union-find canonicalization
/// belongs to the consumer, which must include all members of an alias class.
#[derive(Default)]
pub(super) struct RequirementContributions {
    pub(super) work: crate::KernelRequirementWork,
    owners: Vec<Owner>,
    sites: Vec<Site>,
    targets: Vec<Target>,
    dirty: Vec<TypeVariableId>,
}

impl RequirementContributions {
    pub(super) fn set_order(&mut self, target: TypeVariableId, order: Option<TypeTermId>) {
        self.targets.resize_with(
            self.targets.len().max(target.0 as usize + 1),
            Target::default,
        );
        self.targets[target.0 as usize].order = order;
    }
    pub(super) fn owner(&mut self) -> RequirementOwnerId {
        self.work.owners = self.work.owners.saturating_add(1);
        let id = RequirementOwnerId(
            self.owners
                .len()
                .try_into()
                .expect("requirement owner overflow"),
        );
        self.owners.push(Owner::default());
        id
    }

    pub(super) fn site(
        &mut self,
        owner: RequirementOwnerId,
        target: TypeVariableId,
    ) -> RequirementSiteId {
        self.work.sites = self.work.sites.saturating_add(1);
        assert!(
            !self.owners[owner.0 as usize].sealed,
            "register all nested effect sites before first evaluation"
        );
        let id = RequirementSiteId(
            self.sites
                .len()
                .try_into()
                .expect("requirement site overflow"),
        );
        self.sites.push(Site {
            owner,
            target,
            value: None,
            staged: None,
            next_target: None,
        });
        self.owners[owner.0 as usize].sites.push(id);
        self.targets.resize_with(
            self.targets.len().max(target.0 as usize + 1),
            Target::default,
        );
        let target = &mut self.targets[target.0 as usize];
        if let Some(last) = target.last {
            self.sites[last.0 as usize].next_target = Some(id);
        } else {
            target.first = Some(id);
        }
        target.last = Some(id);
        id
    }

    pub(super) fn begin(&mut self, owner: RequirementOwnerId) {
        self.work.activations = self.work.activations.saturating_add(1);
        let owner = &mut self.owners[owner.0 as usize];
        assert!(
            !owner.evaluating,
            "nested effects share their enclosing activation transaction"
        );
        owner.evaluating = true;
        owner.sealed = true;
        for site in &owner.sites {
            self.work.begin_site_visits = self.work.begin_site_visits.saturating_add(1);
            self.sites[site.0 as usize].staged = None;
        }
    }

    pub(super) fn stage(
        &mut self,
        owner: RequirementOwnerId,
        site: RequirementSiteId,
        value: TypeTermId,
    ) {
        self.work.staged_writes = self.work.staged_writes.saturating_add(1);
        assert!(self.owners[owner.0 as usize].evaluating);
        let site = &mut self.sites[site.0 as usize];
        assert_eq!(
            site.owner, owner,
            "effect belongs to a different occurrence"
        );
        // One bytecode effect is evaluated at most once in an activation.
        // Repeating it with the same fact is harmless; conflicting writes are
        // an identity bug, not a reason to silently retain only the last one.
        assert!(site.staged.is_none_or(|previous| previous == value));
        site.staged = Some(value);
    }

    pub(super) fn commit(&mut self, owner: RequirementOwnerId) {
        let owner = &mut self.owners[owner.0 as usize];
        assert!(owner.evaluating);
        owner.evaluating = false;
        for id in &owner.sites {
            self.work.commit_site_visits = self.work.commit_site_visits.saturating_add(1);
            let site = &mut self.sites[id.0 as usize];
            if site.value == site.staged {
                continue;
            }
            self.work.changed_sites = self.work.changed_sites.saturating_add(1);
            if site.staged.is_none() {
                self.work.withdrawn_sites = self.work.withdrawn_sites.saturating_add(1);
            }
            site.value = site.staged;
            let target = &mut self.targets[site.target.0 as usize];
            if !target.dirty {
                target.dirty = true;
                self.dirty.push(site.target);
            }
        }
    }

    /// An evaluation error must not publish a partially visited branch set.
    pub(super) fn abort(&mut self, owner: RequirementOwnerId) {
        let owner = &mut self.owners[owner.0 as usize];
        assert!(owner.evaluating);
        owner.evaluating = false;
    }

    /// Site ordinals let the solver merge aliased targets in authored order,
    /// independent of union-find rank or alias establishment order. Raw term
    /// identity is not dependency currentness: watch referenced variables too.
    pub(super) fn facts(
        &self,
        target: TypeVariableId,
    ) -> impl Iterator<Item = (RequirementSiteId, TypeTermId)> + '_ {
        let mut next = self
            .targets
            .get(target.0 as usize)
            .and_then(|target| target.first);
        std::iter::from_fn(move || {
            while let Some(id) = next {
                let site = &self.sites[id.0 as usize];
                next = site.next_target;
                if let Some(value) = site.value {
                    return Some((id, value));
                }
            }
            None
        })
    }

    #[cfg(test)]
    fn values(&self, target: TypeVariableId) -> impl Iterator<Item = TypeTermId> + '_ {
        self.facts(target).map(|(_, value)| value)
    }

    /// Only destinations affected by committed changes, never a provider scan.
    pub(super) fn mark_dirty(&mut self, target: TypeVariableId) {
        self.targets.resize_with(
            self.targets.len().max(target.0 as usize + 1),
            Target::default,
        );
        let entry = &mut self.targets[target.0 as usize];
        if !entry.dirty {
            entry.dirty = true;
            self.dirty.push(target);
        }
    }

    pub(super) fn pop_dirty(&mut self) -> Option<TypeVariableId> {
        let target = self.dirty.pop()?;
        self.targets[target.0 as usize].dirty = false;
        Some(target)
    }

    pub(super) fn has_dirty(&self) -> bool {
        !self.dirty.is_empty()
    }
}

impl super::ComponentSolver {
    pub(super) fn project_requirement(
        &mut self,
        provider: TypeVariableId,
        field: Option<boon_contract::SymbolId>,
        pattern: Option<(crate::PackedKernelPattern, &[boon_contract::SymbolId])>,
        consumer: TypeVariableId,
    ) {
        self.requirements.work.projection_evaluations = self
            .requirements
            .work
            .projection_evaluations
            .saturating_add(1);
        self.refresh_requirements();
        let projection = if let Some(projection) = self.requirement_projections.get(&consumer) {
            *projection
        } else {
            let owner = self.requirements.owner();
            let projection = RequirementProjection {
                owner,
                forward: self.register_requirement_site(owner, consumer),
                backward: self.register_requirement_site(owner, provider),
                hole: self.new_contextual_hole_term(),
            };
            self.requirement_projections.insert(consumer, projection);
            projection
        };
        let provider_term = self.program.terms.variable(provider);
        let value = self.resolve_term_head(provider_term);
        let projected = if let Some((pattern, fields)) = pattern {
            self.narrow_pattern_payload(value, pattern)
                .and_then(|payload| self.project_path_term(payload, fields))
        } else if let Some(field) = field {
            self.project_field(value, field)
        } else {
            Some(value)
        };
        let consumer_root = self.root(consumer);
        let base = self.cells[consumer_root.0 as usize]
            .requirement_base
            .flatten();
        // Backflow sees independently authored consumer constraints, never the
        // forward value just contributed by this same projection.
        let backflow = if let Some((pattern, fields)) = pattern {
            self.pattern_projection_scaffold(pattern, fields, base.unwrap_or(projection.hole))
        } else if let Some(field) = field {
            Some(
                self.program
                    .terms
                    .object([(field, base.unwrap_or(projection.hole))], true),
            )
        } else {
            base
        };
        if let Some(operation) = self.active_operation {
            // The directional and requirement sides can change each other's
            // inputs. Let this exact equation settle through the existing queue.
            self.self_replayable[operation.0 as usize] = true;
        }
        self.requirements.begin(projection.owner);
        if let Some(value) = projected {
            self.requirements
                .stage(projection.owner, projection.forward, value);
        }
        if let Some(requirement) = backflow {
            self.requirements
                .stage(projection.owner, projection.backward, requirement);
        }
        self.requirements.commit(projection.owner);
        self.refresh_requirements();
    }

    pub(super) fn begin_summary_requirements(
        &mut self,
        output: TypeVariableId,
        inputs: &[crate::KernelSummaryCallInput],
    ) {
        if !self.summary_requirement_calls.contains_key(&output) {
            let owner = self.requirements.owner();
            let sites = inputs
                .iter()
                .map(|input| match input {
                    crate::KernelSummaryCallInput::Projection {
                        requirement: Some(path),
                        ..
                    } => Some(self.register_requirement_site(owner, path.provider)),
                    _ => None,
                })
                .collect();
            self.summary_requirement_calls
                .insert(output, (owner, sites));
        }
        let owner = self.summary_requirement_calls[&output].0;
        self.requirements.begin(owner);
        self.summary_requirement_values.clear();
        self.summary_requirement_values.resize(inputs.len(), None);
    }

    pub(super) fn finish_summary_requirements(
        &mut self,
        output: TypeVariableId,
        inputs: &[crate::KernelSummaryCallInput],
        success: bool,
    ) {
        let owner = self.summary_requirement_calls[&output].0;
        if !success {
            self.requirements.abort(owner);
            return;
        }
        for (index, input) in inputs.iter().enumerate() {
            let Some(mut term) = self.summary_requirement_values[index] else {
                continue;
            };
            let Some(site) = self.summary_requirement_calls[&output].1[index] else {
                continue;
            };
            let crate::KernelSummaryCallInput::Projection { steps, .. } = input else {
                unreachable!()
            };
            let mut valid = true;
            for step in steps.iter().rev() {
                self.requirements.work.path_step_evaluations = self
                    .requirements
                    .work
                    .path_step_evaluations
                    .saturating_add(1);
                term = match &step.projection {
                    crate::KernelSummaryProjection::Whole => term,
                    crate::KernelSummaryProjection::Field(field) => {
                        self.program.terms.object([(*field, term)], true)
                    }
                    crate::KernelSummaryProjection::Pattern { pattern, fields } => {
                        let Some(scaffold) =
                            self.pattern_projection_scaffold(*pattern, fields, term)
                        else {
                            valid = false;
                            break;
                        };
                        scaffold
                    }
                };
            }
            if valid {
                self.requirements.stage(owner, site, term);
            }
        }
        self.requirements.commit(owner);
        self.refresh_requirements();
    }

    pub(super) fn constrain_summary_value(
        &mut self,
        actual: &mut super::SummaryValue,
        expected: TypeTermId,
    ) {
        if let Some(input) = actual.requirement {
            self.accumulate_summary_requirement(input, expected);
            // Within the activation a still-open value can use the inferred
            // constraint without publishing any caller mutation before commit.
            if let crate::TypeTerm::Variable(variable) = self.program.terms.term(actual.term) {
                let root = self.root_readonly(variable);
                if !self.cells[root.0 as usize].authoritative_provider {
                    actual.term = expected;
                }
            }
        } else {
            self.unify_terms(actual.term, expected);
        }
    }

    pub(super) fn accumulate_summary_requirement(&mut self, input: u32, term: TypeTermId) {
        let previous = self.summary_requirement_values[input as usize];
        let requirement = match previous {
            None => term,
            Some(previous) => self.merge_type_evidence(previous, term, false),
        };
        self.summary_requirement_values[input as usize] = Some(requirement);
    }

    /// Pure value-side read. Missing open paths use preallocated private holes;
    /// only explicit constraint effects create a requirement scaffold.
    pub(super) fn read_summary_requirement_value(
        &mut self,
        provider: TypeVariableId,
        steps: &[crate::KernelSummaryProjectionStep],
    ) -> TypeTermId {
        let mut term = self.program.terms.variable(provider);
        for step in steps {
            self.requirements.work.path_step_evaluations = self
                .requirements
                .work
                .path_step_evaluations
                .saturating_add(1);
            term = self.resolve_term_head(term);
            let open = matches!(
                self.program.terms.term(term),
                crate::TypeTerm::Variable(_) | crate::TypeTerm::Unknown
            );
            let projected = match &step.projection {
                crate::KernelSummaryProjection::Whole => Some(term),
                crate::KernelSummaryProjection::Field(field) => self.project_field(term, *field),
                crate::KernelSummaryProjection::Pattern { pattern, fields } => self
                    .narrow_pattern_payload(term, *pattern)
                    .and_then(|payload| self.project_path_term(payload, fields)),
            };
            term = if let Some(projected) = projected {
                projected
            } else if open
                || match &step.projection {
                    crate::KernelSummaryProjection::Field(field) => {
                        self.open_shape_may_contain_field(term, *field)
                    }
                    _ => false,
                }
            {
                self.program.terms.variable(step.consumer)
            } else {
                let reason = match &step.projection {
                    crate::KernelSummaryProjection::Field(field) => format!(
                        "authoritative provider omits projection `{}`",
                        self.program.terms.name(*field)
                    ),
                    crate::KernelSummaryProjection::Pattern { pattern, .. } => format!(
                        "authoritative provider does not satisfy pattern projection {}",
                        self.pattern_diagnostic_debug(*pattern)
                    ),
                    crate::KernelSummaryProjection::Whole => unreachable!(),
                };
                self.program.terms.unresolved_shape(reason)
            };
        }
        self.resolve_term_head(term)
    }

    pub(super) fn register_requirement_site(
        &mut self,
        owner: RequirementOwnerId,
        target: TypeVariableId,
    ) -> RequirementSiteId {
        let root = self.root(target);
        let cell = &mut self.cells[root.0 as usize];
        if cell.requirement_base.is_none() {
            self.requirements.set_order(root, cell.binding);
        }
        cell.requirement_base.get_or_insert(cell.binding);
        self.requirements.site(owner, target)
    }

    /// Live term-arena intern count while the phase probe is enabled.
    fn probe_intern_snapshot(&self) -> Option<u64> {
        self.requirement_phase_probe
            .is_some()
            .then(|| self.program.terms.work().intern_requests)
    }

    /// Refresh derived bindings before dequeuing another operation. Raw input
    /// terms remain the dependency authority even when their current resolved
    /// aggregate is closed, unchanged, or has widened away a payload.
    pub(super) fn refresh_requirements(&mut self) {
        self.requirement_previous_bindings.clear();
        let phase_started = self
            .requirement_phase_probe
            .is_some()
            .then(std::time::Instant::now);
        if let Some(probe) = self.requirement_phase_probe.as_mut() {
            probe.calls = probe.calls.saturating_add(1);
        }
        let has_dirty = self.requirements.has_dirty();
        if let Some(probe) = self.requirement_phase_probe.as_mut() {
            if has_dirty {
                probe.dirty_refreshes = probe.dirty_refreshes.saturating_add(1);
            } else {
                probe.empty_refreshes = probe.empty_refreshes.saturating_add(1);
            }
        }
        let invalidate_intern = self.probe_intern_snapshot();
        let invalidate_started = self
            .requirement_phase_probe
            .is_some()
            .then(std::time::Instant::now);
        self.invalidate_requirement_cone();
        let invalidate_ns = invalidate_started
            .map(|started| u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
        let invalidate_interned = invalidate_intern.map(|before| {
            self.program
                .terms
                .work()
                .intern_requests
                .saturating_sub(before)
        });
        if let Some(probe) = self.requirement_phase_probe.as_mut() {
            if let Some(ns) = invalidate_ns {
                probe.invalidate_ns = probe.invalidate_ns.saturating_add(ns);
            }
            if let Some(interned) = invalidate_interned {
                probe.intern_invalidate = probe.intern_invalidate.saturating_add(interned);
            }
        }
        while let Some(target) = self.requirements.pop_dirty() {
            let target = self.root(target);
            let Some(base) = self.cells[target.0 as usize].requirement_base else {
                continue;
            };
            self.requirements.work.aggregate_evaluations = self
                .requirements
                .work
                .aggregate_evaluations
                .saturating_add(1);
            let collect_intern = self.probe_intern_snapshot();
            let collect_started = self
                .requirement_phase_probe
                .is_some()
                .then(std::time::Instant::now);
            let mut facts = self.requirement_fact_scratch.take();
            let mut member = Some(self.equivalence_head[target.0 as usize]);
            while let Some(variable) = member {
                facts.extend(self.requirements.facts(variable));
                member = self.equivalence_next[variable.0 as usize];
            }
            facts.sort_unstable_by_key(|(site, _)| *site);
            let mut inputs = self.term_id_scratch.take();
            inputs.extend(base);
            inputs.extend(facts.iter().map(|(_, term)| *term));
            self.requirement_fact_scratch.recycle(facts);
            let collect_ns = collect_started
                .map(|started| u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            let collect_interned = collect_intern.map(|before| {
                self.program
                    .terms
                    .work()
                    .intern_requests
                    .saturating_sub(before)
            });
            if let Some(probe) = self.requirement_phase_probe.as_mut() {
                if let Some(ns) = collect_ns {
                    probe.collect_ns = probe.collect_ns.saturating_add(ns);
                }
                if let Some(interned) = collect_interned {
                    probe.intern_collect = probe.intern_collect.saturating_add(interned);
                }
            }
            #[cfg(debug_assertions)]
            if self.aggregate_probe.is_some() {
                let mut signature = Vec::with_capacity(inputs.len());
                for index in 0..inputs.len() {
                    let term = inputs[index];
                    signature.push(self.resolve_term(term).0);
                }
                signature.sort_unstable();
                let distinct = {
                    let mut distinct = signature.clone();
                    distinct.dedup();
                    distinct.len()
                };
                let closed = signature
                    .iter()
                    .all(|term| !self.program.terms.has_variable(crate::TypeTermId(*term)));
                let mut variant_terms = 0_usize;
                let mut object_terms = 0_usize;
                let mut other_terms = 0_usize;
                for term in &signature {
                    match self.program.terms.term_head(crate::TypeTermId(*term)) {
                        crate::TypeTermHead::VariantSet(_) => variant_terms += 1,
                        crate::TypeTermHead::Object { .. } => object_terms += 1,
                        _ => other_terms += 1,
                    }
                }
                let probe = self.aggregate_probe.as_mut().expect("probe checked above");
                probe.folds = probe.folds.saturating_add(1);
                if other_terms == 0 && object_terms == 0 {
                    probe.variant_folds = probe.variant_folds.saturating_add(1);
                } else if other_terms == 0 && variant_terms == 0 {
                    probe.object_folds = probe.object_folds.saturating_add(1);
                } else {
                    probe.mixed_kind_folds = probe.mixed_kind_folds.saturating_add(1);
                }
                if closed {
                    probe.closed_folds = probe.closed_folds.saturating_add(1);
                } else {
                    probe.mixed_folds = probe.mixed_folds.saturating_add(1);
                }
                probe.visits = probe
                    .visits
                    .saturating_add(u64::try_from(signature.len()).unwrap_or(u64::MAX));
                probe.distinct_visits = probe
                    .distinct_visits
                    .saturating_add(u64::try_from(distinct).unwrap_or(u64::MAX));
                if let Some(previous) = probe.last.get(&target.0) {
                    if previous == &signature {
                        probe.repeat_folds = probe.repeat_folds.saturating_add(1);
                    } else {
                        let added = signature
                            .iter()
                            .filter(|term| !previous.contains(term))
                            .count();
                        let removed = previous
                            .iter()
                            .filter(|term| !signature.contains(term))
                            .count();
                        if added + removed == 1 {
                            probe.single_delta_folds = probe.single_delta_folds.saturating_add(1);
                        }
                    }
                }
                probe.last.insert(target.0, signature);
            }
            let deps_started = self
                .requirement_phase_probe
                .is_some()
                .then(std::time::Instant::now);
            self.replace_binding_dependencies_from(target, &inputs);
            let deps_ns = deps_started
                .map(|started| u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            if let Some(probe) = self.requirement_phase_probe.as_mut() {
                if let Some(ns) = deps_ns {
                    probe.deps_ns = probe.deps_ns.saturating_add(ns);
                }
            }
            // Folding is order-sensitive: merges can bind payload variables,
            // so resolve in the same interleaved order as before. Duplicate
            // contributions (aliases and repeated arm terms) are skipped
            // because merging an already-folded term cannot add evidence; on
            // TodoMVC only about 8% of the visited terms are distinct.
            let mut folded_terms = self.term_id_scratch.take();
            let mut aggregate = None;
            for term in inputs.iter().copied() {
                // Match the existing recursive-shape guard without equating
                // any contributor variable to its destination.
                let open = self.program.terms.has_variable(term);
                let flat = open
                    && self
                        .term_variable_cache
                        .get(term.0 as usize)
                        .and_then(|cached| cached.as_deref())
                        .is_some_and(|variables| {
                            variables.iter().all(|variable| {
                                self.root_readonly(*variable) == *variable
                                    && self.cells[variable.0 as usize].binding.is_none()
                            })
                        });
                if let Some(probe) = self.requirement_phase_probe.as_mut() {
                    if open {
                        probe.open_contributor_visits =
                            probe.open_contributor_visits.saturating_add(1);
                        if flat {
                            probe.flat_contributor_visits =
                                probe.flat_contributor_visits.saturating_add(1);
                        }
                    } else {
                        probe.closed_contributor_visits =
                            probe.closed_contributor_visits.saturating_add(1);
                    }
                }
                let check_intern = self.probe_intern_snapshot();
                let check_started = self
                    .requirement_phase_probe
                    .is_some()
                    .then(std::time::Instant::now);
                let resolved = self.resolve_requirement_contributor(target, term);
                let check_ns = check_started
                    .map(|started| u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
                let check_interned = check_intern.map(|before| {
                    self.program
                        .terms
                        .work()
                        .intern_requests
                        .saturating_sub(before)
                });
                if let Some(probe) = self.requirement_phase_probe.as_mut() {
                    if let Some(ns) = check_ns {
                        probe.contributor_check_ns = probe.contributor_check_ns.saturating_add(ns);
                    }
                    if let Some(interned) = check_interned {
                        probe.intern_resolve = probe.intern_resolve.saturating_add(interned);
                    }
                }
                #[cfg(debug_assertions)]
                {
                    let separate = if self.occurs(target, term) {
                        None
                    } else {
                        Some(self.resolve_term(term))
                    };
                    debug_assert_eq!(
                        separate, resolved,
                        "the combined occurs/resolve check must match the separate walks"
                    );
                }
                let Some(term) = resolved else {
                    continue;
                };
                if folded_terms.contains(&term) {
                    continue;
                }
                folded_terms.push(term);
            }
            // Closed contributions cannot be affected by merge side effects,
            // so a destination whose closed signature repeats can restore the
            // previously folded aggregate exactly instead of re-merging.
            let all_closed = folded_terms
                .iter()
                .all(|term| !self.program.terms.has_variable(*term));
            let mut memo_hit = false;
            if all_closed
                && let Some((signature, stored)) = self.requirement_fold_memo.get(&target.0)
                && signature.as_ref() == folded_terms.as_slice()
            {
                aggregate = *stored;
                memo_hit = true;
            }
            if !memo_hit {
                let merge_intern = self.probe_intern_snapshot();
                let merge_started = self
                    .requirement_phase_probe
                    .is_some()
                    .then(std::time::Instant::now);
                // Ordered fold with retained prefixes: an unchanged prefix is
                // restored and only the changed suffix is merged.
                let previous_order = self
                    .requirement_fold_state
                    .get(&target.0)
                    .map(|state| (state.merged, state.ordered_receipt, state.ordered));
                aggregate = self.fold_requirement_terms(target, &folded_terms);
                if let Some(probe) = self.requirement_phase_probe.as_mut() {
                    probe.folds = probe.folds.saturating_add(1);
                }
                if let Some(started) = merge_started
                    && let Some(probe) = self.requirement_phase_probe.as_mut()
                {
                    probe.merge_ns = probe.merge_ns.saturating_add(
                        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                    );
                }
                let merge_interned = merge_intern.map(|before| {
                    self.program
                        .terms
                        .work()
                        .intern_requests
                        .saturating_sub(before)
                });
                if let Some(probe) = self.requirement_phase_probe.as_mut()
                    && let Some(interned) = merge_interned
                {
                    probe.intern_merge = probe.intern_merge.saturating_add(interned);
                }
                let order_intern = self.probe_intern_snapshot();
                let order_started = self
                    .requirement_phase_probe
                    .is_some()
                    .then(std::time::Instant::now);
                let receipt = self.requirements.targets[target.0 as usize].order;
                let reused_order = previous_order
                    .filter(|(merged, ordered_receipt, _)| {
                        *merged == aggregate && *ordered_receipt == receipt
                    })
                    .map(|(_, _, ordered)| ordered);
                if let Some(retained) = reused_order
                    && !cfg!(debug_assertions)
                {
                    aggregate = retained;
                    if let Some(probe) = self.requirement_phase_probe.as_mut() {
                        probe.order_reuses = probe.order_reuses.saturating_add(1);
                    }
                } else {
                    if let (Some(previous), Some(current)) = (receipt, aggregate) {
                        aggregate = Some(self.retain_requirement_order(previous, current));
                    }
                    #[cfg(debug_assertions)]
                    if let Some(expected) = reused_order {
                        debug_assert_eq!(
                            aggregate, expected,
                            "a repeated (receipt, merged) pair must order to the retained aggregate"
                        );
                    }
                }
                self.record_requirement_fold_order(target, receipt, aggregate);
                let order_ns = order_started
                    .map(|started| u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
                let order_interned = order_intern.map(|before| {
                    self.program
                        .terms
                        .work()
                        .intern_requests
                        .saturating_sub(before)
                });
                if let Some(probe) = self.requirement_phase_probe.as_mut() {
                    if let Some(ns) = order_ns {
                        probe.order_ns = probe.order_ns.saturating_add(ns);
                    }
                    if let Some(interned) = order_interned {
                        probe.intern_order = probe.intern_order.saturating_add(interned);
                    }
                }
                if all_closed {
                    self.requirement_fold_memo.insert(
                        target.0,
                        (folded_terms.clone().into_boxed_slice(), aggregate),
                    );
                }
            } else if let Some(probe) = self.requirement_phase_probe.as_mut() {
                probe.memo_hits = probe.memo_hits.saturating_add(1);
            }
            if memo_hit {
                // The memo stores the published aggregate, so its receipt is
                // the one currently installed for this destination.
                let receipt = self.requirements.targets[target.0 as usize].order;
                self.record_requirement_fold_order(target, receipt, aggregate);
            }
            self.term_id_scratch.recycle(folded_terms);
            self.term_id_scratch.recycle(inputs);
            let commit_intern = self.probe_intern_snapshot();
            let commit_started = self
                .requirement_phase_probe
                .is_some()
                .then(std::time::Instant::now);
            let current_binding = self.cells[target.0 as usize].binding;
            let previous_binding = self
                .requirement_previous_bindings
                .remove(&target.0)
                .or(current_binding);
            self.requirements.set_order(target, aggregate);
            if current_binding != aggregate {
                self.cells[target.0 as usize].binding = aggregate;
            }
            // Only a real binding change can move a consumer's input, so an
            // unchanged recomputed aggregate restores its binding without the
            // schedule_variable walk.
            if previous_binding != aggregate {
                self.touch(target);
            } else if let Some(probe) = self.requirement_phase_probe.as_mut() {
                probe.commit_skipped_touches = probe.commit_skipped_touches.saturating_add(1);
            }
            let commit_ns = commit_started
                .map(|started| u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
            let commit_interned = commit_intern.map(|before| {
                self.program
                    .terms
                    .work()
                    .intern_requests
                    .saturating_sub(before)
            });
            if let Some(probe) = self.requirement_phase_probe.as_mut() {
                if let Some(ns) = commit_ns {
                    probe.commit_ns = probe.commit_ns.saturating_add(ns);
                }
                if let Some(interned) = commit_interned {
                    probe.intern_commit = probe.intern_commit.saturating_add(interned);
                }
            }
        }
        if let Some(started) = phase_started
            && let Some(probe) = self.requirement_phase_probe.as_mut()
        {
            probe.total_ns = probe
                .total_ns
                .saturating_add(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
        }
        debug_assert!(
            self.requirement_previous_bindings.is_empty(),
            "every cleared binding is refolded in the refresh that cleared it"
        );
    }

    /// Preserve only the order of surviving fields. Every value, shape kind,
    /// openness bit and variable identity comes from the newly derived term.
    /// Removed fields are never reintroduced through an ordering receipt.
    ///
    /// The receipt walk almost always rebuilds the term it was given, so the
    /// entry point first asks whether the current term already satisfies the
    /// receipt order and skips the walk when it does.
    fn retain_requirement_order(
        &mut self,
        previous: TypeTermId,
        current: TypeTermId,
    ) -> TypeTermId {
        if previous == current {
            return current;
        }
        if self.requirement_order_matches(previous, current) {
            if let Some(probe) = self.requirement_phase_probe.as_mut() {
                probe.order_early_matches = probe.order_early_matches.saturating_add(1);
            }
            #[cfg(debug_assertions)]
            {
                let walked = self.retain_requirement_order_slow(previous, current);
                debug_assert_eq!(
                    walked, current,
                    "an early order match must equal what the receipt walk rebuilds"
                );
            }
            return current;
        }
        self.retain_requirement_order_slow(previous, current)
    }

    /// Whether the receipt walk would rebuild `current` unchanged: every
    /// surviving field must already appear in the receipt's relative order,
    /// new-only fields must follow them, and every nested payload must match
    /// recursively.
    fn requirement_order_matches(&mut self, previous: TypeTermId, current: TypeTermId) -> bool {
        use crate::{TypeTermHead as H, VariantTerm};
        if previous == current {
            return true;
        }
        match (
            self.program.terms.term_head(previous),
            self.program.terms.term_head(current),
        ) {
            (H::Object { shape: old, .. }, H::Object { shape: new, .. }) => {
                let old_len = self.program.terms.object_fields_for_shape(old).len();
                let new_len = self.program.terms.object_fields_for_shape(new).len();
                let mut old_ordinal = 0;
                let mut seen_new_only = false;
                for ordinal in 0..new_len {
                    let new_name = self
                        .program
                        .terms
                        .object_field_for_shape(new, ordinal)
                        .expect("sealed receipt field exists")
                        .name;
                    let new_ty = self
                        .program
                        .terms
                        .object_field_for_shape(new, ordinal)
                        .expect("sealed receipt field exists")
                        .ty;
                    if self
                        .program
                        .terms
                        .lookup_object_field(old, new_name)
                        .is_none()
                    {
                        seen_new_only = true;
                        continue;
                    }
                    if seen_new_only {
                        return false;
                    }
                    let mut found = false;
                    while old_ordinal < old_len {
                        let old_field = self
                            .program
                            .terms
                            .object_field_for_shape(old, old_ordinal)
                            .expect("sealed receipt field exists");
                        if old_field.name == new_name {
                            if !self.requirement_order_matches(old_field.ty, new_ty) {
                                return false;
                            }
                            old_ordinal += 1;
                            found = true;
                            break;
                        }
                        if self
                            .program
                            .terms
                            .lookup_object_field(new, old_field.name)
                            .is_some()
                        {
                            return false;
                        }
                        old_ordinal += 1;
                    }
                    if !found {
                        return false;
                    }
                }
                while old_ordinal < old_len {
                    let old_field = self
                        .program
                        .terms
                        .object_field_for_shape(old, old_ordinal)
                        .expect("sealed receipt field exists");
                    if self
                        .program
                        .terms
                        .lookup_object_field(new, old_field.name)
                        .is_some()
                    {
                        return false;
                    }
                    old_ordinal += 1;
                }
                true
            }
            (H::VariantSet(old), H::VariantSet(new)) => {
                for ordinal in 0..new.len() {
                    let VariantTerm::Tagged { tag, fields } =
                        self.program.terms.variant_terms(new)[ordinal]
                    else {
                        continue;
                    };
                    let prior =
                        self.program.terms.variant_terms(old).iter().find_map(
                            |prior| match prior {
                                VariantTerm::Tagged {
                                    tag: old_tag,
                                    fields,
                                } if *old_tag == tag => Some(*fields),
                                _ => None,
                            },
                        );
                    if let Some(prior) = prior
                        && !self.requirement_order_matches(prior, fields)
                    {
                        return false;
                    }
                }
                true
            }
            (H::List(old), H::List(new)) => self.requirement_order_matches(old, new),
            (H::Set(old), H::Set(new)) => self.requirement_order_matches(old, new),
            (
                H::Map {
                    key: old_key,
                    value: old_value,
                },
                H::Map { key, value },
            ) => {
                self.requirement_order_matches(old_key, key)
                    && self.requirement_order_matches(old_value, value)
            }
            (
                H::Function {
                    args: old,
                    result: old_result,
                    ..
                },
                H::Function {
                    args,
                    result: current_result,
                    ..
                },
            ) if old.len() == args.len() => {
                for ordinal in 0..args.len() {
                    let old_arg = self.program.terms.term_ids(old)[ordinal];
                    let new_arg = self.program.terms.term_ids(args)[ordinal];
                    if !self.requirement_order_matches(old_arg, new_arg) {
                        return false;
                    }
                }
                self.requirement_order_matches(old_result, current_result)
            }
            _ => true,
        }
    }

    /// The receipt walk itself: rebuild the term in the receipt's order.
    fn retain_requirement_order_slow(
        &mut self,
        previous: TypeTermId,
        current: TypeTermId,
    ) -> TypeTermId {
        use crate::{TypeTermHead as H, VariantTerm};
        self.requirements.work.order_term_visits =
            self.requirements.work.order_term_visits.saturating_add(1);
        match (
            self.program.terms.term_head(previous),
            self.program.terms.term_head(current),
        ) {
            (H::Object { shape: old, .. }, H::Object { shape: new, open }) => {
                let mut fields = self.record_field_scratch.take();
                for ordinal in 0..self.program.terms.object_fields_for_shape(old).len() {
                    let field = self
                        .program
                        .terms
                        .object_field_for_shape(old, ordinal)
                        .unwrap();
                    if let Some(value) = self.program.terms.lookup_object_field(new, field.name) {
                        fields.push((
                            field.name,
                            self.retain_requirement_order_slow(field.ty, value),
                        ));
                    }
                }
                for ordinal in 0..self.program.terms.object_fields_for_shape(new).len() {
                    let field = self
                        .program
                        .terms
                        .object_field_for_shape(new, ordinal)
                        .unwrap();
                    if self
                        .program
                        .terms
                        .lookup_object_field(old, field.name)
                        .is_none()
                    {
                        fields.push((field.name, field.ty));
                    }
                }
                // The rebuilt order already equals the current receipt, so the
                // retained term is the receipt itself.
                let stored_len = self.program.terms.object_fields_for_shape(new).len();
                let already_ordered = stored_len == fields.len()
                    && (0..stored_len).all(|ordinal| {
                        let stored = self
                            .program
                            .terms
                            .object_field_for_shape(new, ordinal)
                            .expect("sealed object field exists");
                        fields[ordinal].0 == stored.name && fields[ordinal].1 == stored.ty
                    });
                if already_ordered {
                    self.record_field_scratch.recycle(fields);
                    return current;
                }
                let result = self.program.terms.object(fields.iter().copied(), open);
                self.record_field_scratch.recycle(fields);
                result
            }
            (H::VariantSet(old), H::VariantSet(new)) => {
                let mut variants = self.variant_scratch.take();
                for ordinal in 0..new.len() {
                    let variant = self.program.terms.variant_terms(new)[ordinal];
                    variants.push(match variant {
                        VariantTerm::Tagged { tag, fields } => {
                            let prior =
                                self.program
                                    .terms
                                    .variant_terms(old)
                                    .iter()
                                    .find_map(|prior| match prior {
                                        VariantTerm::Tagged {
                                            tag: old_tag,
                                            fields,
                                        } if *old_tag == tag => Some(*fields),
                                        _ => None,
                                    });
                            let fields = prior.map_or(fields, |prior| {
                                self.retain_requirement_order_slow(prior, fields)
                            });
                            VariantTerm::Tagged { tag, fields }
                        }
                        tag => tag,
                    });
                }
                let stored = self.program.terms.variant_terms(new);
                if stored.len() == variants.len()
                    && stored
                        .iter()
                        .zip(variants.iter())
                        .all(|(left, right)| left == right)
                {
                    self.variant_scratch.recycle(variants);
                    return current;
                }
                let result = self
                    .program
                    .terms
                    .variant_set_preserving_order(variants.iter().copied());
                self.variant_scratch.recycle(variants);
                result
            }
            (H::List(old), H::List(new)) => {
                let item = self.retain_requirement_order_slow(old, new);
                if item == new {
                    return current;
                }
                self.program.terms.list(item)
            }
            (H::Set(old), H::Set(new)) => {
                let item = self.retain_requirement_order_slow(old, new);
                if item == new {
                    return current;
                }
                self.program.terms.set(item)
            }
            (
                H::Map {
                    key: old_key,
                    value: old_value,
                },
                H::Map {
                    key: current_key,
                    value: current_value,
                },
            ) => {
                let key = self.retain_requirement_order_slow(old_key, current_key);
                let value = self.retain_requirement_order_slow(old_value, current_value);
                if key == current_key && value == current_value {
                    return current;
                }
                self.program.terms.map(key, value)
            }
            (
                H::Function {
                    args: old,
                    result: old_result,
                    ..
                },
                H::Function {
                    args,
                    result,
                    result_mode,
                },
            ) if old.len() == args.len() => {
                let mut arguments = self.term_id_scratch.take();
                let mut changed = false;
                for ordinal in 0..args.len() {
                    let old = self.program.terms.term_ids(old)[ordinal];
                    let new = self.program.terms.term_ids(args)[ordinal];
                    let retained = self.retain_requirement_order_slow(old, new);
                    changed |= retained != new;
                    arguments.push(retained);
                }
                let current_result = result;
                let result = self.retain_requirement_order_slow(old_result, current_result);
                changed |= result != current_result;
                if !changed {
                    self.term_id_scratch.recycle(arguments);
                    return current;
                }
                let function =
                    self.program
                        .terms
                        .function(arguments.iter().copied(), result_mode, result);
                self.term_id_scratch.recycle(arguments);
                function
            }
            _ => current,
        }
    }

    /// Re-derive the affected reverse cone from base facts when external facts
    /// change. Reading yesterday's effective bindings in a cycle would let its
    /// members keep a removed seed alive forever. This is component-local
    /// retraction, not revision caching or a scan of all providers. Clear every
    /// affected derived binding before scheduling any of their reevaluations.
    fn invalidate_requirement_cone(&mut self) {
        let Some(first) = self.requirements.pop_dirty() else {
            return;
        };
        let mut pending = self.variable_scratch.take();
        let mut affected = self.variable_scratch.take();
        pending.push(first);
        let mut popped = 1_u64;
        while let Some(target) = self.requirements.pop_dirty() {
            pending.push(target);
            popped = popped.saturating_add(1);
        }
        self.schedule_generation = super::next_generation(
            &mut self.schedule_generation,
            &mut self.schedule_seen,
            &mut [],
        );
        while let Some(variable) = pending.pop() {
            self.requirements.work.invalidation_variable_visits = self
                .requirements
                .work
                .invalidation_variable_visits
                .saturating_add(1);
            let root = self.root_readonly(variable);
            if self.schedule_seen[root.0 as usize] == self.schedule_generation {
                continue;
            }
            self.schedule_seen[root.0 as usize] = self.schedule_generation;
            if self.cells[root.0 as usize].requirement_base.is_some() {
                affected.push(root);
            }
            let mut member = Some(self.equivalence_head[root.0 as usize]);
            while let Some(variable) = member {
                pending.extend_from_slice(&self.binding_dependents[variable.0 as usize]);
                self.requirements.work.invalidation_edge_visits = self
                    .requirements
                    .work
                    .invalidation_edge_visits
                    .saturating_add(self.binding_dependents[variable.0 as usize].len() as u64);
                member = self.equivalence_next[variable.0 as usize];
            }
        }
        // Stable order is independent of hash maps and alias traversal.
        affected.sort_unstable();
        let mut recorded = 0_u64;
        for target in affected.iter().copied() {
            self.requirements.mark_dirty(target);
            // Keep the cleared binding so the fold can tell an unchanged
            // aggregate from a changed one. Re-scheduling every affected
            // destination unconditionally burned about 85 ms of TodoMVC time
            // in schedule_variable consumer scans that found nothing queued.
            if let Some(previous) = self.cells[target.0 as usize].binding.take() {
                self.requirement_previous_bindings
                    .insert(target.0, previous);
                recorded = recorded.saturating_add(1);
            }
        }
        if let Some(probe) = self.requirement_phase_probe.as_mut() {
            probe.invalidate_dirty_pops = probe.invalidate_dirty_pops.saturating_add(popped);
            probe.invalidate_affected = probe
                .invalidate_affected
                .saturating_add(u64::try_from(affected.len()).unwrap_or(u64::MAX));
            probe.invalidate_recorded = probe.invalidate_recorded.saturating_add(recorded);
        }
        self.variable_scratch.recycle(pending);
        self.variable_scratch.recycle(affected);
    }
}

#[cfg(test)]
mod tests {
    use super::super::ComponentSolver;
    use super::*;
    use crate::ComponentProgramBuilder;

    #[test]
    fn solver_withdrawal_restores_base_and_later_permanent_facts() {
        let mut builder =
            ComponentProgramBuilder::with_terms(crate::TypeTermArena::for_test_symbols([
                "base", "branch", "late",
            ]));
        let target = builder.new_contextual_hole();
        let base_field = builder.terms_mut().intern_name("base");
        let branch_field = builder.terms_mut().intern_name("branch");
        let late_field = builder.terms_mut().intern_name("late");
        let number = builder.terms().number();
        let text = builder.terms().text();
        let base = builder.terms_mut().object([(base_field, number)], true);
        let branch = builder.terms_mut().object([(branch_field, text)], true);
        let late = builder.terms_mut().object([(late_field, text)], true);
        let expected = builder
            .terms_mut()
            .object([(base_field, number), (late_field, text)], true);
        let (mut solver, _) = ComponentSolver::new(builder.finish());
        solver.bind_equal(target, base);
        let owner = solver.requirements.owner();
        let site = solver.register_requirement_site(owner, target);
        solver.requirements.begin(owner);
        solver.requirements.stage(owner, site, branch);
        solver.requirements.commit(owner);
        solver.refresh_requirements();
        assert_ne!(solver.cells[target.0 as usize].binding, Some(base));
        solver.bind_equal(target, late);
        solver.refresh_requirements();
        solver.requirements.begin(owner);
        solver.requirements.commit(owner);
        solver.refresh_requirements();
        assert_eq!(solver.cells[target.0 as usize].binding, Some(expected));
    }

    #[test]
    fn solver_tracks_mutable_payload_without_equating_contributors() {
        let mut builder = ComponentProgramBuilder::new();
        let target = builder.new_contextual_hole();
        let payload = builder.new_authoritative_provider();
        let other_payload = builder.new_contextual_hole();
        let field = builder.terms_mut().intern_name("value");
        let payload_term = builder.variable_term(payload);
        let other_term = builder.variable_term(other_payload);
        let first = builder.terms_mut().object([(field, payload_term)], true);
        let second = builder.terms_mut().object([(field, other_term)], true);
        let number = builder.terms().number();
        let text = builder.terms().text();
        let numbered = builder.terms_mut().object([(field, number)], true);
        let textual = builder.terms_mut().object([(field, text)], true);
        let (mut solver, _) = ComponentSolver::new(builder.finish());
        let owner = solver.requirements.owner();
        let a = solver.register_requirement_site(owner, target);
        let b = solver.register_requirement_site(owner, target);
        solver.requirements.begin(owner);
        solver.requirements.stage(owner, a, first);
        solver.requirements.stage(owner, b, second);
        solver.requirements.commit(owner);
        solver.refresh_requirements();
        assert_ne!(
            solver.root_readonly(payload),
            solver.root_readonly(other_payload)
        );
        solver.replace_binding(payload, number, true);
        solver.refresh_requirements();
        assert_eq!(solver.cells[target.0 as usize].binding, Some(numbered));
        solver.replace_binding(payload, text, true);
        solver.refresh_requirements();
        assert_eq!(solver.cells[target.0 as usize].binding, Some(textual));
        solver.requirements.begin(owner);
        solver.requirements.commit(owner);
        solver.refresh_requirements();
        assert_eq!(solver.cells[target.0 as usize].binding, None);
        assert!(solver.binding_dependencies[target.0 as usize].is_empty());
        solver.replace_binding(payload, number, true);
        assert_eq!(
            solver.requirements.pop_dirty(),
            None,
            "withdrawn dependency must be retired"
        );
    }

    #[test]
    fn solver_aliases_preserve_contributions_without_capturing_them_as_base() {
        for reverse in [false, true] {
            let mut builder = ComponentProgramBuilder::new();
            let a = builder.new_contextual_hole();
            let b = builder.new_contextual_hole();
            let a_field = builder.terms_mut().intern_name("a");
            let b_field = builder.terms_mut().intern_name("b");
            let number = builder.terms().number();
            let first = builder.terms_mut().object([(a_field, number)], true);
            let second = builder.terms_mut().object([(b_field, number)], true);
            let both = builder
                .terms_mut()
                .object([(a_field, number), (b_field, number)], true);
            let (mut solver, _) = ComponentSolver::new(builder.finish());
            let owner_a = solver.requirements.owner();
            let owner_b = solver.requirements.owner();
            let site_a = solver.register_requirement_site(owner_a, a);
            let site_b = solver.register_requirement_site(owner_b, b);
            for (owner, site, term) in [(owner_a, site_a, first), (owner_b, site_b, second)] {
                solver.requirements.begin(owner);
                solver.requirements.stage(owner, site, term);
                solver.requirements.commit(owner);
            }
            solver.refresh_requirements();
            let (left, right) = if reverse { (b, a) } else { (a, b) };
            solver.union_variables(left, right);
            solver.refresh_requirements();
            let root = solver.root_readonly(a);
            assert_eq!(solver.cells[root.0 as usize].binding, Some(both));
            solver.requirements.begin(owner_a);
            solver.requirements.commit(owner_a);
            solver.refresh_requirements();
            assert_eq!(solver.cells[root.0 as usize].binding, Some(second));
            solver.requirements.begin(owner_b);
            solver.requirements.commit(owner_b);
            solver.refresh_requirements();
            assert_eq!(solver.cells[root.0 as usize].binding, None);
        }
    }

    #[test]
    fn solver_incompatible_facts_replay_without_rebuilding_previous_aggregate() {
        let mut builder = ComponentProgramBuilder::new();
        let target = builder.new_contextual_hole();
        let field = builder.terms_mut().intern_name("value");
        let number = builder.terms().number();
        let concrete = builder.terms_mut().object([(field, number)], false);
        let tag = builder.terms_mut().variant_tag("Header");
        let domain = builder.terms_mut().variant_set([tag]);
        let (mut solver, _) = ComponentSolver::new(builder.finish());
        let owner = solver.requirements.owner();
        let value_site = solver.register_requirement_site(owner, target);
        let domain_site = solver.register_requirement_site(owner, target);
        let mut mutations = None;
        for _ in 0..3 {
            solver.requirements.begin(owner);
            solver.requirements.stage(owner, value_site, concrete);
            solver.requirements.stage(owner, domain_site, domain);
            solver.requirements.commit(owner);
            solver.refresh_requirements();
            if let Some(mutations) = mutations {
                assert_eq!(solver.work.mutations, mutations);
            } else {
                mutations = Some(solver.work.mutations);
            }
        }
    }

    #[test]
    fn withdrawing_cycle_seed_removes_self_supported_evidence() {
        let mut builder = ComponentProgramBuilder::new();
        let a = builder.new_contextual_hole();
        let b = builder.new_contextual_hole();
        let a_term = builder.variable_term(a);
        let b_term = builder.variable_term(b);
        let number = builder.terms().number();
        let (mut solver, _) = ComponentSolver::new(builder.finish());
        let a_owner = solver.requirements.owner();
        let b_owner = solver.requirements.owner();
        let seed_owner = solver.requirements.owner();
        let a_site = solver.register_requirement_site(a_owner, a);
        let b_site = solver.register_requirement_site(b_owner, b);
        let seed_site = solver.register_requirement_site(seed_owner, a);
        // Establish the seed first so both effective bindings become closed.
        solver.requirements.begin(seed_owner);
        solver.requirements.stage(seed_owner, seed_site, number);
        solver.requirements.commit(seed_owner);
        solver.refresh_requirements();
        for (owner, site, term) in [(b_owner, b_site, a_term), (a_owner, a_site, b_term)] {
            solver.requirements.begin(owner);
            solver.requirements.stage(owner, site, term);
            solver.requirements.commit(owner);
            solver.refresh_requirements();
        }
        assert_eq!(solver.resolve_term(a_term), number);
        assert_eq!(solver.resolve_term(b_term), number);
        solver.requirements.begin(seed_owner);
        solver.requirements.commit(seed_owner);
        solver.refresh_requirements();
        assert_ne!(
            solver.resolve_term(a_term),
            number,
            "a cycle cannot retain a withdrawn external seed"
        );
        assert_ne!(solver.resolve_term(b_term), number);
    }

    #[test]
    fn projection_does_not_reimport_a_withdrawn_requirement_as_base() {
        let mut builder = ComponentProgramBuilder::new();
        let target = builder.new_contextual_hole();
        let consumer = builder.new_variable();
        let field = builder.terms_mut().intern_name("value");
        let number = builder.terms().number();
        let branch = builder.terms_mut().object([(field, number)], true);
        let (mut solver, _) = ComponentSolver::new(builder.finish());
        let owner = solver.requirements.owner();
        let site = solver.register_requirement_site(owner, target);
        solver.requirements.begin(owner);
        solver.requirements.stage(owner, site, branch);
        solver.requirements.commit(owner);
        solver.refresh_requirements();
        solver.project(target, Some(field), consumer);
        solver.requirements.begin(owner);
        solver.requirements.commit(owner);
        solver.refresh_requirements();
        solver.project(target, Some(field), consumer);
        solver.refresh_requirements();
        assert_eq!(
            solver.cells[target.0 as usize].requirement_base,
            Some(None),
            "a projection must not turn withdrawn evidence into a permanent scaffold"
        );
        let consumer_term = solver.program.terms.variable(consumer);
        assert_ne!(
            solver.resolve_term(consumer_term),
            number,
            "the projection consumer must also lose the withdrawn value"
        );
        let text = solver.program.terms.text();
        solver.bind_equal(consumer, text);
        solver.project(target, Some(field), consumer);
        let target_term = solver.program.terms.variable(target);
        let resolved = solver.resolve_term(target_term);
        assert_eq!(
            solver.project_field(resolved, field),
            Some(text),
            "independent consumer requirements must still flow back to the source"
        );
    }

    #[test]
    fn ordering_receipts_cannot_resurrect_removed_fields_or_old_types() {
        let mut builder =
            ComponentProgramBuilder::with_terms(crate::TypeTermArena::for_test_symbols([
                "a", "b", "removed", "added",
            ]));
        let a = builder.terms().intern_name("a");
        let b = builder.terms().intern_name("b");
        let removed = builder.terms().intern_name("removed");
        let added = builder.terms().intern_name("added");
        let number = builder.terms().number();
        let text = builder.terms().text();
        let prior = builder
            .terms_mut()
            .object([(a, number), (b, number), (removed, number)], true);
        let current = builder
            .terms_mut()
            .object([(b, text), (a, text), (added, number)], false);
        let expected = builder
            .terms_mut()
            .object([(a, text), (b, text), (added, number)], false);
        let (mut solver, _) = ComponentSolver::new(builder.finish());
        assert_eq!(solver.retain_requirement_order(prior, current), expected);
        let prior = solver.program.terms.list(prior);
        let current = solver.program.terms.list(current);
        let expected = solver.program.terms.list(expected);
        assert_eq!(solver.retain_requirement_order(prior, current), expected);
    }

    #[test]
    fn work_counts_replay_visits_even_when_no_facts_change() {
        let mut store = RequirementContributions::default();
        let owner = store.owner();
        let site = store.site(owner, TypeVariableId(0));
        for value in [Some(TypeTermId(1)), Some(TypeTermId(1)), None, None] {
            store.begin(owner);
            if let Some(value) = value {
                store.stage(owner, site, value);
            }
            store.commit(owner);
        }
        assert_eq!(
            store.work,
            crate::KernelRequirementWork {
                owners: 1,
                sites: 1,
                activations: 4,
                begin_site_visits: 4,
                staged_writes: 2,
                commit_site_visits: 4,
                changed_sites: 2,
                withdrawn_sites: 1,
                ..crate::KernelRequirementWork::default()
            }
        );
    }

    #[test]
    fn replacement_withdraws_only_its_own_occurrence() {
        let mut store = RequirementContributions::default();
        let first = store.owner();
        let second = store.owner();
        let target = TypeVariableId(0);
        let a = store.site(first, target);
        let b = store.site(second, target);
        for (owner, site, term) in [(first, a, TypeTermId(1)), (second, b, TypeTermId(2))] {
            store.begin(owner);
            store.stage(owner, site, term);
            store.commit(owner);
        }
        assert_eq!(
            store.values(target).collect::<Vec<_>>(),
            [TypeTermId(1), TypeTermId(2)]
        );
        assert_eq!(store.pop_dirty(), Some(target));
        assert_eq!(store.pop_dirty(), None);
        store.begin(first);
        store.commit(first);
        assert_eq!(store.values(target).collect::<Vec<_>>(), [TypeTermId(2)]);
        assert_eq!(store.pop_dirty(), Some(target));
    }

    #[test]
    fn branch_roundtrip_matches_fresh_state_without_new_sites() {
        let mut store = RequirementContributions::default();
        let owner = store.owner();
        let target = TypeVariableId(3);
        let a = store.site(owner, target);
        let b = store.site(owner, target);
        for site in [a, b, a] {
            store.begin(owner);
            store.stage(owner, site, TypeTermId(site.0 + 10));
            store.commit(owner);
            assert_eq!(
                store.values(target).collect::<Vec<_>>(),
                [TypeTermId(site.0 + 10)]
            );
            assert_eq!(store.pop_dirty(), Some(target));
        }
        assert_eq!(store.sites.len(), 2);
        store.begin(owner);
        store.stage(owner, a, TypeTermId(10));
        store.commit(owner);
        assert_eq!(
            store.pop_dirty(),
            None,
            "unchanged replay must be quiescent"
        );
    }

    #[test]
    fn staged_or_failed_evaluation_does_not_publish_partial_effects() {
        let mut store = RequirementContributions::default();
        let owner = store.owner();
        let target = TypeVariableId(0);
        let site = store.site(owner, target);
        store.begin(owner);
        store.stage(owner, site, TypeTermId(1));
        assert_eq!(store.values(target).count(), 0);
        store.commit(owner);
        assert_eq!(store.pop_dirty(), Some(target));
        store.begin(owner);
        store.stage(owner, site, TypeTermId(2));
        assert_eq!(store.values(target).collect::<Vec<_>>(), [TypeTermId(1)]);
        store.abort(owner);
        assert_eq!(store.values(target).collect::<Vec<_>>(), [TypeTermId(1)]);
        assert_eq!(store.pop_dirty(), None);
        store.begin(owner);
        store.commit(owner);
        assert_eq!(store.values(target).count(), 0);
    }

    #[test]
    fn skipped_nested_invocation_withdraws_all_its_sites() {
        let mut store = RequirementContributions::default();
        let activation = store.owner();
        let target = TypeVariableId(0);
        let parent = store.site(activation, target);
        let child_first = store.site(activation, target);
        let child_second = store.site(activation, target);
        store.begin(activation);
        for site in [parent, child_first, child_second] {
            store.stage(activation, site, TypeTermId(site.0 + 10));
        }
        store.commit(activation);
        assert_eq!(store.values(target).count(), 3);
        store.begin(activation);
        store.stage(activation, parent, TypeTermId(10));
        store.commit(activation);
        assert_eq!(store.values(target).collect::<Vec<_>>(), [TypeTermId(10)]);
    }

    #[test]
    fn parent_abort_does_not_publish_nested_invocation_effects() {
        let mut store = RequirementContributions::default();
        let activation = store.owner();
        let target = TypeVariableId(0);
        let parent = store.site(activation, target);
        let child = store.site(activation, target);
        store.begin(activation);
        store.stage(activation, parent, TypeTermId(1));
        store.commit(activation);
        store.pop_dirty();
        store.begin(activation);
        store.stage(activation, parent, TypeTermId(2));
        store.stage(activation, child, TypeTermId(3));
        store.abort(activation);
        assert_eq!(store.values(target).collect::<Vec<_>>(), [TypeTermId(1)]);
        assert_eq!(store.pop_dirty(), None);
    }

    #[test]
    fn alias_consumers_can_merge_facts_independent_of_member_order() {
        let mut store = RequirementContributions::default();
        let activation = store.owner();
        let a = TypeVariableId(0);
        let b = TypeVariableId(1);
        let first = store.site(activation, b);
        let second = store.site(activation, a);
        store.begin(activation);
        store.stage(activation, first, TypeTermId(1));
        store.stage(activation, second, TypeTermId(2));
        store.commit(activation);
        for members in [[a, b], [b, a]] {
            let mut facts = members
                .into_iter()
                .flat_map(|target| store.facts(target))
                .collect::<Vec<_>>();
            facts.sort_unstable_by_key(|(site, _)| *site);
            assert_eq!(facts, [(first, TypeTermId(1)), (second, TypeTermId(2))]);
        }
    }

    #[test]
    fn contribution_order_is_site_order_not_branch_visit_order() {
        let mut store = RequirementContributions::default();
        let owner = store.owner();
        let target = TypeVariableId(0);
        let a = store.site(owner, target);
        let b = store.site(owner, target);
        store.begin(owner);
        store.stage(owner, b, TypeTermId(2));
        store.stage(owner, a, TypeTermId(1));
        store.commit(owner);
        assert_eq!(
            store.values(target).collect::<Vec<_>>(),
            [TypeTermId(1), TypeTermId(2)]
        );
    }
}
