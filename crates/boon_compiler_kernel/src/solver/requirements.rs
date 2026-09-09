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
        let owner = &mut self.owners[owner.0 as usize];
        assert!(
            !owner.evaluating,
            "nested effects share their enclosing activation transaction"
        );
        owner.evaluating = true;
        owner.sealed = true;
        for site in &owner.sites {
            self.sites[site.0 as usize].staged = None;
        }
    }

    pub(super) fn stage(
        &mut self,
        owner: RequirementOwnerId,
        site: RequirementSiteId,
        value: TypeTermId,
    ) {
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
            let site = &mut self.sites[id.0 as usize];
            if site.value == site.staged {
                continue;
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
}

impl super::ComponentSolver {
    pub(super) fn project_requirement(
        &mut self,
        provider: TypeVariableId,
        field: Option<boon_contract::SymbolId>,
        pattern: Option<(crate::PackedKernelPattern, &[boon_contract::SymbolId])>,
        consumer: TypeVariableId,
    ) {
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

    /// Refresh derived bindings before dequeuing another operation. Raw input
    /// terms remain the dependency authority even when their current resolved
    /// aggregate is closed, unchanged, or has widened away a payload.
    pub(super) fn refresh_requirements(&mut self) {
        self.invalidate_requirement_cone();
        while let Some(target) = self.requirements.pop_dirty() {
            let target = self.root(target);
            let Some(base) = self.cells[target.0 as usize].requirement_base else {
                continue;
            };
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
            self.replace_binding_dependencies_from(target, &inputs);
            let mut aggregate = None;
            for term in inputs.iter().copied() {
                // Match the existing recursive-shape guard without equating
                // any contributor variable to its destination.
                if self.occurs(target, term) {
                    continue;
                }
                let term = self.resolve_term(term);
                aggregate = Some(match aggregate {
                    None => term,
                    Some(previous) => self.merge_type_evidence(previous, term, false),
                });
            }
            self.term_id_scratch.recycle(inputs);
            if let (Some(previous), Some(current)) = (
                self.requirements.targets[target.0 as usize].order,
                aggregate,
            ) {
                aggregate = Some(self.retain_requirement_order(previous, current));
            }
            self.requirements.set_order(target, aggregate);
            if self.cells[target.0 as usize].binding != aggregate {
                self.cells[target.0 as usize].binding = aggregate;
                self.touch(target);
            }
        }
    }

    /// Preserve only the order of surviving fields. Every value, shape kind,
    /// openness bit and variable identity comes from the newly derived term.
    /// Removed fields are never reintroduced through an ordering receipt.
    fn retain_requirement_order(
        &mut self,
        previous: TypeTermId,
        current: TypeTermId,
    ) -> TypeTermId {
        use crate::{TypeTermHead as H, VariantTerm};
        if previous == current {
            return current;
        }
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
                        fields.push((field.name, self.retain_requirement_order(field.ty, value)));
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
                                self.retain_requirement_order(prior, fields)
                            });
                            VariantTerm::Tagged { tag, fields }
                        }
                        tag => tag,
                    });
                }
                let result = self
                    .program
                    .terms
                    .variant_set_preserving_order(variants.iter().copied());
                self.variant_scratch.recycle(variants);
                result
            }
            (H::List(old), H::List(new)) => {
                let item = self.retain_requirement_order(old, new);
                self.program.terms.list(item)
            }
            (H::Set(old), H::Set(new)) => {
                let item = self.retain_requirement_order(old, new);
                self.program.terms.set(item)
            }
            (
                H::Map {
                    key: old_key,
                    value: old_value,
                },
                H::Map { key, value },
            ) => {
                let key = self.retain_requirement_order(old_key, key);
                let value = self.retain_requirement_order(old_value, value);
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
                for ordinal in 0..args.len() {
                    let old = self.program.terms.term_ids(old)[ordinal];
                    let new = self.program.terms.term_ids(args)[ordinal];
                    arguments.push(self.retain_requirement_order(old, new));
                }
                let result = self.retain_requirement_order(old_result, result);
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
        while let Some(target) = self.requirements.pop_dirty() {
            pending.push(target);
        }
        self.schedule_generation = super::next_generation(
            &mut self.schedule_generation,
            &mut self.schedule_seen,
            &mut [],
        );
        while let Some(variable) = pending.pop() {
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
                member = self.equivalence_next[variable.0 as usize];
            }
        }
        // Stable order is independent of hash maps and alias traversal.
        affected.sort_unstable();
        for target in affected.iter().copied() {
            self.requirements.mark_dirty(target);
            if self.cells[target.0 as usize].binding.take().is_some() {
                pending.push(target);
            }
        }
        for target in pending.iter().copied() {
            self.touch(target);
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
