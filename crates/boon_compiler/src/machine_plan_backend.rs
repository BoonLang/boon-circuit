#![allow(clippy::too_many_arguments)]

use boon_ir::{
    self as ir, DerivedValueKind, ErasedProgram, InitialValue, ListInitializer, ListMutationKind,
    ListProjectionKind,
};
use boon_plan::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

fn plan_source_id(value: ir::SourceId) -> SourceId {
    SourceId(value.0)
}

fn plan_state_id(value: ir::StateId) -> StateId {
    StateId(value.0)
}

fn plan_activation_id(value: ir::ActivationId) -> PlanActivationId {
    PlanActivationId(value.0)
}

fn plan_pulse_batch_id(value: ir::PulseBatchId) -> PlanPulseBatchId {
    PlanPulseBatchId(value.0)
}

fn plan_list_id(value: ir::ListId) -> ListId {
    ListId(value.0)
}

fn plan_field_id(value: ir::FieldId) -> FieldId {
    FieldId(value.0)
}

fn plan_scope_id(value: Option<ir::ScopeId>) -> Option<ScopeId> {
    value.map(|value| ScopeId(value.0))
}

fn visit_list_access_expression_ids_mut(
    access: &mut PlanListAccess,
    visitor: &mut impl FnMut(&mut PlanRowExpressionId),
) {
    for key in &mut access.semantic_order {
        visitor(&mut key.expression);
    }
    if let Some(guard) = &mut access.guard {
        visitor(guard);
    }
    for filter in &mut access.filters {
        visitor(&mut filter.predicate);
    }
    for map in &mut access.maps {
        visitor(&mut map.body);
        for capture in &mut map.captures {
            visitor(&mut capture.value);
        }
    }
    access.selection.visit_expressions_mut(visitor);
    visitor(&mut access.limit);
}

fn visit_row_node_children_mut(
    node: &mut PlanRowExpressionNode,
    visitor: &mut impl FnMut(&mut PlanRowExpressionId),
) {
    match node {
        PlanRowExpressionNode::Absent
        | PlanRowExpressionNode::Intrinsic { .. }
        | PlanRowExpressionNode::EffectResult
        | PlanRowExpressionNode::TransientEffectResult { .. }
        | PlanRowExpressionNode::Field { .. }
        | PlanRowExpressionNode::Constant { .. }
        | PlanRowExpressionNode::ListRef { .. }
        | PlanRowExpressionNode::AuthorityListRef { .. }
        | PlanRowExpressionNode::Local { .. }
        | PlanRowExpressionNode::LocalRow { .. }
        | PlanRowExpressionNode::EventRow { .. } => {}
        PlanRowExpressionNode::Flush { payload: input }
        | PlanRowExpressionNode::FlushBoundary { input }
        | PlanRowExpressionNode::TextTrim { input }
        | PlanRowExpressionNode::TextIsEmpty { input }
        | PlanRowExpressionNode::TextLength { input }
        | PlanRowExpressionNode::BytesToHex { input }
        | PlanRowExpressionNode::BytesToBase64 { input }
        | PlanRowExpressionNode::BytesFromHex { input }
        | PlanRowExpressionNode::BytesFromBase64 { input }
        | PlanRowExpressionNode::BytesIsEmpty { input }
        | PlanRowExpressionNode::BytesLength { input }
        | PlanRowExpressionNode::ListSum { input }
        | PlanRowExpressionNode::ObjectField { object: input, .. }
        | PlanRowExpressionNode::ListRowField { row: input, .. } => visitor(input),
        PlanRowExpressionNode::CatchCycle { input, on_cycle } => {
            visitor(input);
            visitor(on_cycle);
        }
        PlanRowExpressionNode::TextToNumber { input, radix } => {
            visitor(input);
            if let Some(radix) = radix {
                visitor(radix);
            }
        }
        PlanRowExpressionNode::TextStartsWith { input, prefix }
        | PlanRowExpressionNode::BytesStartsWith { input, prefix }
        | PlanRowExpressionNode::BytesConcat {
            left: input,
            right: prefix,
        }
        | PlanRowExpressionNode::BytesEqual {
            left: input,
            right: prefix,
        }
        | PlanRowExpressionNode::NumberInfix {
            left: input,
            right: prefix,
            ..
        } => {
            visitor(input);
            visitor(prefix);
        }
        PlanRowExpressionNode::BytesEndsWith { input, suffix }
        | PlanRowExpressionNode::BytesFind {
            input,
            needle: suffix,
        }
        | PlanRowExpressionNode::BytesGet {
            input,
            position: suffix,
        }
        | PlanRowExpressionNode::BytesTake {
            input,
            count: suffix,
        }
        | PlanRowExpressionNode::BytesDrop {
            input,
            count: suffix,
        } => {
            visitor(input);
            visitor(suffix);
        }
        PlanRowExpressionNode::TextSlice {
            input,
            from: start,
            count: length,
        }
        | PlanRowExpressionNode::BytesSlice {
            input,
            from: start,
            count: length,
        }
        | PlanRowExpressionNode::BytesSet {
            input,
            position: start,
            value: length,
        } => {
            visitor(input);
            visitor(start);
            visitor(length);
        }
        PlanRowExpressionNode::TextToBytes { input, encoding }
        | PlanRowExpressionNode::BytesToText { input, encoding } => {
            visitor(input);
            if let Some(encoding) = encoding {
                visitor(encoding);
            }
        }
        PlanRowExpressionNode::BytesZeros { count } => visitor(count),
        PlanRowExpressionNode::BytesReadUnsigned {
            input,
            from: offset,
            byte_count,
            endian,
        }
        | PlanRowExpressionNode::BytesReadSigned {
            input,
            from: offset,
            byte_count,
            endian,
        } => {
            visitor(input);
            visitor(offset);
            visitor(byte_count);
            visitor(endian);
        }
        PlanRowExpressionNode::BytesWriteUnsigned {
            input,
            from: offset,
            byte_count,
            endian,
            value,
        }
        | PlanRowExpressionNode::BytesWriteSigned {
            input,
            from: offset,
            byte_count,
            endian,
            value,
        } => {
            visitor(input);
            visitor(offset);
            visitor(byte_count);
            visitor(endian);
            visitor(value);
        }
        PlanRowExpressionNode::TextConcat { parts }
        | PlanRowExpressionNode::ListLiteral { items: parts } => {
            parts.iter_mut().for_each(visitor);
        }
        PlanRowExpressionNode::ListGetField { index, .. } => visitor(index),
        PlanRowExpressionNode::ListRange { from, to } => {
            visitor(from);
            visitor(to);
        }
        PlanRowExpressionNode::ContextualCollection {
            source,
            body,
            captures,
            indexed_access,
            ..
        } => {
            visitor(source);
            if let Some(indexed_access) = indexed_access {
                indexed_access.selection.visit_expressions_mut(visitor);
            }
            visitor(body);
            for capture in captures {
                visitor(&mut capture.value);
            }
        }
        PlanRowExpressionNode::ContextualOrder {
            source,
            key,
            direction,
            ..
        } => {
            visitor(source);
            visitor(key);
            visitor(direction);
        }
        PlanRowExpressionNode::ListAccess { access } => {
            visit_list_access_expression_ids_mut(access, visitor);
        }
        PlanRowExpressionNode::ListPage { page } => {
            visit_list_access_expression_ids_mut(&mut page.access, visitor);
            if let Some(view_limit) = &mut page.view_limit {
                visitor(view_limit);
            }
            visitor(&mut page.after);
        }
        PlanRowExpressionNode::BoundedListPage { page } => {
            visitor(&mut page.view);
            visitor(&mut page.size);
            visitor(&mut page.after);
        }
        PlanRowExpressionNode::Object { fields }
        | PlanRowExpressionNode::TaggedObject { fields, .. } => {
            for field in fields {
                visitor(&mut field.value);
            }
        }
        PlanRowExpressionNode::MapLiteral { entries, .. } => {
            for entry in entries {
                visitor(&mut entry.key);
                visitor(&mut entry.value);
            }
        }
        PlanRowExpressionNode::SetLiteral { items, .. } => {
            for item in items {
                visitor(item);
            }
        }
        PlanRowExpressionNode::TransientCollection { region } => {
            region.visit_children_mut(visitor);
        }
        PlanRowExpressionNode::BuiltinCall { input, args, .. } => {
            if let Some(input) = input {
                visitor(input);
            }
            for argument in args {
                visitor(&mut argument.value);
            }
        }
        PlanRowExpressionNode::Select { input, arms } => {
            visitor(input);
            for arm in arms {
                visitor(&mut arm.value);
            }
        }
    }
}

fn rewrite_row_expression(
    arena: &mut PlanRowExpressionArena,
    root: PlanRowExpressionId,
    mut rewrite: impl FnMut(
        &PlanRowExpressionArena,
        PlanRowExpressionId,
        &mut PlanRowExpressionNode,
    ) -> Result<(), PlanError>,
) -> Result<PlanRowExpressionId, PlanError> {
    let order = arena.walk_postorder(root)?;
    let mut rewritten = BTreeMap::new();
    for original in order {
        let mut node = arena.node(original)?.clone();
        let mut missing_child = None;
        visit_row_node_children_mut(&mut node, &mut |child| {
            if let Some(replacement) = rewritten.get(child).copied() {
                *child = replacement;
            } else {
                missing_child = Some(*child);
            }
        });
        if let Some(child) = missing_child {
            return Err(PlanError::new(format!(
                "persistent row rewrite reached parent {} before child {}",
                original.0, child.0
            )));
        }
        rewrite(arena, original, &mut node)?;
        rewritten.insert(original, arena.intern(node)?);
    }
    rewritten.get(&root).copied().ok_or_else(|| {
        PlanError::new(format!(
            "persistent row rewrite did not produce root {}",
            root.0
        ))
    })
}

fn visit_row_node_contextual_identities(
    node: &PlanRowExpressionNode,
    visitor: &mut impl FnMut(PlanStaticOwnerId, PlanLocalId),
) {
    match node {
        PlanRowExpressionNode::Local { owner, local, .. }
        | PlanRowExpressionNode::LocalRow { owner, local }
        | PlanRowExpressionNode::ContextualCollection {
            owner,
            row_local: local,
            ..
        }
        | PlanRowExpressionNode::ContextualOrder {
            owner,
            row_local: local,
            ..
        } => visitor(*owner, *local),
        PlanRowExpressionNode::ListAccess { access } => {
            visit_list_access_contextual_identities(access, visitor);
        }
        PlanRowExpressionNode::ListPage { page } => {
            visit_list_access_contextual_identities(&page.access, visitor);
        }
        _ => {}
    }
}

fn visit_list_access_contextual_identities(
    access: &PlanListAccess,
    visitor: &mut impl FnMut(PlanStaticOwnerId, PlanLocalId),
) {
    for key in &access.semantic_order {
        visitor(key.owner, key.row_local);
    }
    for filter in &access.filters {
        visitor(filter.owner, filter.row_local);
    }
    for map in &access.maps {
        visitor(map.owner, map.row_local);
    }
}

fn row_expression_contains_contextual_identity(
    arena: &PlanRowExpressionArena,
    root: PlanRowExpressionId,
    identity: (PlanStaticOwnerId, PlanLocalId),
) -> Result<bool, PlanError> {
    let mut found = false;
    arena.visit(root, &mut |_, node| {
        visit_row_node_contextual_identities(node, &mut |owner, local| {
            found |= (owner, local) == identity;
        });
    })?;
    Ok(found)
}

fn remap_row_expression_contextual_local(
    arena: &mut PlanRowExpressionArena,
    root: PlanRowExpressionId,
    from: (PlanStaticOwnerId, PlanLocalId),
    to: (PlanStaticOwnerId, PlanLocalId),
) -> Result<Option<PlanRowExpressionId>, PlanError> {
    if from == to {
        return Ok(Some(root));
    }
    if row_expression_contains_contextual_identity(arena, root, to)? {
        return Ok(None);
    }
    rewrite_row_expression(arena, root, |_, _, node| {
        let remap = |owner: &mut PlanStaticOwnerId, local: &mut PlanLocalId| {
            if (*owner, *local) == from {
                (*owner, *local) = to;
            }
        };
        match node {
            PlanRowExpressionNode::Local { owner, local, .. }
            | PlanRowExpressionNode::LocalRow { owner, local }
            | PlanRowExpressionNode::ContextualCollection {
                owner,
                row_local: local,
                ..
            }
            | PlanRowExpressionNode::ContextualOrder {
                owner,
                row_local: local,
                ..
            } => remap(owner, local),
            PlanRowExpressionNode::ListAccess { access } => {
                remap_list_access_contextual_local(access, from, to);
            }
            PlanRowExpressionNode::ListPage { page } => {
                remap_list_access_contextual_local(&mut page.access, from, to);
            }
            _ => {}
        }
        Ok(())
    })
    .map(Some)
}

fn remap_list_access_contextual_local(
    access: &mut PlanListAccess,
    from: (PlanStaticOwnerId, PlanLocalId),
    to: (PlanStaticOwnerId, PlanLocalId),
) {
    let remap = |owner: &mut PlanStaticOwnerId, local: &mut PlanLocalId| {
        if (*owner, *local) == from {
            (*owner, *local) = to;
        }
    };
    for key in &mut access.semantic_order {
        remap(&mut key.owner, &mut key.row_local);
    }
    for filter in &mut access.filters {
        remap(&mut filter.owner, &mut filter.row_local);
    }
    for map in &mut access.maps {
        remap(&mut map.owner, &mut map.row_local);
    }
}

fn erased_owner_ancestry(
    program: &ErasedProgram,
    owner: Option<ir::StaticOwnerId>,
) -> Result<Vec<ir::StaticOwnerId>, PlanError> {
    let mut ancestry = Vec::new();
    let mut next = owner;
    while let Some(owner) = next {
        let definition = program
            .scope_index
            .owners
            .iter()
            .find(|definition| definition.id == owner)
            .ok_or_else(|| PlanError::new(format!("missing static owner {owner}")))?;
        ancestry.push(owner);
        next = definition.parent;
    }
    ancestry.reverse();
    Ok(ancestry)
}

fn plan_owner_from_parts(
    program: &ErasedProgram,
    static_owner: Option<ir::StaticOwnerId>,
    owner_ancestry: &[ir::StaticOwnerId],
    diagnostic: &str,
) -> Result<PlanOwner, PlanError> {
    if owner_ancestry.last().copied() != static_owner {
        return Err(PlanError::new(format!(
            "{diagnostic} owner ancestry {owner_ancestry:?} does not end at {static_owner:?}"
        )));
    }
    let mut ancestors = Vec::with_capacity(owner_ancestry.len());
    for owner in owner_ancestry {
        let definition = program
            .scope_index
            .owners
            .iter()
            .find(|definition| definition.id == *owner)
            .ok_or_else(|| PlanError::new(format!("{diagnostic} has missing owner {owner}")))?;
        if let Some(row) = definition.authority_row {
            ancestors.push(PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(owner.as_usize()),
                scope: ScopeId(row.scope.0),
                list: plan_list_id(row.list),
            });
        }
    }
    Ok(PlanOwner {
        static_owner: static_owner
            .map(|owner| PlanStaticOwnerId(owner.as_usize()))
            .unwrap_or(PlanStaticOwnerId::ROOT),
        ancestors,
    })
}

pub(crate) fn plan_owner_for_static_owner(
    program: &ErasedProgram,
    owner: Option<ir::StaticOwnerId>,
    diagnostic: &str,
) -> Result<PlanOwner, PlanError> {
    let ancestry = erased_owner_ancestry(program, owner)?;
    plan_owner_from_parts(program, owner, &ancestry, diagnostic)
}

fn static_owner_descends_from(
    program: &ErasedProgram,
    candidate: ir::StaticOwnerId,
    ancestor: ir::StaticOwnerId,
) -> Result<bool, PlanError> {
    program
        .scope_index
        .owner_descends_from(candidate, ancestor)
        .map_err(PlanError::new)
}

pub(crate) fn producer_function_ownership_seed(
    program: &ErasedProgram,
    owner: ir::StaticOwnerId,
) -> Result<ProducerFunctionOwnershipPlan, PlanError> {
    let mut owned_ir = BTreeSet::new();
    for definition in &program.scope_index.owners {
        if static_owner_descends_from(program, definition.id, owner)? {
            owned_ir.insert(definition.id);
        }
    }
    if !owned_ir.contains(&owner) {
        return Err(PlanError::new(format!(
            "producer function owner {owner} is absent from erased ownership"
        )));
    }

    let static_owners = owned_ir
        .iter()
        .map(|owner| PlanStaticOwnerId(owner.as_usize()))
        .collect::<Vec<_>>();
    if static_owners.first().copied() != Some(PlanStaticOwnerId(owner.as_usize())) {
        return Err(PlanError::new(format!(
            "producer function owner {owner} is not the first canonical owner in its subtree"
        )));
    }

    let sources = program
        .sources
        .iter()
        .filter(|source| {
            source
                .static_owner
                .is_some_and(|owner| owned_ir.contains(&owner))
        })
        .map(|source| plan_source_id(source.id))
        .collect::<Vec<_>>();
    let states = program
        .state_cells
        .iter()
        .filter(|state| {
            state
                .static_owner
                .is_some_and(|owner| owned_ir.contains(&owner))
        })
        .map(|state| plan_state_id(state.id))
        .collect::<Vec<_>>();
    let fields = program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            !field.resource_only
                && field
                    .static_owner
                    .is_some_and(|owner| owned_ir.contains(&owner))
        })
        .map(|field| plan_field_id(field.id))
        .collect::<Vec<_>>();
    let owned_fields = fields.iter().copied().collect::<BTreeSet<_>>();

    let mut lists = BTreeSet::new();
    for derived in &program.derived_values {
        if owned_fields.contains(&plan_field_id(derived.id))
            && let Some(list) = derived.materialized_list_id
        {
            lists.insert(plan_list_id(list));
        }
    }
    for materialization in &program.materializations {
        if owned_ir.contains(&materialization.owner)
            && let Some(list) = materialization.target_list_id
        {
            lists.insert(plan_list_id(list));
        }
    }
    for definition in program
        .scope_index
        .owners
        .iter()
        .filter(|definition| owned_ir.contains(&definition.id))
    {
        if let Some(row) = definition.target_row.or(definition.authority_row) {
            lists.insert(plan_list_id(row.list));
        }
    }

    Ok(ProducerFunctionOwnershipPlan::new(
        static_owners,
        sources,
        states,
        fields,
        lists.into_iter().collect(),
        Vec::new(),
        Vec::new(),
    ))
}

fn complete_producer_function_ownership(
    program: &ErasedProgram,
    instances: &[ProducerFunctionInstancePlan],
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    indexes: &[PlanListIndex],
    regions: &[OperationRegion],
) -> Result<Vec<ProducerFunctionInstancePlan>, PlanError> {
    let realized_fields = scalar_slots
        .iter()
        .filter_map(|slot| slot.indexed_field_id)
        .chain(list_slots.iter().flat_map(ListStorageSlot::row_field_ids))
        .chain(
            regions
                .iter()
                .flat_map(|region| &region.ops)
                .filter_map(|op| match op.output {
                    Some(ValueRef::Field(field)) => Some(field),
                    _ => None,
                }),
        )
        .collect::<BTreeSet<_>>();
    let mut completed = Vec::with_capacity(instances.len());
    for instance in instances {
        let owner = ir::StaticOwnerId(instance.owner.static_owner.0);
        let mut ownership = producer_function_ownership_seed(program, owner)?;
        ownership
            .fields
            .retain(|field| realized_fields.contains(field));
        let owned_static_owners = ownership
            .static_owners
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let owned_lists = ownership.lists.iter().copied().collect::<BTreeSet<_>>();
        ownership.indexes = indexes
            .iter()
            .filter(|index| owned_lists.contains(&index.source_list))
            .map(|index| index.id)
            .collect();
        ownership.effects = regions
            .iter()
            .flat_map(|region| &region.ops)
            .filter_map(|op| match &op.kind {
                PlanOpKind::StateUpdate {
                    effect: Some(effect),
                    ..
                } if owned_static_owners.contains(&effect.owner.static_owner) => {
                    Some(effect.invocation_id)
                }
                _ => None,
            })
            .collect();
        ownership = ownership.canonicalized();

        let mut instance = instance.clone();
        instance.ownership = ownership;
        completed.push(instance);
    }
    completed.sort_by_key(|instance| instance.call_site_id);
    Ok(completed)
}

pub(crate) fn validate_producer_function_effect_ownership(
    instances: &[ProducerFunctionInstancePlan],
    regions: &[OperationRegion],
    effects: &[EffectContract],
) -> Result<(), PlanError> {
    let invocations = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            } => Some((effect.invocation_id, effect)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let contracts = effects
        .iter()
        .map(|contract| (contract.effect_id, contract))
        .collect::<BTreeMap<_, _>>();

    for instance in instances {
        for invocation_id in &instance.ownership.effects {
            let invocation = invocations.get(invocation_id).ok_or_else(|| {
                PlanError::new("distributed producer function owns a missing effect invocation")
            })?;
            let contract = contracts.get(&invocation.effect_id).ok_or_else(|| {
                PlanError::new(
                    "distributed producer function effect invocation has no host contract",
                )
            })?;
            if matches!(contract.replay, EffectReplay::Idempotent { .. }) {
                return Err(PlanError::new(format!(
                    "distributed producer function owns durable idempotent outbox effect `{}`; producer function instances are process-local and cannot own durable outbox effects",
                    contract.host_operation
                )));
            }
        }
    }
    Ok(())
}

fn plan_state_owner(
    program: &ErasedProgram,
    state: &ir::StateCell,
) -> Result<PlanOwner, PlanError> {
    let bindings = program
        .scope_index
        .bindings
        .iter()
        .filter(|binding| {
            matches!(
                binding.target,
                ir::ErasedBindingTarget::State { runtime, .. } if runtime == state.id
            )
        })
        .collect::<Vec<_>>();
    let [binding] = bindings.as_slice() else {
        return Err(PlanError::new(format!(
            "state `{}` has {} storage bindings; exactly one is required for structural ownership",
            state.path,
            bindings.len()
        )));
    };
    if binding.static_owner != state.static_owner {
        return Err(PlanError::new(format!(
            "state `{}` storage owner {:?} does not match state owner {:?}",
            state.path, binding.static_owner, state.static_owner
        )));
    }
    let owner = plan_owner_from_parts(
        program,
        binding.static_owner,
        &binding.owner_ancestry,
        &format!("state `{}`", state.path),
    )?;
    if plan_scope_id(state.scope_id) != owner.ancestors.last().map(|ancestor| ancestor.scope) {
        return Err(PlanError::new(format!(
            "state `{}` scope {:?} does not match its structural owner ancestry",
            state.path, state.scope_id
        )));
    }
    Ok(owner)
}

fn plan_indexed_state_field(
    program: &ErasedProgram,
    state: &ir::StateCell,
) -> Result<Option<FieldId>, PlanError> {
    if !state.indexed {
        return Ok(None);
    }
    let fields = program
        .scope_index
        .bindings
        .iter()
        .filter_map(|binding| match binding.target {
            ir::ErasedBindingTarget::State {
                runtime,
                field: Some(field),
                ..
            } if runtime == state.id => Some(field),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let fields = fields.into_iter().collect::<Vec<_>>();
    let [field] = fields.as_slice() else {
        return Err(PlanError::new(format!(
            "indexed state `{}` does not own exactly one erased row field",
            state.path
        )));
    };
    Ok(Some(plan_field_id(*field)))
}

fn plan_source_owner(
    program: &ErasedProgram,
    source: &ir::SourcePort,
) -> Result<PlanOwner, PlanError> {
    let erased = program
        .scope_index
        .sources
        .get(source.id.as_usize())
        .filter(|candidate| candidate.source == source.id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "source `{}` has no erased structural ownership",
                source.path
            ))
        })?;
    if erased.static_owner != source.static_owner {
        return Err(PlanError::new(format!(
            "source `{}` erased owner {:?} does not match source owner {:?}",
            source.path, erased.static_owner, source.static_owner
        )));
    }

    let owner = plan_owner_from_parts(
        program,
        erased.static_owner,
        &erased.owner_ancestry,
        &format!("source `{}`", source.path),
    )?;
    if plan_scope_id(source.scope_id) != owner.ancestors.last().map(|ancestor| ancestor.scope) {
        return Err(PlanError::new(format!(
            "source `{}` scope {:?} does not match its structural owner ancestry",
            source.path, source.scope_id
        )));
    }
    Ok(owner)
}

fn plan_event_cause_owner(
    program: &ErasedProgram,
    cause: ir::EventCause,
) -> Result<PlanOwner, PlanError> {
    match cause {
        ir::EventCause::Source(source_id) => {
            let source = program
                .sources
                .get(source_id.as_usize())
                .filter(|source| source.id == source_id)
                .ok_or_else(|| {
                    PlanError::new(format!("event cause references missing source {source_id}"))
                })?;
            plan_source_owner(program, source)
        }
        ir::EventCause::State(state_id) => {
            let state = program
                .state_cells
                .get(state_id.as_usize())
                .filter(|state| state.id == state_id)
                .ok_or_else(|| {
                    PlanError::new(format!("event cause references missing state {state_id}"))
                })?;
            plan_state_owner(program, state)
        }
        ir::EventCause::Pulse(pulse_id) => {
            let pulse = program
                .pulse_batches()
                .get(pulse_id.as_usize())
                .filter(|pulse| pulse.id == pulse_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "event cause references missing pulse batch {pulse_id}"
                    ))
                })?;
            if let Some(state_id) = pulse.state {
                let state = program
                    .state_cells
                    .get(state_id.as_usize())
                    .filter(|state| state.id == state_id)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "pulse batch {pulse_id} references missing state {state_id}"
                        ))
                    })?;
                plan_state_owner(program, state)
            } else {
                let expression = program
                    .executable
                    .expressions
                    .get(pulse.call_expression.as_usize())
                    .filter(|expression| expression.id == pulse.call_expression)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "pulse batch {pulse_id} references missing call expression {}",
                            pulse.call_expression
                        ))
                    })?;
                plan_owner_for_static_owner(
                    program,
                    expression.owner,
                    &format!("pulse batch {pulse_id}"),
                )
            }
        }
    }
}

fn plan_source_row_projections(
    program: &ErasedProgram,
    source: &ir::SourcePort,
) -> Result<Vec<SourceRowProjection>, PlanError> {
    let mut projections = program
        .scope_index
        .row_source_projections
        .iter()
        .filter(|projection| projection.source == source.id)
        .map(|projection| SourceRowProjection {
            list: plan_list_id(projection.row.list),
            path: projection.path.clone(),
        })
        .collect::<Vec<_>>();
    projections.sort();
    projections.dedup();
    if projections
        .iter()
        .any(|projection| projection.path.is_empty())
    {
        return Err(PlanError::new(format!(
            "source `{}` has an empty row projection",
            source.path
        )));
    }
    Ok(projections)
}

fn demand_plan(
    program: &ErasedProgram,
    scalar_fields: &ScalarFieldCatalog,
    root_computation_fields: &BTreeSet<FieldId>,
    document: Option<&DocumentPlan>,
) -> Result<DemandPlan, PlanError> {
    let mut outputs_by_producer = BTreeMap::<ir::ExecutableExprId, BTreeSet<FieldId>>::new();
    for derived in &program.derived_values {
        let ValueRef::Field(field) = derived_output_ref(derived, scalar_fields)? else {
            continue;
        };
        if !root_computation_fields.contains(&field) {
            continue;
        }
        let producer = derived.producer;
        if program
            .executable
            .expressions
            .get(producer.as_usize())
            .is_none_or(|expression| expression.id != producer)
        {
            continue;
        }
        outputs_by_producer
            .entry(producer)
            .or_default()
            .insert(field);
    }

    let reads_by_expression = program
        .scope_index
        .reads
        .iter()
        .map(|read| (read.expression, read.id))
        .collect::<BTreeMap<_, _>>();
    let mut selected = BTreeSet::new();
    let mut visited_reads = BTreeSet::new();
    let mut visited_expressions = BTreeSet::new();
    for output in &program.output_values {
        collect_demanded_expression_outputs(
            program,
            output.value_expression_id,
            root_computation_fields,
            &outputs_by_producer,
            &reads_by_expression,
            &mut visited_reads,
            &mut visited_expressions,
            &mut selected,
        )?;
    }
    if let Some(document) = document {
        collect_document_demand_outputs(document, root_computation_fields, &mut selected)?;
    }
    Ok(DemandPlan {
        root_derived_outputs: RootOutputDemand::Selected(selected.into_iter().collect()),
    })
}

fn collect_document_demand_outputs(
    document: &DocumentPlan,
    root_computation_fields: &BTreeSet<FieldId>,
    selected: &mut BTreeSet<FieldId>,
) -> Result<(), PlanError> {
    let mut select = |field: FieldId, location: &str| {
        if !root_computation_fields.contains(&field) {
            return Err(PlanError::new(format!(
                "{location} references root field {} without an executable root computation",
                field.0
            )));
        }
        selected.insert(field);
        Ok(())
    };

    for expression in &document.expressions {
        if let DocumentExprOp::Read {
            read: DocumentRead::Field { field },
        } = &expression.op
        {
            select(*field, "retained document expression")?;
        }
    }
    for materialization in &document.materializations {
        if let DocumentMaterializationSource::Field { field } = &materialization.source {
            select(*field, "retained document materialization")?;
        }
    }
    for binding in &document.view_bindings {
        if let DocumentBindingTarget::Field { field } = &binding.target {
            select(*field, "retained document binding")?;
        }
    }
    Ok(())
}

fn root_computation_fields(regions: &[OperationRegion]) -> BTreeSet<FieldId> {
    regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| &region.ops)
        .filter(|op| !op.indexed)
        .filter_map(|op| match op.output {
            Some(ValueRef::Field(field)) => Some(field),
            _ => None,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn collect_demanded_read_outputs(
    program: &ErasedProgram,
    read: ir::ErasedReadId,
    demanded_outputs: &BTreeSet<FieldId>,
    outputs_by_producer: &BTreeMap<ir::ExecutableExprId, BTreeSet<FieldId>>,
    reads_by_expression: &BTreeMap<ir::ExecutableExprId, ir::ErasedReadId>,
    visited_reads: &mut BTreeSet<ir::ErasedReadId>,
    visited_expressions: &mut BTreeSet<ir::ExecutableExprId>,
    selected: &mut BTreeSet<FieldId>,
) -> Result<(), PlanError> {
    if !visited_reads.insert(read) {
        return Ok(());
    }
    let read = program
        .scope_index
        .reads
        .get(read.as_usize())
        .filter(|candidate| candidate.id == read)
        .ok_or_else(|| PlanError::new(format!("demand references missing erased read {read}")))?;
    match &read.target {
        ir::ErasedReadTarget::Binding { binding, .. } => {
            let binding = program
                .scope_index
                .bindings
                .get(binding.as_usize())
                .filter(|candidate| candidate.id == *binding)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "demand read {} references missing binding {binding}",
                        read.id
                    ))
                })?;
            let mut exact = BTreeSet::new();
            if let ir::ErasedBindingTarget::Value {
                field: Some(field),
                row: None,
            } = binding.target
            {
                exact.insert(plan_field_id(field));
            }
            if let Some(outputs) = outputs_by_producer.get(&binding.producer) {
                exact.extend(outputs.iter().copied());
            }
            exact.retain(|field| demanded_outputs.contains(field));
            if exact.is_empty() && matches!(binding.target, ir::ErasedBindingTarget::Value { .. }) {
                collect_demanded_expression_outputs(
                    program,
                    binding.producer,
                    demanded_outputs,
                    outputs_by_producer,
                    reads_by_expression,
                    visited_reads,
                    visited_expressions,
                    selected,
                )?;
            } else {
                selected.extend(exact);
            }
        }
        ir::ErasedReadTarget::Expression { expression, .. }
        | ir::ErasedReadTarget::Local {
            value: expression, ..
        } => collect_demanded_expression_outputs(
            program,
            *expression,
            demanded_outputs,
            outputs_by_producer,
            reads_by_expression,
            visited_reads,
            visited_expressions,
            selected,
        )?,
        ir::ErasedReadTarget::SourcePayload { .. }
        | ir::ErasedReadTarget::StateProjection { .. }
        | ir::ErasedReadTarget::ExternalValue { .. }
        | ir::ErasedReadTarget::ElementState { .. }
        | ir::ErasedReadTarget::MaterializationLocal { .. }
        | ir::ErasedReadTarget::FunctionParameter { .. } => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn collect_demanded_expression_outputs(
    program: &ErasedProgram,
    expression: ir::ExecutableExprId,
    demanded_outputs: &BTreeSet<FieldId>,
    outputs_by_producer: &BTreeMap<ir::ExecutableExprId, BTreeSet<FieldId>>,
    reads_by_expression: &BTreeMap<ir::ExecutableExprId, ir::ErasedReadId>,
    visited_reads: &mut BTreeSet<ir::ErasedReadId>,
    visited_expressions: &mut BTreeSet<ir::ExecutableExprId>,
    selected: &mut BTreeSet<FieldId>,
) -> Result<(), PlanError> {
    if !visited_expressions.insert(expression) {
        return Ok(());
    }
    if let Some(outputs) = outputs_by_producer.get(&expression) {
        selected.extend(outputs.iter().copied());
        return Ok(());
    }
    if let Some(read) = reads_by_expression.get(&expression).copied() {
        return collect_demanded_read_outputs(
            program,
            read,
            demanded_outputs,
            outputs_by_producer,
            reads_by_expression,
            visited_reads,
            visited_expressions,
            selected,
        );
    }
    let expression = program
        .executable
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| {
            PlanError::new(format!(
                "demand traversal reaches missing expression {expression}"
            ))
        })?;
    for child in ir::executable_expression_children(&expression.kind) {
        collect_demanded_expression_outputs(
            program,
            child,
            demanded_outputs,
            outputs_by_producer,
            reads_by_expression,
            visited_reads,
            visited_expressions,
            selected,
        )?;
    }
    Ok(())
}

fn effect_contracts(program: &ErasedProgram) -> Result<Vec<EffectContract>, PlanError> {
    let mut effects = BTreeMap::new();
    for expression in &program.executable.expressions {
        let host_operation = match &expression.kind {
            ir::ExecutableExpressionKind::Call { name, .. } => name.as_str(),
            _ => continue,
        };
        let Some(contract) = builtin_effect_contract(host_operation)? else {
            continue;
        };
        if let Err(error) = contract.validate() {
            return Err(PlanError::new(format!(
                "host effect `{host_operation}` has no safe durable replay contract: {error}"
            )));
        }
        if let Some(existing) = effects.insert(contract.effect_id, contract.clone())
            && existing != contract
        {
            return Err(PlanError::new(format!(
                "host effect `{host_operation}` has conflicting centralized contracts"
            )));
        }
    }
    Ok(effects.into_values().collect())
}

fn effect_outbox_schemas(effects: &[EffectContract]) -> Result<Vec<EffectOutboxSchema>, PlanError> {
    let mut schemas = Vec::new();
    for contract in effects {
        let EffectReplay::Idempotent { .. } = &contract.replay else {
            continue;
        };
        let schema = builtin_effect_outbox_schema(&contract.host_operation)?.ok_or_else(|| {
            PlanError::new(format!(
                "idempotent host effect `{}` is missing a centralized intent/result outbox schema",
                contract.host_operation
            ))
        })?;
        schemas.push(schema);
    }
    schemas.sort_by_key(|schema| schema.effect_id);
    Ok(schemas)
}

fn bind_effect_outbox_invocations(
    schemas: &mut [EffectOutboxSchema],
    regions: &[OperationRegion],
) -> Result<(), PlanError> {
    let mut invocations = BTreeMap::<EffectId, Vec<EffectInvocationId>>::new();
    for invocation in regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(invocation),
                ..
            }
            | PlanOpKind::EffectUpdate {
                effect: invocation, ..
            } => Some(invocation),
            _ => None,
        })
    {
        invocations
            .entry(invocation.effect_id)
            .or_default()
            .push(invocation.invocation_id);
    }
    for (effect_id, invocation_ids) in invocations {
        if let Some(schema) = schemas
            .iter_mut()
            .find(|schema| schema.effect_id == effect_id)
        {
            schema.bind_invocations(invocation_ids);
        }
    }
    Ok(())
}

fn effect_invocation_for_operation(
    host_operation: &str,
    target_path: &str,
    gate: PlanRowExpressionId,
    intent_expressions: Vec<(String, PlanRowExpressionId)>,
    output: Option<ValueRef>,
    owner: PlanOwner,
) -> Result<EffectInvocationPlan, PlanError> {
    let contract = builtin_effect_contract(host_operation)?.ok_or_else(|| {
        PlanError::new(format!(
            "effectful update has no centralized contract for `{host_operation}`"
        ))
    })?;
    contract.validate()?;
    let target = output.ok_or_else(|| {
        PlanError::new(format!(
            "effectful update `{}` has no result target",
            target_path
        ))
    })?;
    let schema = contract.schema.as_ref().ok_or_else(|| {
        PlanError::new(format!(
            "effectful update has no centralized typed schema for `{host_operation}`"
        ))
    })?;
    let DataTypePlan::Record {
        fields: intent_schema,
        open: false,
    } = &schema.intent_type
    else {
        return Err(PlanError::new(format!(
            "effectful update `{host_operation}` has a non-record intent schema"
        )));
    };
    if intent_schema.len() != intent_expressions.len() {
        return Err(PlanError::new(format!(
            "effectful update `{host_operation}` resolved {} of {} schema intent fields",
            intent_expressions.len(),
            intent_schema.len()
        )));
    }
    let expressions = intent_expressions.into_iter().collect::<BTreeMap<_, _>>();
    Ok(EffectInvocationPlan {
        invocation_id: EffectInvocationId::from_result_owner(contract.effect_id, target_path)?,
        effect_id: contract.effect_id,
        owner,
        gate,
        intent_fields: intent_schema
            .iter()
            .map(|field| {
                Ok(EffectIntentFieldPlan {
                    name: field.name.clone(),
                    expression: expressions.get(&field.name).cloned().ok_or_else(|| {
                        PlanError::new(format!(
                            "effectful update `{host_operation}` lost intent field `{}`",
                            field.name
                        ))
                    })?,
                    data_type: field.data_type.clone(),
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?,
        idempotency_key: EffectIdempotencyKeyPlan::InvocationTurnIntentSha256,
        result: EffectResultRoute::Target {
            target,
            policy: contract.result_policy,
        },
        barrier: contract.barrier,
    })
}

fn output_root_plans(
    program: &ErasedProgram,
    document: Option<&DocumentPlan>,
    index: &ValueIndex,
) -> Result<Vec<OutputRootPlan>, PlanError> {
    let mut outputs = Vec::with_capacity(program.output_values.len());
    for output in &program.output_values {
        let demand = match output.demand {
            ir::SemanticOutputDemandPolicy::HostDemanded => OutputDemandPolicy::HostDemanded,
        };
        let (contract, value) = match output.contract {
            ir::SemanticOutputContractKind::RetainedVisual { kind } => {
                let document = document.ok_or_else(|| {
                    PlanError::new(format!(
                        "retained visual output root `{}` has no compiled document value",
                        output.root
                    ))
                })?;
                let contract = match kind {
                    ir::SemanticRetainedVisualKind::Document => OutputContractKind::Document,
                    ir::SemanticRetainedVisualKind::Scene => OutputContractKind::Scene,
                };
                let expected = match document.root.kind {
                    DocumentRootKind::Document => OutputContractKind::Document,
                    DocumentRootKind::Scene => OutputContractKind::Scene,
                };
                if contract != expected {
                    return Err(PlanError::new(format!(
                        "retained visual output root `{}` does not match its document value",
                        output.root
                    )));
                }
                (
                    contract,
                    OutputValueRef::RetainedVisual {
                        expression: document.root.expression,
                    },
                )
            }
            ir::SemanticOutputContractKind::HostValue => {
                let data_type = output.data_type.as_ref().ok_or_else(|| {
                    PlanError::new(format!(
                        "host output root `{}` has no closed inferred data type",
                        output.root
                    ))
                })?;
                let value = index.resolve_storage(output.binding_id).ok_or_else(|| {
                    PlanError::new(format!(
                        "host output root `{}` has no machine value for storage binding {}",
                        output.root, output.binding_id
                    ))
                })?;
                let data_type = semantic_data_type_plan(data_type);
                let list_fields = output_list_field_refs(program, &value, &data_type)?;
                (
                    OutputContractKind::HostValue { data_type },
                    OutputValueRef::RuntimeValue { value, list_fields },
                )
            }
        };
        outputs.push(OutputRootPlan::new(
            output.root.clone(),
            contract,
            demand,
            value,
        )?);
    }
    outputs.sort_by(|left, right| left.name.cmp(&right.name));
    if outputs.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err(PlanError::new("typed output root names must be unique"));
    }
    Ok(outputs)
}

fn output_list_field_refs(
    program: &ErasedProgram,
    value: &ValueRef,
    data_type: &DataTypePlan,
) -> Result<Vec<OutputListFieldRef>, PlanError> {
    let (ValueRef::List(list_id), DataTypePlan::List { item }) = (value, data_type) else {
        return Ok(Vec::new());
    };
    let names = match item.as_ref() {
        DataTypePlan::Record {
            fields,
            open: false,
        } => fields
            .iter()
            .map(|field| field.name.clone())
            .collect::<Vec<_>>(),
        _ => vec!["value".to_owned()],
    };
    names
        .into_iter()
        .map(|name| {
            let field_id =
                row_input_field_id_for_list_id(program, *list_id, &name).ok_or_else(|| {
                    PlanError::new(format!(
                        "host list output from ListId {} has no exact row field `{name}`",
                        list_id.0
                    ))
                })?;
            Ok(OutputListFieldRef {
                list_id: *list_id,
                name,
                field_id,
            })
        })
        .collect()
}

fn host_port_plans(
    program: &ErasedProgram,
    outputs: &[OutputRootPlan],
) -> Result<Vec<HostPortPlan>, PlanError> {
    let source_id = |path: &str, line: usize| {
        program
            .sources
            .iter()
            .find(|source| source.path == path)
            .map(|source| plan_source_id(source.id))
            .ok_or_else(|| {
                PlanError::new(format!(
                    "host port at line {line} references missing source `{path}`"
                ))
            })
    };
    let output_id = |name: &str, line: usize| {
        outputs
            .iter()
            .find(|output| output.name == name)
            .map(|output| output.id)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "host port at line {line} references missing output root `{name}`"
                ))
            })
    };

    program
        .host_ports
        .iter()
        .map(|port| match port {
            ir::HostPortDeclaration::HttpServer {
                line,
                request_source,
                disconnect_source,
                response_output,
            } => Ok(HostPortPlan::HttpServer {
                request_source: source_id(request_source, *line)?,
                disconnect_source: disconnect_source
                    .as_deref()
                    .map(|source| source_id(source, *line))
                    .transpose()?,
                response_output: output_id(response_output, *line)?,
            }),
            ir::HostPortDeclaration::WebSocketServer {
                line,
                open_source,
                message_source,
                close_source,
                error_source,
                actions_output,
            } => Ok(HostPortPlan::WebSocketServer {
                open_source: source_id(open_source, *line)?,
                message_source: source_id(message_source, *line)?,
                close_source: source_id(close_source, *line)?,
                error_source: source_id(error_source, *line)?,
                actions_output: output_id(actions_output, *line)?,
            }),
        })
        .collect()
}

fn source_payload_schema_from_ir(
    _program: &ErasedProgram,
    source: &ir::SourcePort,
) -> Result<SourcePayloadSchema, PlanError> {
    let value = &source.payload_schema;
    Ok(SourcePayloadSchema {
        fields: value
            .fields
            .iter()
            .map(source_payload_field_from_ir)
            .collect(),
        typed_fields: value
            .typed_fields
            .iter()
            .map(source_payload_descriptor_from_ir)
            .collect(),
    })
}

fn source_payload_descriptor_from_ir(
    value: &ir::SourcePayloadDescriptor,
) -> SourcePayloadDescriptor {
    SourcePayloadDescriptor {
        field: source_payload_field_from_ir(&value.field),
        data_type: semantic_data_type_plan(&value.data_type).canonicalized(),
    }
}

fn source_payload_field_from_ir(value: &ir::SourcePayloadField) -> SourcePayloadField {
    match value {
        ir::SourcePayloadField::Address => SourcePayloadField::Address,
        ir::SourcePayloadField::Bytes => SourcePayloadField::Bytes,
        ir::SourcePayloadField::Key => SourcePayloadField::Key,
        ir::SourcePayloadField::Named(name) => SourcePayloadField::Named(name.clone()),
        ir::SourcePayloadField::Text => SourcePayloadField::Text,
    }
}

fn state_executable_value_type(
    program: &ErasedProgram,
    state: &boon_ir::StateCell,
) -> PlanValueType {
    executable_state_initializer(program, state)
        .and_then(|expression| inferred_executable_expression_value_type(program, expression))
        .filter(|value_type| plan_value_type_is_concrete(*value_type))
        .or_else(|| {
            semantic_memory_for_state(program, state)
                .map(|memory| semantic_data_type_plan(&memory.data_type).canonicalized())
                .map(|data_type| plan_value_type_from_semantic_data_type(&data_type))
                .filter(|value_type| plan_value_type_is_concrete(*value_type))
        })
        .unwrap_or(PlanValueType::Unknown)
}

fn list_initializer_kind_from_ir(value: &ListInitializer) -> ListInitializerKind {
    match value {
        ListInitializer::RecordLiteral { .. } => ListInitializerKind::RecordLiteral,
        ListInitializer::Range { .. } => ListInitializerKind::Range,
        ListInitializer::Empty => ListInitializerKind::Empty,
        ListInitializer::Unknown { .. } => ListInitializerKind::Unknown,
    }
}

fn plan_range_initializer(value: &ListInitializer) -> Option<PlanRangeInitializer> {
    match value {
        ListInitializer::Range { from, to } => Some(PlanRangeInitializer {
            from: *from,
            to: *to,
        }),
        ListInitializer::RecordLiteral { .. }
        | ListInitializer::Empty
        | ListInitializer::Unknown { .. } => None,
    }
}

fn plan_derived_kind_from_ir(value: &DerivedValueKind) -> PlanDerivedKind {
    match value {
        DerivedValueKind::SourceEventTransform => PlanDerivedKind::SourceEventTransform,
        DerivedValueKind::ListView => PlanDerivedKind::ListView,
        DerivedValueKind::Aggregate => PlanDerivedKind::Aggregate,
        DerivedValueKind::Pure => PlanDerivedKind::Pure,
        DerivedValueKind::Unknown => PlanDerivedKind::Unknown,
    }
}

fn state_initial_provenance(_slot: &ScalarStorageSlot) -> InitialProvenance {
    InitialProvenance::ReconstructableDefault
}

#[derive(Clone)]
struct MigrationStorageDefault {
    value_type: PlanValueType,
    constant: Option<PlanConstantValue>,
    indexed_edge: Option<ir::MigrationEdge>,
}

fn plan_value_type_from_semantic_data_type(data_type: &DataTypePlan) -> PlanValueType {
    match data_type {
        DataTypePlan::Text => PlanValueType::Text,
        DataTypePlan::Number => PlanValueType::Number,
        DataTypePlan::Bytes { fixed_len } => PlanValueType::Bytes {
            fixed_len: *fixed_len,
        },
        DataTypePlan::Bits { width } => PlanValueType::Bits { width: *width },
        DataTypePlan::Variant { .. } => PlanValueType::Tag,
        DataTypePlan::Record { .. }
        | DataTypePlan::List { .. }
        | DataTypePlan::Map { .. }
        | DataTypePlan::Set { .. }
        | DataTypePlan::Union { .. } => PlanValueType::Data,
        DataTypePlan::Unknown => PlanValueType::Unknown,
    }
}

fn deterministic_fresh_constant(data_type: &DataTypePlan) -> Option<PlanConstantValue> {
    match data_type {
        DataTypePlan::Text => Some(PlanConstantValue::Text {
            value: String::new(),
        }),
        DataTypePlan::Number => Some(PlanConstantValue::Number {
            value: ExactNumber::zero(),
        }),
        DataTypePlan::Bits { width } => Some(PlanConstantValue::Bits {
            value: boon_data::Bits::zero(*width).ok()?,
        }),
        DataTypePlan::Bytes {
            fixed_len: None | Some(0),
        } => {
            let mut hasher = Sha256::new();
            hasher.update([]);
            Some(PlanConstantValue::Bytes {
                byte_len: 0,
                sha256: format!("{:x}", hasher.finalize()),
                inline_bytes: Some(Vec::new()),
            })
        }
        DataTypePlan::Variant { variants } => {
            variants.first().map(|variant| PlanConstantValue::Tag {
                name: variant.tag.clone(),
            })
        }
        DataTypePlan::Union { members } => members.first().and_then(deterministic_fresh_constant),
        DataTypePlan::Record { .. }
        | DataTypePlan::List { .. }
        | DataTypePlan::Map { .. }
        | DataTypePlan::Set { .. } => None,
        DataTypePlan::Bytes { fixed_len: Some(_) } | DataTypePlan::Unknown => None,
    }
}

fn semantic_memory_for_state<'a>(
    program: &'a ErasedProgram,
    state: &ir::StateCell,
) -> Option<&'a ir::SemanticMemory> {
    program.semantic_memory.iter().find(|memory| {
        semantic_memory_is_runtime_active(program, memory)
            && matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
                    | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. }
                    if state_id == state.id
            )
    })
}

struct MigrationListStorageDefault {
    initializer_kind: ListInitializerKind,
    range: Option<PlanRangeInitializer>,
    initial_rows: Vec<PlanInitialListRow>,
}

fn whole_list_migration_source<'a>(
    program: &'a ErasedProgram,
    list: &ir::ListMemory,
) -> Result<Option<&'a ir::ListMemory>, PlanError> {
    let Some(destination_memory) = program.semantic_memory.iter().find(|memory| {
        semantic_memory_is_active(memory)
            && matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List { list_id, .. } if list_id == list.id
            )
    }) else {
        return Ok(None);
    };
    let Some(edge) = program.migration_edges.iter().find(|edge| {
        edge.transfer_kind == ir::MigrationTransferKind::List
            && edge.destination.memory_id == destination_memory.id
    }) else {
        return Ok(None);
    };
    if edge.transform != ir::MigrationTransform::Identity || edge.source_leaves.len() != 1 {
        return Err(PlanError::new(
            "whole-list migration default requires one identity source",
        ));
    }
    let source_memory = program
        .semantic_memory
        .get(edge.source_leaves[0].memory_id.as_usize())
        .ok_or_else(|| PlanError::new("whole-list migration default source memory is absent"))?;
    let source_list_id = match source_memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => list_id,
        _ => {
            return Err(PlanError::new(
                "whole-list migration default source is not a list",
            ));
        }
    };
    program
        .lists
        .iter()
        .find(|source| source.id == source_list_id)
        .map(Some)
        .ok_or_else(|| PlanError::new("whole-list migration default source list is absent"))
}

fn migration_list_storage_default(
    program: &ErasedProgram,
    list: &ir::ListMemory,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    list_indexes: &mut Vec<PlanListIndex>,
) -> Result<Option<MigrationListStorageDefault>, PlanError> {
    let Some(source_list) = whole_list_migration_source(program, list)? else {
        return Ok(None);
    };
    if matches!(source_list.initializer, ListInitializer::Unknown { .. }) {
        return Err(PlanError::new(format!(
            "whole-list migration from `{}` cannot reconstruct sparse default rows",
            source_list.name
        )));
    }
    let initial_rows = plan_initial_list_rows(
        program,
        list,
        &source_list.initializer,
        index,
        arena,
        constants,
        list_indexes,
    )?;
    if initial_rows
        .iter()
        .flat_map(|row| &row.fields)
        .any(|field| field.field_id.is_none())
    {
        return Err(PlanError::new(format!(
            "whole-list migration from `{}` cannot map a default row field into `{}`",
            source_list.name, list.name
        )));
    }
    Ok(Some(MigrationListStorageDefault {
        initializer_kind: list_initializer_kind_from_ir(&source_list.initializer),
        range: plan_range_initializer(&source_list.initializer),
        initial_rows,
    }))
}

fn compiled_list_storage_slot(
    program: &ErasedProgram,
    list: &ir::ListMemory,
    id: PlanStorageId,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    list_indexes: &mut Vec<PlanListIndex>,
) -> Result<ListStorageSlot, PlanError> {
    let migration_default =
        migration_list_storage_default(program, list, index, arena, constants, list_indexes)?;
    Ok(ListStorageSlot {
        id,
        list_id: plan_list_id(list.id),
        scope_id: plan_scope_id(list.row_scope_id),
        row_fields: list_row_fields(program, list),
        capacity: list.capacity,
        hidden_key_type: list.hidden_key_type.clone(),
        has_generation: list.has_generation,
        initializer_kind: migration_default.as_ref().map_or_else(
            || list_initializer_kind_from_ir(&list.initializer),
            |value| value.initializer_kind,
        ),
        range: migration_default.as_ref().map_or_else(
            || plan_range_initializer(&list.initializer),
            |value| value.range,
        ),
        initial_rows: migration_default.map_or_else(
            || {
                plan_initial_list_rows(
                    program,
                    list,
                    &list.initializer,
                    index,
                    arena,
                    constants,
                    list_indexes,
                )
            },
            |value| Ok(value.initial_rows),
        )?,
    })
}

fn migration_identity_source_constant(
    program: &ErasedProgram,
    edge: &ir::MigrationEdge,
) -> Option<PlanConstantValue> {
    if edge.transform != ir::MigrationTransform::Identity || edge.source_leaves.len() != 1 {
        return None;
    }
    let source_memory = program
        .semantic_memory
        .get(edge.source_leaves[0].memory_id.as_usize())?;
    let source_state_id = match source_memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => state_id,
        ir::SemanticMemoryRuntimeBacking::List { .. }
        | ir::SemanticMemoryRuntimeBacking::Collection { .. } => return None,
    };
    let source_state = program
        .state_cells
        .iter()
        .find(|state| state.id == source_state_id)?;
    constant_state_initial_expression_value(program, source_state)
}

fn migration_storage_default(
    program: &ErasedProgram,
    state: &ir::StateCell,
) -> Option<MigrationStorageDefault> {
    let memory = semantic_memory_for_state(program, state)?;
    let edge = program
        .migration_edges
        .iter()
        .find(|edge| edge.destination.memory_id == memory.id)?;
    let data_type = semantic_data_type_plan(&memory.data_type).canonicalized();
    let value_type = plan_value_type_from_semantic_data_type(&data_type);
    if value_type == PlanValueType::Unknown {
        return None;
    }
    if state.indexed && edge.transfer_kind == ir::MigrationTransferKind::IndexedField {
        return Some(MigrationStorageDefault {
            value_type,
            constant: None,
            indexed_edge: Some(edge.clone()),
        });
    }
    let constant = migration_identity_source_constant(program, edge)
        .or_else(|| deterministic_fresh_constant(&data_type))?;
    Some(MigrationStorageDefault {
        value_type,
        constant: Some(constant),
        indexed_edge: None,
    })
}

fn list_initial_provenance(slot: &ListStorageSlot) -> InitialProvenance {
    match slot.initializer_kind {
        ListInitializerKind::Unknown => InitialProvenance::MaterializedAuthority,
        ListInitializerKind::RecordLiteral
        | ListInitializerKind::Range
        | ListInitializerKind::Empty => InitialProvenance::ReconstructableDefault,
    }
}

fn exact_reconstructable_list_literal(
    output: &ValueRef,
    expression: Option<&PlanDerivedExpression>,
    list_slots: &[ListStorageSlot],
    constants: &[PlanConstant],
    arena: &PlanRowExpressionArena,
) -> Result<bool, PlanError> {
    let ValueRef::List(list_id) = output else {
        return Ok(false);
    };
    let Some(slot) = list_slots.iter().find(|slot| slot.list_id == *list_id) else {
        return Ok(false);
    };
    if slot.initializer_kind != ListInitializerKind::RecordLiteral {
        return Ok(false);
    }
    let Some(PlanDerivedExpression::RowExpression { expression }) = expression else {
        return Ok(false);
    };
    let PlanRowExpressionNode::ListLiteral { items } = arena.node(*expression)? else {
        return Ok(false);
    };
    if items.len() != slot.initial_rows.len() {
        return Ok(false);
    }
    for (item, initial) in items.iter().zip(&slot.initial_rows) {
        let PlanRowExpressionNode::Object { fields } = arena.node(*item)? else {
            return Ok(false);
        };
        if fields.iter().any(|field| field.spread) || fields.len() != initial.fields.len() {
            return Ok(false);
        }
        for actual in fields {
            let Some(expected) = initial
                .fields
                .iter()
                .find(|expected| expected.name == actual.name)
            else {
                return Ok(false);
            };
            let matches = match &expected.initializer {
                PlanInitialListFieldInitializer::Constant { value } => {
                    plan_row_expression_matches_constant(arena, actual.value, value, constants)?
                }
                PlanInitialListFieldInitializer::Expression { expression } => {
                    actual.value == *expression
                }
            };
            if !matches {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn plan_row_expression_matches_constant(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    expected: &PlanConstantValue,
    constants: &[PlanConstant],
) -> Result<bool, PlanError> {
    if let PlanRowExpressionNode::Constant { constant_id } = arena.node(expression)? {
        return Ok(constants
            .get(constant_id.0)
            .is_some_and(|constant| constant.id == *constant_id && constant.value == *expected));
    }
    Ok(matches!(
        expected,
        PlanConstantValue::Data { value }
            if plan_row_expression_static_data(arena, expression, constants)?.as_ref() == Some(value)
    ))
}

fn plan_row_expression_static_data(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    constants: &[PlanConstant],
) -> Result<Option<boon_data::Value>, PlanError> {
    Ok(match arena.node(expression)? {
        PlanRowExpressionNode::Constant { constant_id } => {
            let Some(constant) = constants
                .get(constant_id.0)
                .filter(|constant| constant.id == *constant_id)
            else {
                return Ok(None);
            };
            match &constant.value {
                PlanConstantValue::Text { value } => Some(boon_data::Value::Text(value.clone())),
                PlanConstantValue::Number { value } => {
                    Some(boon_data::Value::Number(value.clone()))
                }
                PlanConstantValue::Bytes {
                    inline_bytes: Some(bytes),
                    ..
                } => Some(boon_data::Value::Bytes(bytes.clone().into())),
                PlanConstantValue::Bytes {
                    inline_bytes: None, ..
                } => None,
                PlanConstantValue::Tag { name } => Some(boon_data::Value::tag(name)),
                PlanConstantValue::Bits { value } => Some(boon_data::Value::Bits(value.clone())),
                PlanConstantValue::Data { value } => Some(value.clone()),
            }
        }
        PlanRowExpressionNode::ListLiteral { items } => items
            .iter()
            .map(|item| plan_row_expression_static_data(arena, *item, constants))
            .collect::<Result<Option<Vec<_>>, _>>()?
            .map(boon_data::Value::List),
        PlanRowExpressionNode::Object { fields } if fields.iter().all(|field| !field.spread) => {
            fields
                .iter()
                .map(|field| {
                    let Some(value) =
                        plan_row_expression_static_data(arena, field.value, constants)?
                    else {
                        return Ok(None);
                    };
                    Ok(Some((field.name.clone(), value)))
                })
                .collect::<Result<Option<BTreeMap<_, _>>, PlanError>>()?
                .map(boon_data::Value::Object)
        }
        PlanRowExpressionNode::TaggedObject { tag, fields }
            if fields.iter().all(|field| !field.spread) =>
        {
            fields
                .iter()
                .map(|field| {
                    let Some(value) =
                        plan_row_expression_static_data(arena, field.value, constants)?
                    else {
                        return Ok(None);
                    };
                    Ok(Some((field.name.clone(), value)))
                })
                .collect::<Result<Option<BTreeMap<_, _>>, PlanError>>()?
                .map(|fields| boon_data::Value::Tag {
                    tag: tag.clone(),
                    fields,
                })
        }
        _ => None,
    })
}

fn state_only_authority_map_is_noop(
    expression: Option<&PlanDerivedExpression>,
    materialization: Option<&PlanListMaterialization>,
    arena: &PlanRowExpressionArena,
) -> Result<bool, PlanError> {
    let (
        Some(PlanDerivedExpression::RowExpression { expression }),
        Some(PlanListMaterialization {
            target_list,
            authority_source_list: Some(authority_source_list),
            fields,
            row_field_copies,
            ..
        }),
    ) = (expression, materialization)
    else {
        return Ok(false);
    };
    if target_list != authority_source_list || !fields.is_empty() || !row_field_copies.is_empty() {
        return Ok(false);
    }
    let PlanRowExpressionNode::ContextualCollection {
        operation: PlanContextualOperationKind::Map,
        source,
        body,
        captures,
        indexed_access: None,
        ..
    } = arena.node(*expression)?
    else {
        return Ok(false);
    };
    Ok(captures.is_empty()
        && matches!(
            arena.node(*source)?,
            PlanRowExpressionNode::AuthorityListRef { list_id } if list_id == target_list
        )
        && matches!(
            arena.node(*body)?,
            PlanRowExpressionNode::Object { fields } if fields.is_empty()
        ))
}

fn semantic_data_type_plan(value: &ir::SemanticDataType) -> DataTypePlan {
    match value {
        ir::SemanticDataType::Number => DataTypePlan::Number,
        ir::SemanticDataType::Text => DataTypePlan::Text,
        ir::SemanticDataType::Bytes { fixed_len } => DataTypePlan::Bytes {
            fixed_len: fixed_len.map(|len| len as u64),
        },
        ir::SemanticDataType::Bits { width } => DataTypePlan::Bits { width: *width },
        ir::SemanticDataType::Variant { variants } => DataTypePlan::Variant {
            variants: variants
                .iter()
                .map(|variant| DataVariantPlan {
                    tag: variant.tag.clone(),
                    fields: variant
                        .fields
                        .iter()
                        .map(|field| DataTypeFieldPlan {
                            name: field.name.clone(),
                            data_type: semantic_data_type_plan(&field.data_type),
                        })
                        .collect(),
                    open: variant.open,
                })
                .collect(),
        }
        .canonicalized(),
        ir::SemanticDataType::Record { fields, open } => DataTypePlan::Record {
            fields: fields
                .iter()
                .map(|field| DataTypeFieldPlan {
                    name: field.name.clone(),
                    data_type: semantic_data_type_plan(&field.data_type),
                })
                .collect(),
            open: *open,
        }
        .canonicalized(),
        ir::SemanticDataType::List { item } => DataTypePlan::List {
            item: Box::new(semantic_data_type_plan(item)),
        },
        ir::SemanticDataType::Map { key, value } => DataTypePlan::Map {
            key: Box::new(semantic_data_type_plan(key)),
            value: Box::new(semantic_data_type_plan(value)),
        },
        ir::SemanticDataType::Set { item } => DataTypePlan::Set {
            item: Box::new(semantic_data_type_plan(item)),
        },
        ir::SemanticDataType::Union { members } => DataTypePlan::Union {
            members: members.iter().map(semantic_data_type_plan).collect(),
        }
        .canonicalized(),
        ir::SemanticDataType::Unknown { .. } => DataTypePlan::Unknown,
    }
}

fn semantic_memory_kind(kind: ir::SemanticMemoryKind) -> MemoryKind {
    match kind {
        ir::SemanticMemoryKind::RootScalar => MemoryKind::Scalar,
        ir::SemanticMemoryKind::IndexedField => MemoryKind::IndexedField,
        ir::SemanticMemoryKind::ListOwner => MemoryKind::List,
        ir::SemanticMemoryKind::Map => MemoryKind::Map,
        ir::SemanticMemoryKind::Set => MemoryKind::Set,
    }
}

fn semantic_memory_owner(memory: &ir::SemanticMemory) -> MemoryOwnerPath {
    MemoryOwnerPath {
        canonical_module: memory.identity.canonical_module.clone(),
        named_owner_path: memory.identity.owner_path.clone(),
    }
}

fn semantic_memory_id(memory: &ir::SemanticMemory) -> Result<MemoryId, PlanError> {
    MemoryId::from_identity(
        &semantic_memory_owner(memory),
        &memory.identity.semantic_path,
        semantic_memory_kind(memory.identity.kind),
    )
}

fn semantic_memory_is_active(memory: &ir::SemanticMemory) -> bool {
    matches!(memory.status, ir::SemanticMemoryStatus::Active)
}

fn semantic_memory_is_runtime_active(program: &ErasedProgram, memory: &ir::SemanticMemory) -> bool {
    if !semantic_memory_is_active(memory) {
        return false;
    }
    let ir::SemanticMemoryRuntimeBacking::IndexedState {
        list_id: Some(list_id),
        ..
    } = memory.runtime_backing
    else {
        return true;
    };
    program.semantic_memory.iter().any(|candidate| {
        semantic_memory_is_active(candidate)
            && matches!(
                candidate.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List {
                    list_id: candidate_list_id,
                    ..
                } if candidate_list_id == list_id
            )
    })
}

fn semantic_memory_is_transient_effect_result(
    memory: &ir::SemanticMemory,
    transient_effect_result_targets: &BTreeSet<ValueRef>,
) -> bool {
    let (state_id, field_id) = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, field_id }
        | ir::SemanticMemoryRuntimeBacking::IndexedState {
            state_id, field_id, ..
        } => (state_id, field_id),
        ir::SemanticMemoryRuntimeBacking::List { .. }
        | ir::SemanticMemoryRuntimeBacking::Collection { .. } => return false,
    };
    transient_effect_result_targets.contains(&ValueRef::State(plan_state_id(state_id)))
        || field_id.is_some_and(|field_id| {
            transient_effect_result_targets.contains(&ValueRef::Field(plan_field_id(field_id)))
        })
}

fn state_for_semantic_memory<'a>(
    program: &'a ErasedProgram,
    memory: &ir::SemanticMemory,
) -> Result<&'a ir::StateCell, PlanError> {
    let state_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => state_id,
        ir::SemanticMemoryRuntimeBacking::List { .. }
        | ir::SemanticMemoryRuntimeBacking::Collection { .. } => {
            return Err(PlanError::new(format!(
                "semantic memory `{}` has non-state backing where state backing is required",
                memory.identity.semantic_path
            )));
        }
    };
    program
        .state_cells
        .iter()
        .find(|state| state.id == state_id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic memory `{}` references missing state backing {}",
                memory.identity.semantic_path, state_id.0
            ))
        })
}

fn scalar_slot_for_semantic_memory<'a>(
    memory: &ir::SemanticMemory,
    scalar_slots: &'a [ScalarStorageSlot],
) -> Result<&'a ScalarStorageSlot, PlanError> {
    let state_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
        | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => state_id,
        ir::SemanticMemoryRuntimeBacking::List { .. }
        | ir::SemanticMemoryRuntimeBacking::Collection { .. } => {
            return Err(PlanError::new(format!(
                "semantic memory `{}` has no scalar runtime backing",
                memory.identity.semantic_path
            )));
        }
    };
    scalar_slots
        .iter()
        .find(|slot| slot.state_id == plan_state_id(state_id))
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic memory `{}` cannot resolve state slot {}",
                memory.identity.semantic_path, state_id.0
            ))
        })
}

fn semantic_scalar_memory_plan(
    program: &ErasedProgram,
    memory: &ir::SemanticMemory,
    scalar_slots: &[ScalarStorageSlot],
) -> Result<MemoryPlan, PlanError> {
    let slot = scalar_slot_for_semantic_memory(memory, scalar_slots)?;
    let state = state_for_semantic_memory(program, memory)?;
    if memory.identity.semantic_path == format!("hold_{}", state.source_line) {
        return Err(PlanError::new(format!(
            "persistence identity cannot use anonymous line-based state `{}` at line {}; name the state under a stable semantic owner",
            memory.identity.semantic_path, state.source_line
        )));
    }
    let kind = semantic_memory_kind(memory.identity.kind);
    if !matches!(kind, MemoryKind::Scalar | MemoryKind::IndexedField) {
        return Err(PlanError::new(
            "non-scalar semantic memory cannot use scalar plan",
        ));
    }
    if slot.indexed != (kind == MemoryKind::IndexedField) {
        return Err(PlanError::new(format!(
            "semantic memory `{}` kind disagrees with runtime backing",
            memory.identity.semantic_path
        )));
    }
    let owner = semantic_memory_owner(memory);
    let memory_id = semantic_memory_id(memory)?;
    let data_type = semantic_data_type_plan(&memory.data_type).canonicalized();
    let mut leaves = memory
        .leaves
        .iter()
        .map(|leaf| {
            MemoryLeafPlan::new(
                memory_id,
                None,
                leaf.semantic_path.clone(),
                semantic_data_type_plan(&leaf.data_type),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    leaves.sort_by_key(|leaf| leaf.leaf_id);
    if leaves.is_empty() {
        return Err(PlanError::new(format!(
            "semantic memory `{}` has no durable leaves",
            memory.identity.semantic_path
        )));
    }
    Ok(MemoryPlan {
        runtime_slot: slot.id,
        memory_id,
        kind,
        semantic_path: memory.identity.semantic_path.clone(),
        type_fingerprint: data_type_fingerprint(&data_type)?,
        data_type,
        initial_provenance: state_initial_provenance(slot),
        owner,
        leaves,
    })
}

fn semantic_list_memory_plan(
    program: &ErasedProgram,
    memory: &ir::SemanticMemory,
    list_slots: &[ListStorageSlot],
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
    include_draining_fields: bool,
    durable_indexed_memory: Option<&BTreeSet<ir::SemanticMemoryId>>,
) -> Result<ListMemoryPlan, PlanError> {
    let list_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => list_id,
        _ => {
            return Err(PlanError::new(format!(
                "semantic list `{}` has no list runtime backing",
                memory.identity.semantic_path
            )));
        }
    };
    let list = program
        .lists
        .iter()
        .find(|list| list.id == list_id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic list `{}` references missing list backing {}",
                memory.identity.semantic_path, list_id.0
            ))
        })?;
    let slot = list_slots
        .iter()
        .find(|slot| slot.list_id == plan_list_id(list_id))
        .ok_or_else(|| {
            PlanError::new(format!(
                "semantic list `{}` cannot resolve runtime slot {}",
                memory.identity.semantic_path, list_id.0
            ))
        })?;
    let owner = semantic_memory_owner(memory);
    let memory_id = semantic_memory_id(memory)?;
    let indexed_memory = program
        .semantic_memory
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::IndexedState {
                    list_id: Some(candidate_list),
                    ..
                } if candidate_list == list_id
            )
        })
        .collect::<Vec<_>>();
    let indexed_runtime_fields = indexed_memory
        .iter()
        .filter_map(|memory| match memory.runtime_backing {
            ir::SemanticMemoryRuntimeBacking::IndexedState {
                field_id: Some(field_id),
                ..
            } => Some(plan_field_id(field_id)),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let has_indexed_memory = !indexed_memory.is_empty();
    let semantic_list_type = semantic_data_type_plan(&memory.data_type).canonicalized();
    let DataTypePlan::List { item } = semantic_list_type.clone() else {
        return Err(PlanError::new(format!(
            "semantic list `{}` does not have a list data type",
            memory.identity.semantic_path
        )));
    };
    let semantic_row_fields = match *item {
        DataTypePlan::Record { fields, .. } => fields,
        _ => Vec::new(),
    };
    let authority_fields = program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            field.row.map(|row| row.list) == Some(list.id)
                && field.role.is_authority()
                && slot.contains_row_field(plan_field_id(field.id))
        })
        .collect::<Vec<_>>();
    let mut row_fields = Vec::new();
    if !has_indexed_memory {
        for field in &semantic_row_fields {
            let runtime_field_id =
                storage_input_field_id(program, &list.name, &field.name, authority_field_ids)
                    .filter(|field_id| slot.contains_row_field(*field_id));
            let Some(runtime_field_id) = runtime_field_id else {
                continue;
            };
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(runtime_field_id),
                format!("{}.{}", memory.identity.semantic_path, field.name),
                field.data_type.clone(),
            )?);
        }
        for field in &authority_fields {
            let runtime_field_id = plan_field_id(field.id);
            if row_fields
                .iter()
                .any(|field| field.runtime_field_id == Some(runtime_field_id))
            {
                continue;
            }
            let field_type =
                data_type_plan_from_typecheck_type(&field.flow_type.ty).ok_or_else(|| {
                    PlanError::new(format!(
                        "authoritative field `{}` has no canonical checked type",
                        field.diagnostic_path
                    ))
                })?;
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(runtime_field_id),
                list_authority_persistence_path(&memory.identity.semantic_path, field)?,
                field_type,
            )?);
        }
    } else {
        for field in authority_fields
            .iter()
            .filter(|field| !indexed_runtime_fields.contains(&plan_field_id(field.id)))
        {
            let runtime_field_id = plan_field_id(field.id);
            let field_type =
                data_type_plan_from_typecheck_type(&field.flow_type.ty).ok_or_else(|| {
                    PlanError::new(format!(
                        "authoritative field `{}` has no canonical checked type",
                        field.diagnostic_path
                    ))
                })?;
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(runtime_field_id),
                list_authority_persistence_path(&memory.identity.semantic_path, field)?,
                field_type,
            )?);
        }
        for field_memory in indexed_memory.into_iter().filter(|field| {
            (include_draining_fields || semantic_memory_is_runtime_active(program, field))
                && durable_indexed_memory.is_none_or(|durable| durable.contains(&field.id))
        }) {
            let field_id = match field_memory.runtime_backing {
                ir::SemanticMemoryRuntimeBacking::IndexedState {
                    field_id: Some(field_id),
                    ..
                } => plan_field_id(field_id),
                _ => {
                    return Err(PlanError::new(format!(
                        "indexed semantic memory `{}` has no runtime field backing",
                        field_memory.identity.semantic_path
                    )));
                }
            };
            if !slot.contains_row_field(field_id) {
                return Err(PlanError::new(format!(
                    "indexed semantic memory `{}` field {} is absent from list slot",
                    field_memory.identity.semantic_path, field_id.0
                )));
            }
            row_fields.push(MemoryLeafPlan::new(
                memory_id,
                Some(field_id),
                field_memory.identity.semantic_path.clone(),
                semantic_data_type_plan(&field_memory.data_type),
            )?);
        }
    }
    let mut runtime_field_ids = BTreeSet::new();
    if row_fields.iter().any(|field| {
        field
            .runtime_field_id
            .is_none_or(|field_id| !runtime_field_ids.insert(field_id))
    }) {
        return Err(PlanError::new(format!(
            "semantic list `{}` has duplicate or missing authoritative row field identities",
            memory.identity.semantic_path
        )));
    }
    let mut persistence_paths = BTreeMap::new();
    for field in &row_fields {
        if let Some(previous) =
            persistence_paths.insert(field.semantic_path.clone(), field.runtime_field_id)
        {
            return Err(PlanError::new(format!(
                "semantic list `{}` maps persistence path `{}` to both {previous:?} and {:?}",
                memory.identity.semantic_path, field.semantic_path, field.runtime_field_id
            )));
        }
    }
    row_fields.sort_by_key(|field| field.leaf_id);
    let row_type = DataTypePlan::Record {
        fields: row_fields
            .iter()
            .map(|field| DataTypeFieldPlan {
                name: field
                    .semantic_path
                    .rsplit_once('.')
                    .map_or_else(|| field.semantic_path.clone(), |(_, name)| name.to_owned()),
                data_type: field.data_type.clone(),
            })
            .collect(),
        open: false,
    }
    .canonicalized();
    let data_type = if has_indexed_memory || !row_fields.is_empty() {
        DataTypePlan::List {
            item: Box::new(row_type),
        }
    } else {
        semantic_list_type
    };
    ListMemoryPlan::new(
        slot.id,
        memory.identity.semantic_path.clone(),
        data_type,
        list_initial_provenance(slot),
        owner,
        list.hidden_key_type.clone(),
        list.has_generation,
        row_fields,
    )
}

fn semantic_collection_memory_plan(
    program: &ErasedProgram,
    memory: &ir::SemanticMemory,
) -> Result<CollectionMemoryPlan, PlanError> {
    let ir::SemanticMemoryRuntimeBacking::Collection { expression, owner } = memory.runtime_backing
    else {
        return Err(PlanError::new(format!(
            "semantic collection memory `{}` has no collection runtime backing",
            memory.identity.semantic_path
        )));
    };
    let kind = semantic_memory_kind(memory.identity.kind);
    if !matches!(kind, MemoryKind::Map | MemoryKind::Set) {
        return Err(PlanError::new(format!(
            "semantic collection memory `{}` has non-collection kind {kind:?}",
            memory.identity.semantic_path
        )));
    }
    let runtime_owner = plan_owner_for_static_owner(
        program,
        owner,
        &format!("collection memory `{}`", memory.identity.semantic_path),
    )?;
    let lexical_owner_rows = runtime_owner
        .ancestors
        .iter()
        .map(|ancestor| PlanCollectionOwnerRow {
            scope: ancestor.scope,
            list: ancestor.list,
        })
        .collect::<Vec<_>>();
    let structural_owner_rows = memory
        .structural_owner_rows
        .iter()
        .map(|row| PlanCollectionOwnerRow {
            scope: ScopeId(row.scope.0),
            list: plan_list_id(row.list),
        })
        .collect::<Vec<_>>();
    let runtime_owner_rows =
        merge_collection_owner_rows(&lexical_owner_rows, &structural_owner_rows);
    CollectionMemoryPlan::new(
        PlanCollectionAuthorityId(expression.as_usize()),
        kind,
        memory.identity.semantic_path.clone(),
        semantic_data_type_plan(&memory.data_type),
        semantic_memory_owner(memory),
        runtime_owner,
        runtime_owner_rows,
    )
}

fn merge_collection_owner_rows(
    lexical: &[PlanCollectionOwnerRow],
    structural: &[PlanCollectionOwnerRow],
) -> Vec<PlanCollectionOwnerRow> {
    if structural.starts_with(lexical) {
        return structural.to_vec();
    }
    if lexical.starts_with(structural) {
        return lexical.to_vec();
    }
    let overlap = (1..=lexical.len().min(structural.len()))
        .rev()
        .find(|length| lexical[lexical.len() - length..] == structural[..*length])
        .unwrap_or(0);
    lexical
        .iter()
        .chain(&structural[overlap..])
        .cloned()
        .collect()
}

fn data_type_plan_from_typecheck_type(ty: &boon_typecheck::Type) -> Option<DataTypePlan> {
    use boon_typecheck::{BytesType, Type, Variant};

    Some(
        match ty {
            Type::Text => DataTypePlan::Text,
            Type::Number => DataTypePlan::Number,
            Type::Bytes(BytesType::Dynamic) => DataTypePlan::Bytes { fixed_len: None },
            Type::Bytes(BytesType::Fixed(len)) => DataTypePlan::Bytes {
                fixed_len: Some((*len).try_into().ok()?),
            },
            Type::Bits { width } => DataTypePlan::Bits { width: *width },
            Type::VariantSet(variants) => DataTypePlan::Variant {
                variants: variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => Some(DataVariantPlan {
                            tag: tag.clone(),
                            fields: Vec::new(),
                            open: false,
                        }),
                        Variant::Tagged { tag, fields } => Some(DataVariantPlan {
                            tag: tag.clone(),
                            fields: fields
                                .fields
                                .iter()
                                .map(|(name, ty)| {
                                    Some(DataTypeFieldPlan {
                                        name: name.clone(),
                                        data_type: data_type_plan_from_typecheck_type(ty)?,
                                    })
                                })
                                .collect::<Option<Vec<_>>>()?,
                            open: fields.open,
                        }),
                    })
                    .collect::<Option<Vec<_>>>()?,
            },
            Type::Object(shape) => DataTypePlan::Record {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| {
                        Some(DataTypeFieldPlan {
                            name: name.clone(),
                            data_type: data_type_plan_from_typecheck_type(ty)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
                open: shape.open,
            },
            Type::List(item) => DataTypePlan::List {
                item: Box::new(data_type_plan_from_typecheck_type(item)?),
            },
            Type::Map { key, value } => DataTypePlan::Map {
                key: Box::new(data_type_plan_from_typecheck_type(key)?),
                value: Box::new(data_type_plan_from_typecheck_type(value)?),
            },
            Type::Set(item) => DataTypePlan::Set {
                item: Box::new(data_type_plan_from_typecheck_type(item)?),
            },
            Type::Union(members) => DataTypePlan::Union {
                members: members
                    .iter()
                    .map(data_type_plan_from_typecheck_type)
                    .collect::<Option<Vec<_>>>()?,
            },
            Type::Absent
            | Type::RenderContract
            | Type::Function { .. }
            | Type::UnresolvedShape { .. }
            | Type::Var(_)
            | Type::Unknown => return None,
        }
        .canonicalized(),
    )
}

fn append_item_field_names(ty: &boon_typecheck::Type) -> Vec<String> {
    let boon_typecheck::Type::Object(shape) = ty else {
        return vec!["value".to_owned()];
    };
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();
    for name in shape.field_order.iter().chain(shape.fields.keys()) {
        if shape.fields.contains_key(name) && seen.insert(name.as_str()) {
            names.push(name.clone());
        }
    }
    names
}

fn list_authority_persistence_path(
    list_path: &str,
    field: &ir::ErasedFieldDef,
) -> Result<String, PlanError> {
    if field.row_path.is_empty() {
        return Err(PlanError::new(format!(
            "authoritative field {} has no structural row-member path",
            field.id
        )));
    }
    Ok(format!(
        "{list_path}.@authority:{}",
        field.row_path.join("/")
    ))
}

fn plan_value_type_for_value_ref(
    program: &ErasedProgram,
    index: &ValueIndex,
    value_ref: &ValueRef,
) -> Option<PlanValueType> {
    Some(match value_ref {
        ValueRef::Field(field) => *index.field_value_type(*field)?,
        ValueRef::State(state) => {
            let path = program
                .state_cells
                .iter()
                .find(|candidate| plan_state_id(candidate.id) == *state)?
                .path
                .as_str();
            *index.state_value_type(path)?
        }
        ValueRef::StateProjection {
            state_id,
            field_path,
        } => plan_value_type_from_semantic_data_type(
            &index.state_projection_data_type(*state_id, field_path)?,
        ),
        ValueRef::Source(_) => PlanValueType::Tag,
        ValueRef::SourcePayload { source_id, field } => {
            let source = program
                .sources
                .iter()
                .find(|source| plan_source_id(source.id) == *source_id)?;
            let typed = source
                .payload_schema
                .typed_fields
                .iter()
                .find(|descriptor| source_payload_field_from_ir(&descriptor.field) == *field)
                .map(|descriptor| {
                    plan_value_type_from_semantic_data_type(&semantic_data_type_plan(
                        &descriptor.data_type,
                    ))
                });
            match typed {
                Some(PlanValueType::Unknown) => return None,
                Some(value_type) => value_type,
                None => match field {
                    SourcePayloadField::Bytes => PlanValueType::Bytes { fixed_len: None },
                    SourcePayloadField::Key => PlanValueType::Number,
                    SourcePayloadField::Address
                    | SourcePayloadField::Named(_)
                    | SourcePayloadField::Text => PlanValueType::Text,
                },
            }
        }
        ValueRef::Pulse(_) => PlanValueType::Tag,
        ValueRef::Constant(_) | ValueRef::List(_) | ValueRef::DistributedImport(_) => return None,
    })
}

fn migration_leaf_ref(
    program: &ErasedProgram,
    source: &ir::MigrationSourceLeaf,
    indexed_list_owner: Option<&MigrationListOwnerPlan>,
    data_type: DataTypePlan,
) -> Result<MigrationLeafRefPlan, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration source references missing semantic memory"))?;
    MigrationLeafRefPlan::new(
        indexed_list_owner.map_or(semantic_memory_id(memory), |owner| Ok(owner.memory_id))?,
        source.semantic_path.clone(),
        data_type,
    )
}

fn migration_indexed_list_owner(
    program: &ErasedProgram,
    memory: &ir::SemanticMemory,
) -> Result<MigrationListOwnerPlan, PlanError> {
    let list_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::IndexedState {
            list_id: Some(list_id),
            ..
        } => list_id,
        _ => {
            return Err(PlanError::new(format!(
                "indexed migration authority `{}` has no owning list backing",
                memory.identity.semantic_path
            )));
        }
    };
    let list_memory = program
        .semantic_memory
        .iter()
        .find(|candidate| {
            matches!(
                candidate.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List {
                    list_id: candidate_list_id,
                    ..
                } if candidate_list_id == list_id
            )
        })
        .ok_or_else(|| {
            PlanError::new(format!(
                "indexed migration authority `{}` cannot resolve owning list {}",
                memory.identity.semantic_path, list_id.0
            ))
        })?;
    MigrationListOwnerPlan::new(
        semantic_memory_owner(list_memory),
        list_memory.identity.semantic_path.clone(),
    )
}

fn migration_input_data_type(
    program: &ErasedProgram,
    sources: &[&ir::MigrationSourceLeaf],
    leaves: &[MigrationLeafRefPlan],
) -> Result<DataTypePlan, PlanError> {
    let first = sources
        .first()
        .ok_or_else(|| PlanError::new("migration input has no source leaves"))?;
    if sources
        .iter()
        .any(|source| source.memory_id != first.memory_id)
    {
        return Err(PlanError::new(
            "one DRAIN input cannot span multiple semantic memories",
        ));
    }
    if sources.len() == 1 {
        return Ok(leaves[0].data_type.clone());
    }
    let memory = program
        .semantic_memory
        .get(first.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration input references missing semantic memory"))?;
    Ok(semantic_data_type_plan(&memory.data_type))
}

fn durable_migration_source_list_plan(
    program: &ErasedProgram,
    source: &ir::MigrationSourceLeaf,
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<ListMemoryPlan, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration source references missing semantic memory"))?;
    if memory.identity.kind != ir::SemanticMemoryKind::ListOwner {
        return Err(PlanError::new(format!(
            "migration source `{}` is not a list authority",
            memory.identity.semantic_path
        )));
    }
    let list_id = match memory.runtime_backing {
        ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => list_id,
        _ => unreachable!("list-owner memory must have list backing"),
    };
    let list = program
        .lists
        .iter()
        .find(|list| list.id == list_id)
        .ok_or_else(|| PlanError::new("migration source list backing is absent"))?;
    let scalar_fields = ScalarFieldCatalog::new(program)?;
    let index = ValueIndex::new(program, &BTreeMap::new(), &BTreeMap::new(), &scalar_fields)?;
    let mut arena = PlanRowExpressionArena::new();
    let mut constants = Vec::new();
    let mut list_indexes = Vec::new();
    let catalog_slot = compiled_list_storage_slot(
        program,
        list,
        PlanStorageId(0),
        &index,
        &mut arena,
        &mut constants,
        &mut list_indexes,
    )?;
    semantic_list_memory_plan(
        program,
        memory,
        std::slice::from_ref(&catalog_slot),
        authority_field_ids,
        true,
        None,
    )
}

fn durable_migration_source_type(
    program: &ErasedProgram,
    source: &ir::MigrationSourceLeaf,
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<DataTypePlan, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("migration source references missing semantic memory"))?;
    if memory.identity.kind == ir::SemanticMemoryKind::ListOwner {
        return Ok(
            durable_migration_source_list_plan(program, source, authority_field_ids)?.data_type,
        );
    }
    Ok(semantic_data_type_plan(&source.data_type))
}

fn durable_migration_destination_type(
    edge: &ir::MigrationEdge,
    memory_id: MemoryId,
    memory: &[MemoryPlan],
    lists: &[ListMemoryPlan],
) -> Result<DataTypePlan, PlanError> {
    match edge.transfer_kind {
        ir::MigrationTransferKind::List => lists
            .iter()
            .find(|list| list.memory_id == memory_id)
            .map(|list| list.data_type.clone())
            .ok_or_else(|| {
                PlanError::new("migration destination list is absent from target schema")
            }),
        ir::MigrationTransferKind::Scalar | ir::MigrationTransferKind::IndexedField => {
            let target = memory
                .iter()
                .find(|target| target.memory_id == memory_id)
                .ok_or_else(|| {
                    PlanError::new("migration destination memory is absent from target schema")
                })?;
            if target.semantic_path == edge.destination.semantic_path {
                return Ok(target.data_type.clone());
            }
            target
                .leaves
                .iter()
                .find(|leaf| leaf.semantic_path == edge.destination.semantic_path)
                .map(|leaf| leaf.data_type.clone())
                .ok_or_else(|| {
                    PlanError::new("migration destination leaf is absent from target schema")
                })
        }
    }
}

fn migration_row_field_key(semantic_path: &str) -> &str {
    semantic_path
        .rsplit_once('.')
        .map_or(semantic_path, |(_, field)| field)
}

fn migration_row_fields_by_key(
    list: &ListMemoryPlan,
) -> Result<BTreeMap<String, &MemoryLeafPlan>, PlanError> {
    let mut fields = BTreeMap::new();
    for field in &list.row_fields {
        let key = migration_row_field_key(&field.semantic_path).to_owned();
        if fields.insert(key.clone(), field).is_some() {
            return Err(PlanError::new(format!(
                "whole-list migration row schema has duplicate durable field `{key}`"
            )));
        }
    }
    Ok(fields)
}

fn migration_list_row_fields(
    program: &ErasedProgram,
    edge: &ir::MigrationEdge,
    destination_memory_id: MemoryId,
    target_lists: &[ListMemoryPlan],
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<Vec<MigrationListRowFieldPlan>, PlanError> {
    if edge.transfer_kind != ir::MigrationTransferKind::List {
        return Ok(Vec::new());
    }
    if edge.transform != ir::MigrationTransform::Identity || edge.source_leaves.len() != 1 {
        return Err(PlanError::new(
            "whole-list migration must be one identity transfer",
        ));
    }
    let source =
        durable_migration_source_list_plan(program, &edge.source_leaves[0], authority_field_ids)?;
    let destination = target_lists
        .iter()
        .find(|list| list.memory_id == destination_memory_id)
        .ok_or_else(|| PlanError::new("migration destination list is absent from target schema"))?;
    if source.has_generation != destination.has_generation {
        return Err(PlanError::new(
            "whole-list identity migration changes hidden row identity schema",
        ));
    }

    let source_fields = migration_row_fields_by_key(&source)?;
    let destination_fields = migration_row_fields_by_key(destination)?;
    if destination_fields
        .keys()
        .any(|field| !source_fields.contains_key(field))
        || source_fields.keys().any(|field| {
            !destination_fields.contains_key(field) && !field.starts_with("@authority:")
        })
    {
        return Err(PlanError::new(format!(
            "whole-list identity migration from `{}` to `{}` changes durable row fields (source={:?}, destination={:?}); migrate changed row fields explicitly",
            source.semantic_path,
            destination.semantic_path,
            source_fields.keys().collect::<Vec<_>>(),
            destination_fields.keys().collect::<Vec<_>>()
        )));
    }

    source_fields
        .into_iter()
        .map(|(key, source_field)| {
            let destination = destination_fields
                .get(&key)
                .map(|destination_field| {
                    if source_field.data_type != destination_field.data_type
                        || source_field.type_fingerprint != destination_field.type_fingerprint
                    {
                        return Err(PlanError::new(format!(
                            "whole-list identity migration changes durable row field `{key}` type"
                        )));
                    }
                    MigrationDestinationPlan::new(
                        destination.memory_id,
                        destination_field.semantic_path.clone(),
                        destination_field.data_type.clone(),
                    )
                })
                .transpose()?;
            Ok(MigrationListRowFieldPlan {
                source: MigrationLeafRefPlan::new(
                    source.memory_id,
                    source_field.semantic_path.clone(),
                    source_field.data_type.clone(),
                )?,
                destination,
            })
        })
        .collect()
}

struct ExecutableMigrationExpressionLowerer<'a> {
    program: &'a ErasedProgram,
    drain_inputs: BTreeMap<boon_typecheck::CheckedExprId, MigrationInputId>,
    active_expressions: BTreeSet<ir::ExecutableExprId>,
    lexical_bindings: Vec<BTreeMap<ir::ExecutableLocalBindingId, ir::ExecutableExprId>>,
    active_lexical_bindings: BTreeSet<ir::ExecutableLocalBindingId>,
}

impl ExecutableMigrationExpressionLowerer<'_> {
    fn lower_expr(
        &mut self,
        expr_id: ir::ExecutableExprId,
    ) -> Result<MigrationExpressionPlan, PlanError> {
        let expression = self
            .program
            .executable
            .expressions
            .get(expr_id.as_usize())
            .filter(|expression| expression.id == expr_id)
            .cloned()
            .ok_or_else(|| {
                PlanError::new(format!(
                    "migration recipe references missing executable expression {expr_id}"
                ))
            })?;
        if !self.active_expressions.insert(expr_id) {
            return Err(PlanError::new(format!(
                "migration executable expression graph contains a cycle at {expr_id}"
            )));
        }
        let result = match expression.kind {
            ir::ExecutableExpressionKind::Drain { .. } => self
                .drain_inputs
                .get(&expression.checked_expr_id)
                .copied()
                .map(|input_id| MigrationExpressionPlan::Input { input_id })
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "migration executable expression {expr_id} (checked {}) references an unbound DRAIN input",
                        expression.checked_expr_id.0
                    ))
                }),
            ir::ExecutableExpressionKind::Text(value) => {
                Ok(MigrationExpressionPlan::Text { value })
            }
            ir::ExecutableExpressionKind::TextTemplate { segments } => {
                Ok(MigrationExpressionPlan::TextConcat {
                    parts: segments
                        .into_iter()
                        .map(|segment| match segment {
                            ir::ExecutableTextSegment::Static { value } => {
                                Ok(MigrationExpressionPlan::Text { value })
                            }
                            ir::ExecutableTextSegment::Dynamic { value } => self.lower_expr(value),
                        })
                        .collect::<Result<Vec<_>, PlanError>>()?,
                })
            }
            ir::ExecutableExpressionKind::Number(value) => {
                Ok(MigrationExpressionPlan::Number { value })
            }
            ir::ExecutableExpressionKind::Bits(value) => {
                Ok(MigrationExpressionPlan::Bits { value })
            }
            ir::ExecutableExpressionKind::BytesByte(value) => {
                Ok(MigrationExpressionPlan::Number {
                    value: ExactNumber::from_i64(i64::from(value)),
                })
            }
            ir::ExecutableExpressionKind::Absent => Err(PlanError::new(format!(
                "private absence at executable expression {expr_id} cannot be migration data"
            ))),
            ir::ExecutableExpressionKind::Flush { .. }
            | ir::ExecutableExpressionKind::FlushBoundary { .. } => {
                Err(PlanError::new(format!(
                    "live FLUSH control at executable expression {expr_id} cannot be migration data"
                )))
            }
            ir::ExecutableExpressionKind::Tag(tag) => {
                Ok(MigrationExpressionPlan::Variant { tag })
            }
            ir::ExecutableExpressionKind::TaggedObject { tag, fields } => {
                Ok(MigrationExpressionPlan::Tagged {
                    tag,
                    fields: self.lower_fields(&fields)?,
                })
            }
            ir::ExecutableExpressionKind::Object(fields) => {
                Ok(MigrationExpressionPlan::Record {
                    fields: self.lower_fields(&fields)?,
                })
            }
            ir::ExecutableExpressionKind::List { items, .. } => {
                Ok(MigrationExpressionPlan::List {
                    items: items
                        .into_iter()
                        .map(|item| self.lower_expr(item))
                        .collect::<Result<Vec<_>, _>>()?,
                })
            }
            ir::ExecutableExpressionKind::MapEntry { .. }
            | ir::ExecutableExpressionKind::Map { .. }
            | ir::ExecutableExpressionKind::Set { .. } => Err(PlanError::new(format!(
                "MAP/SET expression {expr_id} reached migration lowering before canonical collection migration support"
            ))),
            ir::ExecutableExpressionKind::Bytes { items, .. } => {
                Ok(MigrationExpressionPlan::Bytes {
                    items: items
                        .into_iter()
                        .map(|item| self.lower_expr(item))
                        .collect::<Result<Vec<_>, _>>()?,
                })
            }
            ir::ExecutableExpressionKind::Infix { left, op, right } => {
                Ok(MigrationExpressionPlan::Infix {
                    operator: op,
                    left: Box::new(self.lower_expr(left)?),
                    right: Box::new(self.lower_expr(right)?),
                })
            }
            ir::ExecutableExpressionKind::Call {
                callable_kind: ir::ExecutableCallableKind::Builtin,
                name,
                arguments,
                contexts,
                ..
            } => {
                if !contexts.is_empty() {
                    return Err(PlanError::new(format!(
                        "migration executable expression {expr_id} reads a call-local host context"
                    )));
                }
                self.lower_call(&name, arguments)
            }
            ir::ExecutableExpressionKind::Call {
                callable_kind: ir::ExecutableCallableKind::External,
                name,
                ..
            } => Err(PlanError::new(format!(
                "migration executable expression {expr_id} invokes external function `{name}`"
            ))),
            ir::ExecutableExpressionKind::When { input, arms } => {
                Ok(MigrationExpressionPlan::Match {
                    input: Box::new(self.lower_expr(input)?),
                    arms: arms
                        .into_iter()
                        .map(|arm| {
                            Ok(MigrationMatchArmPlan {
                                pattern: executable_select_pattern(&arm.pattern)?,
                                output: self.lower_expr(arm.output)?,
                            })
                        })
                        .collect::<Result<Vec<_>, PlanError>>()?,
                })
            }
            ir::ExecutableExpressionKind::Project { input, fields } => {
                let input = self.lower_expr(input)?;
                Ok(if fields.is_empty() {
                    input
                } else {
                    MigrationExpressionPlan::Project {
                        input: Box::new(input),
                        fields,
                    }
                })
            }
            ir::ExecutableExpressionKind::FunctionParameter {
                parameter,
                projection,
            } => {
                let index = u16::try_from(parameter.ordinal).map_err(|_| {
                    PlanError::new(format!(
                        "migration function parameter {}:{} exceeds the recipe VM index range",
                        parameter.function, parameter.ordinal
                    ))
                })?;
                let parameter = MigrationExpressionPlan::Parameter { index };
                Ok(if projection.is_empty() {
                    parameter
                } else {
                    MigrationExpressionPlan::Project {
                        input: Box::new(parameter),
                        fields: projection,
                    }
                })
            }
            ir::ExecutableExpressionKind::LocalRead {
                binding,
                declaration,
                projection,
            } => {
                let value = self
                    .lexical_bindings
                    .iter()
                    .rev()
                    .find_map(|bindings| bindings.get(&binding).copied())
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "migration expression {expr_id} reads inactive lexical binding {} for declaration {}",
                            binding.0, declaration.0
                        ))
                    })?;
                if !self.active_lexical_bindings.insert(binding) {
                    return Err(PlanError::new(format!(
                        "migration lexical binding {} for declaration {} forms a value cycle",
                        binding.0, declaration.0
                    )));
                }
                let lowered = self.lower_expr(value);
                self.active_lexical_bindings.remove(&binding);
                let lowered = lowered?;
                Ok(if projection.is_empty() {
                    lowered
                } else {
                    MigrationExpressionPlan::Project {
                        input: Box::new(lowered),
                        fields: projection,
                    }
                })
            }
            ir::ExecutableExpressionKind::Block { bindings, result } => {
                self.lexical_bindings.push(
                    bindings
                        .into_iter()
                        .map(|binding| (binding.id, binding.value))
                        .collect(),
                );
                let lowered = self.lower_expr(result);
                self.lexical_bindings.pop();
                lowered
            }
            ir::ExecutableExpressionKind::CanonicalRead { path, projection, .. } => {
                let projected = if projection.is_empty() {
                    path
                } else {
                    format!("{path}.{}", projection.join("."))
                };
                Err(PlanError::new(format!(
                    "migration executable expression {expr_id} reads `{projected}` outside its DRAIN inputs or function parameters"
                )))
            }
            ir::ExecutableExpressionKind::ExternalRead { canonical_path } => {
                Err(PlanError::new(format!(
                    "migration executable expression {expr_id} reads external value `{canonical_path}`"
                )))
            }
            ir::ExecutableExpressionKind::Delimiter => Err(PlanError::new(format!(
                "migration executable expression {expr_id} retains an unbound pipeline delimiter"
            ))),
            ir::ExecutableExpressionKind::Source { .. }
            | ir::ExecutableExpressionKind::ElementState { .. }
            | ir::ExecutableExpressionKind::Materialize { .. }
            | ir::ExecutableExpressionKind::Draining { .. }
            | ir::ExecutableExpressionKind::Hold { .. }
            | ir::ExecutableExpressionKind::Latest { .. }
            | ir::ExecutableExpressionKind::Then { .. }
            | ir::ExecutableExpressionKind::MatchArm { .. }
            | ir::ExecutableExpressionKind::MaterializationLocal { .. } => {
                Err(PlanError::new(format!(
                    "executable expression {expr_id} is not legal in a target-neutral migration recipe"
                )))
            }
        };
        self.active_expressions.remove(&expr_id);
        result
    }

    fn lower_fields(
        &mut self,
        fields: &[ir::ExecutableRecordField],
    ) -> Result<Vec<MigrationObjectFieldPlan>, PlanError> {
        fields
            .iter()
            .map(|field| {
                if field.spread {
                    return Err(PlanError::new(
                        "migration record spread is not a closed target-neutral recipe",
                    ));
                }
                Ok(MigrationObjectFieldPlan {
                    name: field.name.clone(),
                    value: self.lower_expr(field.value)?,
                })
            })
            .collect()
    }

    fn lower_call(
        &mut self,
        function: &str,
        mut args: Vec<ir::ExecutableCallArgument>,
    ) -> Result<MigrationExpressionPlan, PlanError> {
        if !migration_call_is_supported(function) {
            return Err(PlanError::new(format!(
                "pure migration call `{function}` is outside the target-neutral recipe VM"
            )));
        }
        args.sort_by_key(|argument| argument.ordinal);
        let piped = args
            .iter()
            .filter(|argument| argument.from_pipe)
            .collect::<Vec<_>>();
        if piped.len() > 1 {
            return Err(PlanError::new(format!(
                "migration call `{function}` has more than one executable pipeline input"
            )));
        }
        let input = piped
            .first()
            .map(|argument| self.lower_expr(argument.value))
            .transpose()?
            .map(Box::new);
        let arguments = args
            .into_iter()
            .filter(|argument| !argument.from_pipe)
            .map(|argument| {
                Ok(MigrationCallArgumentPlan {
                    name: argument.name,
                    value: MigrationArgumentValuePlan::Expression {
                        value: Box::new(self.lower_expr(argument.value)?),
                    },
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        Ok(MigrationExpressionPlan::Call {
            function: function.to_owned(),
            input,
            arguments,
        })
    }
}

fn checked_expr_id(
    expression: ir::ExprId,
    context: &str,
) -> Result<boon_typecheck::CheckedExprId, PlanError> {
    u32::try_from(expression.as_usize())
        .map(boon_typecheck::CheckedExprId)
        .map_err(|_| {
            PlanError::new(format!(
                "{context} expression {} exceeds the checked-expression ID range",
                expression.as_usize()
            ))
        })
}

fn migration_executable_children(
    program: &ErasedProgram,
    kind: &ir::ExecutableExpressionKind,
    context: &str,
) -> Result<Vec<ir::ExecutableExprId>, PlanError> {
    let ir::ExecutableExpressionKind::Materialize { materialization } = kind else {
        return Ok(ir::executable_expression_children(kind));
    };
    let materialization = program
        .materializations
        .get(*materialization)
        .filter(|candidate| candidate.id == *materialization)
        .ok_or_else(|| {
            PlanError::new(format!(
                "{context} references missing contextual materialization {materialization}"
            ))
        })?;
    Ok(materialization.expression_roots())
}

fn migration_transform_executable_root(
    program: &ErasedProgram,
    edge: &ir::MigrationEdge,
    destination_memory: &ir::SemanticMemory,
    checked_root: ir::ExprId,
) -> Result<ir::ExecutableExprId, PlanError> {
    let state = state_for_semantic_memory(program, destination_memory)?;
    let executable_state_id = state.executable_state_id.ok_or_else(|| {
        PlanError::new(format!(
            "migration destination `{}` state {} has no ExecutableStateId",
            edge.destination.semantic_path, state.id
        ))
    })?;
    let executable_state = program
        .executable
        .states
        .get(executable_state_id.as_usize())
        .filter(|candidate| candidate.id == executable_state_id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "migration destination `{}` references missing executable state {}",
                edge.destination.semantic_path, executable_state_id
            ))
        })?;
    let bindings = program
        .scope_index
        .bindings
        .iter()
        .filter(|binding| {
            matches!(
                binding.target,
                ir::ErasedBindingTarget::State {
                    executable,
                    runtime,
                    ..
                } if executable == executable_state_id && runtime == state.id
            )
        })
        .collect::<Vec<_>>();
    let [binding] = bindings.as_slice() else {
        return Err(PlanError::new(format!(
            "migration destination `{}` state {} requires one executable storage binding, found {}",
            edge.destination.semantic_path,
            state.id,
            bindings.len()
        )));
    };
    if binding.producer != executable_state.expression {
        return Err(PlanError::new(format!(
            "migration destination `{}` storage binding {} producer {} does not match executable state {} producer {}",
            edge.destination.semantic_path,
            binding.id,
            binding.producer,
            executable_state_id,
            executable_state.expression
        )));
    }
    let initializer = executable_state.initial;
    if edge.destination.semantic_path == destination_memory.identity.semantic_path {
        return Ok(initializer);
    }

    let checked_root = checked_expr_id(checked_root, "nested migration transform")?;
    let mut matches = Vec::new();
    let mut pending = vec![initializer];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = program
            .executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "migration destination `{}` initializer reaches missing executable expression {}",
                    edge.destination.semantic_path, expression_id
                ))
            })?;
        if expression.checked_expr_id == checked_root {
            matches.push(expression_id);
        }
        pending.extend(migration_executable_children(
            program,
            &expression.kind,
            "migration destination initializer",
        )?);
    }
    match matches.as_slice() {
        [root] => Ok(*root),
        [] => Err(PlanError::new(format!(
            "nested migration transform `{}` checked expression {} has no executable root inside destination initializer {}; MigrationTransform is missing an exact ExecutableExprId",
            edge.destination.semantic_path, checked_root.0, initializer
        ))),
        many => Err(PlanError::new(format!(
            "nested migration transform `{}` checked expression {} has ambiguous executable roots {:?} inside destination initializer {}",
            edge.destination.semantic_path,
            checked_root.0,
            many.iter().map(|root| root.0).collect::<Vec<_>>(),
            initializer
        ))),
    }
}

fn migration_recipe(
    program: &ErasedProgram,
    target_memory: &[MemoryPlan],
    target_lists: &[ListMemoryPlan],
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Result<Option<MigrationRecipePlan>, PlanError> {
    if program.migration_edges.is_empty() {
        return Ok(None);
    }
    let mut transfers = Vec::with_capacity(program.migration_edges.len());
    for edge in &program.migration_edges {
        let destination_memory = program
            .semantic_memory
            .get(edge.destination.memory_id.as_usize())
            .ok_or_else(|| {
                PlanError::new("migration destination references missing semantic memory")
            })?;
        if !semantic_memory_is_active(destination_memory) {
            return Err(PlanError::new(format!(
                "migration destination `{}` is not active target authority",
                destination_memory.identity.semantic_path
            )));
        }
        let indexed_list_owner = if edge.transfer_kind == ir::MigrationTransferKind::IndexedField {
            let owner = migration_indexed_list_owner(program, destination_memory)?;
            for source in &edge.source_leaves {
                let source_memory = program
                    .semantic_memory
                    .get(source.memory_id.as_usize())
                    .ok_or_else(|| {
                        PlanError::new(
                            "indexed migration source references missing semantic memory",
                        )
                    })?;
                if migration_indexed_list_owner(program, source_memory)? != owner {
                    return Err(PlanError::new(format!(
                        "indexed migration `{}` crosses stable list owners",
                        edge.destination.semantic_path
                    )));
                }
            }
            Some(owner)
        } else {
            None
        };
        let mut grouped_sources =
            BTreeMap::<boon_typecheck::CheckedExprId, Vec<&ir::MigrationSourceLeaf>>::new();
        for source in &edge.source_leaves {
            grouped_sources
                .entry(checked_expr_id(
                    source.drain_expr_id,
                    "migration DRAIN source",
                )?)
                .or_default()
                .push(source);
        }
        let mut drain_inputs = BTreeMap::new();
        let mut inputs = Vec::with_capacity(grouped_sources.len());
        for (drain_checked_expr_id, sources) in grouped_sources {
            let leaves = sources
                .iter()
                .map(|source| {
                    migration_leaf_ref(
                        program,
                        source,
                        indexed_list_owner.as_ref(),
                        durable_migration_source_type(program, source, authority_field_ids)?,
                    )
                })
                .collect::<Result<Vec<_>, PlanError>>()?;
            let input = MigrationInputPlan::new(
                leaves.clone(),
                migration_input_data_type(program, &sources, &leaves)?,
            )?;
            drain_inputs.insert(drain_checked_expr_id, input.input_id);
            inputs.push(input);
        }
        let transform = match &edge.transform {
            ir::MigrationTransform::Identity => {
                let input_id = inputs
                    .first()
                    .filter(|_| inputs.len() == 1)
                    .map(|input| input.input_id)
                    .ok_or_else(|| {
                        PlanError::new("identity migration must have exactly one DRAIN input")
                    })?;
                MigrationTransformPlan::Identity { input_id }
            }
            ir::MigrationTransform::PureExpression {
                expression_root, ..
            } => {
                let root = migration_transform_executable_root(
                    program,
                    edge,
                    destination_memory,
                    *expression_root,
                )?;
                let mut lowerer = ExecutableMigrationExpressionLowerer {
                    program,
                    drain_inputs,
                    active_expressions: BTreeSet::new(),
                    lexical_bindings: Vec::new(),
                    active_lexical_bindings: BTreeSet::new(),
                };
                MigrationTransformPlan::Expression {
                    root: lowerer.lower_expr(root)?,
                }
            }
        };
        let semantic_destination_memory_id = semantic_memory_id(destination_memory)?;
        let list_row_fields = migration_list_row_fields(
            program,
            edge,
            semantic_destination_memory_id,
            target_lists,
            authority_field_ids,
        )?;
        let destination_memory_id = indexed_list_owner
            .as_ref()
            .map_or(semantic_destination_memory_id, |owner| owner.memory_id);
        transfers.push(MigrationTransferPlan {
            transfer_kind: match edge.transfer_kind {
                ir::MigrationTransferKind::Scalar => MigrationTransferKindPlan::Scalar,
                ir::MigrationTransferKind::List => MigrationTransferKindPlan::List,
                ir::MigrationTransferKind::IndexedField => {
                    MigrationTransferKindPlan::IndexedRowField
                }
            },
            indexed_list_owner,
            list_row_fields,
            inputs,
            destination: MigrationDestinationPlan::new(
                destination_memory_id,
                edge.destination.semantic_path.clone(),
                durable_migration_destination_type(
                    edge,
                    semantic_destination_memory_id,
                    target_memory,
                    target_lists,
                )?,
            )?,
            transform,
        });
    }
    Ok(Some(MigrationRecipePlan::new(transfers)?))
}

fn validate_predecessor_binding(
    application: &ApplicationPlan,
    target_schema_version: u64,
    predecessor: &MigrationPredecessorBinding,
) -> Result<(), PlanError> {
    let canonical_application = ApplicationPlan::new(predecessor.application.identity.clone())?;
    if predecessor.application != canonical_application {
        return Err(PlanError::new(
            "migration predecessor application identity hash is invalid",
        ));
    }
    if predecessor.application.identity != application.identity {
        return Err(PlanError::new(
            "migration predecessor belongs to a different application identity",
        ));
    }
    predecessor
        .persistence
        .validate_for_application(&predecessor.application)?;
    if predecessor.persistence.schema_version >= target_schema_version {
        return Err(PlanError::new(format!(
            "migration predecessor schema version {} must precede target version {target_schema_version}",
            predecessor.persistence.schema_version
        )));
    }
    Ok(())
}

fn memory_kind_at_semantic_path(
    memory: &[MemoryPlan],
    lists: &[ListMemoryPlan],
    owner: &MemoryOwnerPath,
    semantic_path: &str,
) -> Option<MemoryKind> {
    memory
        .iter()
        .find(|candidate| candidate.owner == *owner && candidate.semantic_path == semantic_path)
        .map(|candidate| candidate.kind)
        .or_else(|| {
            lists
                .iter()
                .find(|candidate| {
                    candidate.owner == *owner && candidate.semantic_path == semantic_path
                })
                .map(|_| MemoryKind::List)
        })
}

fn prove_compatible_without_drain(
    predecessor: &PersistencePlan,
    target_memory: &[MemoryPlan],
    target_lists: &[ListMemoryPlan],
) -> Result<(), PlanError> {
    for source in &predecessor.memory {
        if let Some(target_kind) = memory_kind_at_semantic_path(
            target_memory,
            target_lists,
            &source.owner,
            &source.semantic_path,
        ) && target_kind != source.kind
        {
            return Err(PlanError::new(format!(
                "persistent memory `{}` changes kind without DRAIN",
                source.semantic_path
            )));
        }
        let Some(target) = target_memory
            .iter()
            .find(|target| target.memory_id == source.memory_id)
        else {
            continue;
        };
        if target.kind != source.kind
            || target.owner != source.owner
            || target.semantic_path != source.semantic_path
            || target.type_fingerprint != source.type_fingerprint
            || target.data_type != source.data_type
        {
            return Err(PlanError::new(format!(
                "persistent memory `{}` changes type or identity without DRAIN",
                source.semantic_path
            )));
        }
        for source_leaf in &source.leaves {
            if let Some(target_leaf) = target
                .leaves
                .iter()
                .find(|target_leaf| target_leaf.leaf_id == source_leaf.leaf_id)
                && (target_leaf.semantic_path != source_leaf.semantic_path
                    || target_leaf.type_fingerprint != source_leaf.type_fingerprint
                    || target_leaf.data_type != source_leaf.data_type)
            {
                return Err(PlanError::new(format!(
                    "persistent leaf `{}` changes type without DRAIN",
                    source_leaf.semantic_path
                )));
            }
        }
    }

    for source in &predecessor.lists {
        if let Some(target_kind) = memory_kind_at_semantic_path(
            target_memory,
            target_lists,
            &source.owner,
            &source.semantic_path,
        ) && target_kind != MemoryKind::List
        {
            return Err(PlanError::new(format!(
                "persistent list `{}` changes kind without DRAIN",
                source.semantic_path
            )));
        }
        let Some(target) = target_lists
            .iter()
            .find(|target| target.memory_id == source.memory_id)
        else {
            continue;
        };
        if target.owner != source.owner
            || target.semantic_path != source.semantic_path
            || target.hidden_key_type != source.hidden_key_type
            || target.has_generation != source.has_generation
        {
            return Err(PlanError::new(format!(
                "persistent list `{}` changes row identity without DRAIN (owner {:?} -> {:?}, semantic path `{}` -> `{}`, hidden key `{}` -> `{}`, generation {} -> {})",
                source.semantic_path,
                source.owner,
                target.owner,
                source.semantic_path,
                target.semantic_path,
                source.hidden_key_type,
                target.hidden_key_type,
                source.has_generation,
                target.has_generation,
            )));
        }
        if source.row_fields.is_empty()
            && target.row_fields.is_empty()
            && (target.type_fingerprint != source.type_fingerprint
                || target.data_type != source.data_type)
        {
            return Err(PlanError::new(format!(
                "persistent list `{}` changes item type without DRAIN",
                source.semantic_path
            )));
        }
        for source_leaf in &source.row_fields {
            if let Some(target_leaf) = target
                .row_fields
                .iter()
                .find(|target_leaf| target_leaf.leaf_id == source_leaf.leaf_id)
                && (target_leaf.semantic_path != source_leaf.semantic_path
                    || target_leaf.type_fingerprint != source_leaf.type_fingerprint
                    || target_leaf.data_type != source_leaf.data_type)
            {
                return Err(PlanError::new(format!(
                    "persistent row field `{}` changes type without DRAIN",
                    source_leaf.semantic_path
                )));
            }
        }
    }
    Ok(())
}

fn source_contains_migration_leaf(
    predecessor: &PersistencePlan,
    leaf: &MigrationLeafRefPlan,
) -> bool {
    predecessor.memory.iter().any(|memory| {
        memory.memory_id == leaf.memory_id
            && memory.leaves.iter().any(|candidate| {
                candidate.leaf_id == leaf.leaf_id
                    && candidate.semantic_path == leaf.semantic_path
                    && candidate.type_fingerprint == leaf.type_fingerprint
                    && candidate.data_type == leaf.data_type
            })
    }) || predecessor.lists.iter().any(|list| {
        list.memory_id == leaf.memory_id
            && ((MemoryLeafId::from_memory_path(list.memory_id, &list.semantic_path).is_ok_and(
                |leaf_id| {
                    leaf_id == leaf.leaf_id
                        && list.semantic_path == leaf.semantic_path
                        && list.type_fingerprint == leaf.type_fingerprint
                        && list.data_type == leaf.data_type
                },
            )) || list.row_fields.iter().any(|candidate| {
                candidate.leaf_id == leaf.leaf_id
                    && candidate.semantic_path == leaf.semantic_path
                    && candidate.type_fingerprint == leaf.type_fingerprint
                    && candidate.data_type == leaf.data_type
            }))
    })
}

fn migration_source_candidates(
    predecessor: &PersistencePlan,
    leaf: &MigrationLeafRefPlan,
) -> Vec<String> {
    predecessor
        .memory
        .iter()
        .flat_map(|memory| &memory.leaves)
        .chain(predecessor.lists.iter().flat_map(|list| &list.row_fields))
        .filter(|candidate| candidate.semantic_path == leaf.semantic_path)
        .map(|candidate| {
            format!(
                "leaf_id_match={}, type={:?}",
                candidate.leaf_id == leaf.leaf_id,
                candidate.data_type
            )
        })
        .chain(
            predecessor
                .lists
                .iter()
                .filter(|list| list.semantic_path == leaf.semantic_path)
                .map(|list| {
                    format!(
                        "list_memory_id_match={}, type={:?}",
                        list.memory_id == leaf.memory_id,
                        list.data_type
                    )
                }),
        )
        .collect()
}

fn contains_migration_list_owner(lists: &[ListMemoryPlan], owner: &MigrationListOwnerPlan) -> bool {
    lists.iter().any(|list| {
        list.memory_id == owner.memory_id
            && list.semantic_path == owner.semantic_path
            && list.owner == owner.owner
    })
}

fn prove_recipe_sources_exist(
    predecessor: &PersistencePlan,
    recipe: &MigrationRecipePlan,
) -> Result<(), PlanError> {
    for transfer in &recipe.transfers {
        if let Some(owner) = &transfer.indexed_list_owner
            && !contains_migration_list_owner(&predecessor.lists, owner)
        {
            return Err(PlanError::new(format!(
                "indexed migration list owner `{}` is absent in predecessor schema {}",
                owner.semantic_path, predecessor.schema_version
            )));
        }
        for leaf in transfer.inputs.iter().flat_map(|input| &input.leaves) {
            if !source_contains_migration_leaf(predecessor, leaf) {
                let candidates = migration_source_candidates(predecessor, leaf);
                return Err(PlanError::new(format!(
                    "migration source `{}` is absent or has a different type in predecessor schema {}; expected {:?}, candidates: {}",
                    leaf.semantic_path,
                    predecessor.schema_version,
                    leaf.data_type,
                    if candidates.is_empty() {
                        "none".to_owned()
                    } else {
                        candidates.join("; ")
                    }
                )));
            }
        }
    }
    Ok(())
}

fn prove_recipe_destinations_exist(
    memory: &[MemoryPlan],
    lists: &[ListMemoryPlan],
    recipe: &MigrationRecipePlan,
) -> Result<(), PlanError> {
    for transfer in &recipe.transfers {
        if let Some(owner) = &transfer.indexed_list_owner
            && !contains_migration_list_owner(lists, owner)
        {
            return Err(PlanError::new(format!(
                "indexed migration list owner `{}` is absent in target schema",
                owner.semantic_path
            )));
        }
        let destination = &transfer.destination;
        let present = match transfer.transfer_kind {
            MigrationTransferKindPlan::Scalar => memory.iter().any(|candidate| {
                candidate.memory_id == destination.memory_id
                    && candidate.kind == MemoryKind::Scalar
                    && ((candidate.semantic_path == destination.semantic_path
                        && candidate.type_fingerprint == destination.type_fingerprint
                        && candidate.data_type == destination.data_type)
                        || candidate.leaves.iter().any(|leaf| {
                            leaf.leaf_id == destination.leaf_id
                                && leaf.semantic_path == destination.semantic_path
                                && leaf.type_fingerprint == destination.type_fingerprint
                                && leaf.data_type == destination.data_type
                        }))
            }),
            MigrationTransferKindPlan::IndexedRowField => lists.iter().any(|list| {
                list.memory_id == destination.memory_id
                    && list.row_fields.iter().any(|leaf| {
                        leaf.leaf_id == destination.leaf_id
                            && leaf.semantic_path == destination.semantic_path
                            && leaf.type_fingerprint == destination.type_fingerprint
                            && leaf.data_type == destination.data_type
                    })
            }),
            MigrationTransferKindPlan::List => lists.iter().any(|candidate| {
                candidate.memory_id == destination.memory_id
                    && candidate.semantic_path == destination.semantic_path
                    && candidate.type_fingerprint == destination.type_fingerprint
                    && candidate.data_type == destination.data_type
            }),
        };
        if !present {
            return Err(PlanError::new(format!(
                "migration destination `{}` is absent or has a different type in target schema",
                destination.semantic_path
            )));
        }
    }
    Ok(())
}

fn merge_migration_catalog(
    predecessors: &[MigrationPredecessorBinding],
    current_recipe: Option<&MigrationRecipePlan>,
    target_schema_version: u64,
) -> Result<(Vec<MigrationRecipePlan>, Vec<MigrationEdgePlan>), PlanError> {
    let mut recipes = BTreeMap::<MigrationRecipeId, MigrationRecipePlan>::new();
    let mut edges = BTreeMap::<MigrationEdgeId, MigrationEdgePlan>::new();
    for predecessor in predecessors {
        for recipe in &predecessor.persistence.migration_recipes {
            if let Some(existing) = recipes.insert(recipe.migration_recipe_id, recipe.clone())
                && existing != *recipe
            {
                return Err(PlanError::new(
                    "predecessor catalogs disagree on migration recipe content",
                ));
            }
        }
        for edge in &predecessor.persistence.migration_edges {
            if let Some(existing) = edges.insert(edge.migration_edge_id, edge.clone())
                && existing != *edge
            {
                return Err(PlanError::new(
                    "predecessor catalogs disagree on migration edge content",
                ));
            }
        }
    }
    if let Some(recipe) = current_recipe {
        if let Some(existing) = recipes.insert(recipe.migration_recipe_id, recipe.clone())
            && existing != *recipe
        {
            return Err(PlanError::new(
                "current migration recipe ID conflicts with inherited content",
            ));
        }
        for predecessor in predecessors {
            let edge = MigrationEdgePlan::new(
                predecessor.source_schema_version(),
                target_schema_version,
                predecessor.source_schema_hash(),
                recipe.migration_recipe_id,
            )?;
            if let Some(existing) = edges.insert(edge.migration_edge_id, edge.clone())
                && existing != edge
            {
                return Err(PlanError::new(
                    "current predecessor binding conflicts with inherited edge content",
                ));
            }
        }
    }
    Ok((
        recipes.into_values().collect(),
        edges.into_values().collect(),
    ))
}

fn persistence_plan(
    program: &ErasedProgram,
    application: &ApplicationPlan,
    schema_version: u64,
    scalar_slots: &[ScalarStorageSlot],
    list_slots: &[ListStorageSlot],
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
    transient_effect_result_targets: &BTreeSet<ValueRef>,
    transient_producer_states: &BTreeSet<StateId>,
    transient_producer_lists: &BTreeSet<ListId>,
    effect_outbox: Vec<EffectOutboxSchema>,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<PersistencePlan, PlanError> {
    let mut memory = Vec::new();
    let mut lists = Vec::new();
    let mut collections = Vec::new();
    let durable_semantic_memory = program
        .semantic_memory
        .iter()
        .filter(|memory| {
            semantic_memory_is_runtime_active(program, memory)
                && !semantic_memory_is_transient_effect_result(
                    memory,
                    transient_effect_result_targets,
                )
                && !match memory.runtime_backing {
                    ir::SemanticMemoryRuntimeBacking::RootState { state_id, .. }
                    | ir::SemanticMemoryRuntimeBacking::IndexedState { state_id, .. } => {
                        transient_producer_states.contains(&plan_state_id(state_id))
                            || program
                                .state_cells
                                .get(state_id.as_usize())
                                .filter(|state| state.id == state_id)
                                .is_some_and(|state| {
                                    matches!(
                                        state.lifetime,
                                        ir::StateCellLifetimeV1::ActivationLocal { .. }
                                    )
                                })
                    }
                    ir::SemanticMemoryRuntimeBacking::List { list_id, .. } => {
                        transient_producer_lists.contains(&plan_list_id(list_id))
                    }
                    ir::SemanticMemoryRuntimeBacking::Collection { .. } => false,
                }
        })
        .collect::<Vec<_>>();
    let durable_indexed_memory = durable_semantic_memory
        .iter()
        .filter(|memory| memory.identity.kind == ir::SemanticMemoryKind::IndexedField)
        .map(|memory| memory.id)
        .collect::<BTreeSet<_>>();
    for semantic_memory in durable_semantic_memory {
        match semantic_memory.identity.kind {
            ir::SemanticMemoryKind::RootScalar | ir::SemanticMemoryKind::IndexedField => {
                memory.push(semantic_scalar_memory_plan(
                    program,
                    semantic_memory,
                    scalar_slots,
                )?);
            }
            ir::SemanticMemoryKind::ListOwner => lists.push(semantic_list_memory_plan(
                program,
                semantic_memory,
                list_slots,
                authority_field_ids,
                false,
                Some(&durable_indexed_memory),
            )?),
            ir::SemanticMemoryKind::Map | ir::SemanticMemoryKind::Set => {
                collections.push(semantic_collection_memory_plan(program, semantic_memory)?);
            }
        }
    }
    for predecessor in migration_predecessors {
        validate_predecessor_binding(application, schema_version, predecessor)?;
    }
    let explicit_recipe = migration_recipe(program, &memory, &lists, authority_field_ids)?;
    if let Some(recipe) = &explicit_recipe {
        prove_recipe_destinations_exist(&memory, &lists, recipe)?;
        for predecessor in migration_predecessors {
            prove_recipe_sources_exist(&predecessor.persistence, recipe)?;
        }
    } else {
        for predecessor in migration_predecessors {
            prove_compatible_without_drain(&predecessor.persistence, &memory, &lists)?;
        }
    }
    let compatible_recipe = if explicit_recipe.is_none() && !migration_predecessors.is_empty() {
        Some(MigrationRecipePlan::new(Vec::new())?)
    } else {
        None
    };
    let current_recipe = explicit_recipe.as_ref().or(compatible_recipe.as_ref());
    let current_migration_recipe_id = current_recipe.map(|recipe| recipe.migration_recipe_id);
    let (migration_recipes, migration_edges) =
        merge_migration_catalog(migration_predecessors, current_recipe, schema_version)?;
    PersistencePlan::new_with_migrations_effect_outbox_and_collections(
        application,
        schema_version,
        memory,
        lists,
        collections,
        effect_outbox,
        migration_recipes,
        current_migration_recipe_id,
        migration_edges,
    )
}

fn list_mutation_owns_authority(program: &ErasedProgram, mutation: &ir::ListMutation) -> bool {
    match mutation.kind {
        ListMutationKind::Append { .. } => true,
        ListMutationKind::Remove {
            owner, row_local, ..
        } => program.materializations.iter().any(|materialization| {
            materialization.owner == owner
                && materialization.row_local == row_local
                && materialization.source_list_id == Some(mutation.list_id)
                && materialization.target_list_id == Some(mutation.list_id)
        }),
    }
}

pub fn compile_typed_program(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: &ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
) -> Result<MachinePlan, PlanError> {
    compile_typed_program_with_distributed_context(
        program,
        target_profile,
        program_role,
        application_identity,
        schema_version,
        migration_predecessors,
        &DistributedMachineContext::default(),
    )
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DistributedMachineContext {
    pub expression_refs: BTreeMap<ir::ExecutableExprId, ValueRef>,
    pub path_refs: BTreeMap<String, ValueRef>,
    pub producer_function_instances: Vec<ProducerFunctionInstancePlan>,
    pub synthetic_source_routes: Vec<SourceRoute>,
    pub invocation_result_sources: BTreeSet<SourceId>,
    pub endpoint: Option<DistributedEndpointPlan>,
}

pub(crate) fn compile_typed_program_with_distributed_context(
    program: &ErasedProgram,
    target_profile: TargetProfile,
    program_role: ProgramRole,
    application_identity: &ApplicationIdentity,
    schema_version: u64,
    migration_predecessors: &[MigrationPredecessorBinding],
    distributed: &DistributedMachineContext,
) -> Result<MachinePlan, PlanError> {
    let effects = effect_contracts(program)?;
    let mut effect_outbox = effect_outbox_schemas(&effects)?;
    let authority_field_ids = list_authority_field_ids(program);
    let scalar_fields = ScalarFieldCatalog::new(program)?;
    let index = ValueIndex::new(
        program,
        &distributed.expression_refs,
        &distributed.path_refs,
        &scalar_fields,
    )?;
    let mut next_op = 0usize;
    let mut unresolved_refs = BTreeSet::new();

    let mut source_routes = program
        .sources
        .iter()
        .enumerate()
        .map(|(route_id, source)| {
            Ok(SourceRoute {
                id: PlanSourceRouteId(route_id),
                source_id: plan_source_id(source.id),
                owner: plan_source_owner(program, source)?,
                path: source.path.clone(),
                scoped: source.scoped,
                scope_id: plan_scope_id(source.scope_id),
                row_projections: plan_source_row_projections(program, source)?,
                interval_ms: source.interval_ms,
                payload_schema: source_payload_schema_from_ir(program, source)?,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    for synthetic in &distributed.synthetic_source_routes {
        if let Some(existing) = source_routes
            .iter_mut()
            .find(|route| route.source_id == synthetic.source_id || route.path == synthetic.path)
        {
            if existing.source_id != synthetic.source_id || existing.path != synthetic.path {
                return Err(PlanError::new(
                    "distributed event source collides with a different source route",
                ));
            }
            let route_id = existing.id;
            *existing = synthetic.clone();
            existing.id = route_id;
            continue;
        }
        let mut synthetic = synthetic.clone();
        synthetic.id = PlanSourceRouteId(source_routes.len());
        source_routes.push(synthetic);
    }

    let mut row_expressions = PlanRowExpressionArena::new();
    let mut constants = Vec::new();
    let activations = program
        .activations()
        .iter()
        .map(|activation| {
            Ok(PlanActivation {
                id: plan_activation_id(activation.id),
                owner: plan_owner_for_static_owner(
                    program,
                    activation.static_owner,
                    &format!("activation {}", activation.id),
                )?,
                states: activation
                    .states
                    .iter()
                    .copied()
                    .map(plan_state_id)
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let migration_storage_defaults = program
        .state_cells
        .iter()
        .map(|state| migration_storage_default(program, state))
        .collect::<Vec<_>>();

    let mut scalar_slots = Vec::with_capacity(program.state_cells.len());
    for state in program.state_cells.iter() {
        let state_index = state.id.as_usize();
        let slot_id = scalar_slots.len();
        let migration_default = migration_storage_defaults[state_index].as_ref();
        let initializer = match migration_default {
            Some(default) if default.indexed_edge.is_some() => ScalarInitializerPlan::Expression {
                expression: migration_indexed_default_expression(
                    program,
                    state,
                    default
                        .indexed_edge
                        .as_ref()
                        .expect("matched indexed migration edge"),
                    &index,
                    &mut row_expressions,
                    &mut constants,
                )?,
            },
            Some(default) => ScalarInitializerPlan::Constant {
                constant_id: push_plan_constant(
                    &mut constants,
                    default.constant.clone().ok_or_else(|| {
                        PlanError::new(format!(
                            "state `{}` migration default has neither expression nor constant",
                            state.path
                        ))
                    })?,
                ),
            },
            None => {
                if let Some(value) = constant_state_initial_expression_value(program, state) {
                    ScalarInitializerPlan::Constant {
                        constant_id: push_plan_constant(&mut constants, value),
                    }
                } else {
                    ScalarInitializerPlan::Expression {
                        expression: initial_state_expression(
                            program,
                            state,
                            &index,
                            &mut row_expressions,
                            &mut constants,
                        )?,
                    }
                }
            }
        };
        let value_type = migration_default.map_or_else(
            || state_executable_value_type(program, state),
            |default| default.value_type,
        );
        if !plan_value_type_is_concrete(value_type) {
            let executable_state = state
                .executable_state_id
                .and_then(|id| program.executable.states.get(id.as_usize()))
                .filter(|candidate| Some(candidate.id) == state.executable_state_id);
            let initializer = executable_state.and_then(|state| {
                program
                    .executable
                    .expressions
                    .get(state.initial.as_usize())
                    .filter(|candidate| candidate.id == state.initial)
            });
            return Err(PlanError::new(format!(
                "state `{}` ({:?}) has no concrete executable value type; executable state {:?}, initializer {:?}",
                state.path, state.id, executable_state, initializer
            )));
        }
        scalar_slots.push(ScalarStorageSlot {
            id: PlanStorageId(slot_id),
            state_id: plan_state_id(state.id),
            owner: plan_state_owner(program, state)?,
            value_type,
            scope_id: plan_scope_id(state.scope_id),
            indexed: state.indexed,
            indexed_field_id: plan_indexed_state_field(program, state)?,
            lifetime: match state.lifetime {
                ir::StateCellLifetimeV1::Persistent => PlanStateLifetime::Persistent,
                ir::StateCellLifetimeV1::ActivationLocal { then_expression } => {
                    let activation = program
                        .activations()
                        .iter()
                        .find(|activation| activation.then_expression == then_expression)
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "activation-local state `{}` has no activation for THEN {}",
                                state.path, then_expression
                            ))
                        })?;
                    if !activation.states.contains(&state.id) {
                        return Err(PlanError::new(format!(
                            "activation {} does not own activation-local state `{}`",
                            activation.id, state.path
                        )));
                    }
                    PlanStateLifetime::ActivationLocal {
                        activation: plan_activation_id(activation.id),
                    }
                }
            },
            initializer,
        });
    }

    let mut list_indexes = Vec::new();
    let list_slot_offset = scalar_slots.len();
    let list_slots = program
        .lists
        .iter()
        .enumerate()
        .map(|(slot_index, list)| {
            compiled_list_storage_slot(
                program,
                list,
                PlanStorageId(list_slot_offset + slot_index),
                &index,
                &mut row_expressions,
                &mut constants,
                &mut list_indexes,
            )
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let byte_bank_offset = scalar_slots.len() + list_slots.len();
    let byte_banks = scalar_slots
        .iter()
        .filter_map(|slot| match slot.value_type {
            PlanValueType::Bytes {
                fixed_len: Some(fixed_len),
            } => Some(ByteStorageBank {
                id: PlanStorageId(byte_bank_offset),
                state_storage_id: slot.id,
                state_id: slot.state_id,
                scope_id: slot.scope_id,
                indexed: slot.indexed,
                fixed_len,
                capacity: byte_bank_capacity_hint(slot, &list_slots),
            }),
            _ => None,
        })
        .enumerate()
        .map(|(bank_index, mut bank)| {
            bank.id = PlanStorageId(byte_bank_offset + bank_index);
            bank
        })
        .collect::<Vec<_>>();
    let byte_bank_storage_count = byte_banks.len();

    let source_ops = source_routes
        .iter()
        .map(|route| {
            op(
                &mut next_op,
                PlanOpKind::SourceRoute,
                Vec::new(),
                Some(ValueRef::Source(route.source_id)),
                false,
                0,
            )
        })
        .collect::<Vec<_>>();

    let projection_owned_outputs = program
        .list_projections
        .iter()
        .filter_map(|projection| index.resolve(&projection.target))
        .collect::<BTreeSet<_>>();
    let mutation_owned_lists = program
        .list_mutations
        .iter()
        .filter(|mutation| list_mutation_owns_authority(program, mutation))
        .map(|mutation| plan_list_id(mutation.list_id))
        .collect::<BTreeSet<_>>();
    let mut transient_effect_schedules = BTreeMap::<usize, &ir::HostEffectSchedule>::new();
    for (schedule_index, schedule) in program.host_effect_schedules.iter().enumerate() {
        let Some(derived_index) = schedule.transient_result else {
            continue;
        };
        if schedule.id != schedule_index || !schedule.state_update_arms.is_empty() {
            return Err(PlanError::new(format!(
                "transient host-effect schedule {} has a noncanonical identity or retained state-update arms",
                schedule.id
            )));
        }
        let derived = program.derived_values.get(derived_index).ok_or_else(|| {
            PlanError::new(format!(
                "transient host-effect schedule {} references missing derived value index {derived_index}",
                schedule.id
            ))
        })?;
        if !executable_expression_reaches(program, derived.producer, schedule.expression) {
            return Err(PlanError::new(format!(
                "transient host-effect schedule {} does not belong to derived value `{}`",
                schedule.id, derived.path
            )));
        }
        if let Some(previous) = transient_effect_schedules.insert(derived_index, schedule) {
            return Err(PlanError::new(format!(
                "derived value `{}` has multiple transient host-effect schedules {} and {}",
                derived.path, previous.id, schedule.id
            )));
        }
    }
    let mut derived_ops = Vec::new();
    let mut materialized_row_outputs = BTreeSet::new();
    let mut split_pulse_outputs = BTreeMap::<usize, Vec<ValueRef>>::new();
    for (derived_index, derived) in program.derived_values.iter().enumerate() {
        let derived_output = derived_output_ref(derived, &scalar_fields)?;
        if projection_owned_outputs.contains(&derived_output) {
            continue;
        }
        if let ValueRef::List(target_list) = derived_output
            && mutation_owned_lists.contains(&target_list)
        {
            let state_fields = materialized_state_fields(program, target_list)?;
            for field in state_dependent_materialized_row_fields(
                program,
                target_list,
                &state_fields,
                None,
                &index,
                &mut row_expressions,
                &mut constants,
                &mut list_indexes,
                true,
            )? {
                if !materialized_row_outputs.insert(field.output) {
                    return Err(PlanError::new(format!(
                        "materialized row field {} has more than one demand-current computation",
                        field.output.0
                    )));
                }
                derived_ops.push(op(
                    &mut next_op,
                    PlanOpKind::DerivedValue {
                        derived_kind: PlanDerivedKind::Pure,
                        startup_recompute: false,
                        expression: Some(PlanDerivedExpression::MaterializedRowField {
                            local: field.local,
                            expression: field.expression,
                        }),
                        materialization: None,
                    },
                    field.inputs,
                    Some(ValueRef::Field(field.output)),
                    true,
                    0,
                ));
            }
            continue;
        }
        if derived.kind == DerivedValueKind::ListView && derived.materialized_list_id.is_none() {
            return Err(PlanError::new(format!(
                "derived list view `{}` has no typed materialized ListId",
                derived.path
            )));
        }
        let event_owned_sources = derived
            .trigger_arms
            .iter()
            .filter_map(|arm| event_cause_path(program, arm.cause))
            .collect::<BTreeSet<_>>();
        let mut inputs = Vec::new();
        let mut unresolved = derived
            .sources
            .iter()
            .filter(|path| !event_owned_sources.contains(path.as_str()))
            .map(|path| resolve_path(&index, path, &mut inputs, &mut unresolved_refs))
            .sum();
        let transient_effect = transient_effect_schedules.get(&derived_index).copied();
        let mut expression = if let Some(schedule) = transient_effect {
            let contract = builtin_effect_contract(&schedule.operation)?.ok_or_else(|| {
                PlanError::new(format!(
                    "transient host effect `{}` has no centralized effect contract",
                    schedule.operation
                ))
            })?;
            if !matches!(
                contract.replay,
                EffectReplay::ReadOnly | EffectReplay::ProcessScoped
            ) {
                return Err(PlanError::new(format!(
                    "direct host effect `{}` for `{}` is not transient-safe ({:?})",
                    schedule.operation, derived.path, contract.replay
                )));
            }
            if !matches!(derived_output, ValueRef::Field(_)) {
                return Err(PlanError::new(format!(
                    "direct host effect result `{}` must lower to one scalar FieldId, found {derived_output:?}",
                    derived.path
                )));
            }
            inputs.clear();
            unresolved = 0;
            Some(PlanDerivedExpression::RowExpression {
                expression: row_expressions.intern(
                    PlanRowExpressionNode::TransientEffectResult {
                        invocation_id: EffectInvocationId::from_result_owner(
                            contract.effect_id,
                            &derived.path,
                        )?,
                    },
                )?,
            })
        } else {
            derived_expression_for_value(
                program,
                derived,
                &index,
                distributed,
                &mut row_expressions,
                &mut constants,
                &mut inputs,
                &mut list_indexes,
                &mut unresolved_refs,
            )?
        };
        let mut materialization = None;
        if unresolved == 0
            && let Some(expression) = expression.as_mut()
        {
            lower_bounded_list_access(
                program,
                &index,
                &mut row_expressions,
                &constants,
                expression,
                &mut list_indexes,
            )?;
        }
        if exact_reconstructable_list_literal(
            &derived_output,
            expression.as_ref(),
            &list_slots,
            &constants,
            &row_expressions,
        )? {
            continue;
        }
        let mut authority_map_was_split = false;
        if let Some(target_list) = derived_materialized_list_id(program, derived)
            && let Some(mut inner) = expression.take()
        {
            let state_fields = materialized_state_fields(program, target_list)?;
            let state_dependent_fields = if unresolved == 0 {
                state_dependent_materialized_row_fields(
                    program,
                    target_list,
                    &state_fields,
                    Some(&inner),
                    &index,
                    &mut row_expressions,
                    &mut constants,
                    &mut list_indexes,
                    false,
                )?
            } else {
                Vec::new()
            };
            let deferred_names = state_dependent_fields
                .iter()
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            let deferred_outputs = state_dependent_fields
                .iter()
                .map(|field| field.output)
                .collect::<BTreeSet<_>>();
            if !deferred_outputs.is_empty() {
                split_pulse_outputs.insert(
                    derived_index,
                    deferred_outputs
                        .iter()
                        .copied()
                        .map(ValueRef::Field)
                        .collect(),
                );
            }
            let omitted_fields = state_fields
                .keys()
                .cloned()
                .chain(materialized_resource_fields(program, target_list))
                .chain(deferred_names.iter().cloned())
                .collect::<BTreeSet<_>>();
            strip_materialized_non_value_fields(&mut inner, &omitted_fields, &mut row_expressions)?;
            if !state_dependent_fields.is_empty()
                && detach_startup_pulse_materialization(program, &mut inner, &row_expressions)?
            {
                inputs.retain(|input| !matches!(input, ValueRef::Pulse(_)));
            }
            for state in state_fields.values() {
                if derived_expression_reads_state(&inner, *state, &row_expressions)? {
                    return Err(PlanError::new(format!(
                        "materialized list {} reads indexed state {} before its keyed row exists",
                        target_list.0, state.0
                    )));
                }
                inputs.retain(|input| {
                    !matches!(
                        input,
                        ValueRef::State(candidate) if candidate == state
                    ) && !matches!(
                        input,
                        ValueRef::StateProjection { state_id, .. } if state_id == state
                    )
                });
            }
            for field in state_dependent_fields {
                if !materialized_row_outputs.insert(field.output) {
                    return Err(PlanError::new(format!(
                        "materialized row field {} has more than one demand-current computation",
                        field.output.0
                    )));
                }
                derived_ops.push(op(
                    &mut next_op,
                    PlanOpKind::DerivedValue {
                        derived_kind: PlanDerivedKind::Pure,
                        startup_recompute: false,
                        expression: Some(PlanDerivedExpression::MaterializedRowField {
                            local: field.local,
                            expression: field.expression,
                        }),
                        materialization: None,
                    },
                    field.inputs,
                    Some(ValueRef::Field(field.output)),
                    true,
                    0,
                ));
            }
            if unresolved == 0
                && let Some(fields) = take_authority_mapped_row_fields(
                    program,
                    target_list,
                    &mut inner,
                    &mut row_expressions,
                )?
            {
                for field in fields {
                    if !materialized_row_outputs.insert(field.output) {
                        return Err(PlanError::new(format!(
                            "materialized row field {} has more than one demand-current computation",
                            field.output.0
                        )));
                    }
                    derived_ops.push(op(
                        &mut next_op,
                        PlanOpKind::DerivedValue {
                            derived_kind: PlanDerivedKind::Pure,
                            startup_recompute: false,
                            expression: Some(PlanDerivedExpression::MaterializedRowField {
                                local: field.local,
                                expression: field.expression,
                            }),
                            materialization: None,
                        },
                        field.inputs,
                        Some(ValueRef::Field(field.output)),
                        true,
                        0,
                    ));
                }
                authority_map_was_split = true;
            }
            if authority_map_was_split {
                continue;
            }
            let mut field_names = BTreeSet::new();
            collect_materialized_list_field_names(&inner, &mut field_names, &row_expressions)?;
            let mut fields = materialized_output_fields(program, target_list, &state_fields)?;
            fields.retain(|name, field| {
                !deferred_names.contains(name) && !deferred_outputs.contains(field)
            });
            for name in field_names {
                if !fields.contains_key(&name) {
                    unresolved += 1;
                    unresolved_refs.insert(format!(
                        "materialized list {} field `{name}`",
                        target_list.0
                    ));
                }
            }
            let mut row_field_copies = materialized_list_row_field_copies(
                program,
                target_list,
                &inner,
                &row_expressions,
                &list_indexes,
            )?;
            row_field_copies.retain(|copy| !deferred_outputs.contains(&copy.target_field));
            let value_list_authorities =
                ExecutableRowLowerer::<'_>::materialized_value_list_authorities(
                    program,
                    target_list,
                    &inner,
                    &fields,
                    &row_expressions,
                )?;
            let authority_source_list =
                if derived_expression_reads_authority_list(&inner, target_list, &row_expressions)?
                    || !value_list_authorities.is_empty()
                {
                    Some(target_list)
                } else {
                    let target = program
                        .lists
                        .iter()
                        .find(|list| plan_list_id(list.id) == target_list)
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "materialized target list {} is absent",
                                target_list.0
                            ))
                        })?;
                    let migration_source = whole_list_migration_source(program, target)?
                        .map(|source| plan_list_id(source.id));
                    let mut row_sources = BTreeSet::new();
                    collect_materialized_iteration_sources(
                        &inner,
                        &row_expressions,
                        &list_indexes,
                        &mut row_sources,
                    )?;
                    migration_source.filter(|source| row_sources.contains(source))
                };
            materialization = Some(PlanListMaterialization {
                target_list,
                authority_source_list,
                fields,
                row_field_copies,
                value_list_authorities,
            });
            expression = Some(inner);
        }
        if state_only_authority_map_is_noop(
            expression.as_ref(),
            materialization.as_ref(),
            &row_expressions,
        )? {
            continue;
        }
        derived_ops.push(op(
            &mut next_op,
            PlanOpKind::DerivedValue {
                derived_kind: if transient_effect.is_some() {
                    PlanDerivedKind::Pure
                } else {
                    plan_derived_kind_from_ir(&derived.kind)
                },
                startup_recompute: transient_effect.is_some() || derived.startup_recompute,
                expression,
                materialization,
            },
            inputs,
            Some(derived_output),
            derived.indexed,
            unresolved,
        ));
    }
    let mut update_ops = program
        .state_update_arms
        .iter()
        .map(|arm| {
            let state = program
                .state_cells
                .get(arm.state.as_usize())
                .filter(|state| state.id == arm.state)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "state update arm references missing state {}",
                        arm.state.0
                    ))
                })?;
            let gate = program
                .executable
                .expressions
                .get(arm.gate_expression_id.as_usize())
                .filter(|expression| {
                    expression.id == arm.gate_expression_id
                        && expression.checked_expr_id == arm.gate_checked_expr_id
                        && expression.owner == arm.owner
                })
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "state update `{}` has stale gate {}",
                        state.path, arm.gate_expression_id
                    ))
                })?;
            let output_expression = program
                .executable
                .expressions
                .get(arm.output_expression_id.as_usize())
                .filter(|expression| expression.id == arm.output_expression_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "state update `{}` gate {} has missing output {}",
                        state.path, gate.id, arm.output_expression_id
                    ))
                })?;
            let _ = output_expression;
            let (trigger, cause_path) = event_cause_value_ref(program, arm.cause)?;
            let output = Some(ValueRef::State(plan_state_id(state.id)));
            let active_state = state.executable_state_id.ok_or_else(|| {
                PlanError::new(format!(
                    "state update `{}` has no executable state identity",
                    state.path
                ))
            })?;
            let mut inputs = vec![trigger.clone()];
            let exact_effect = exact_host_effect_expression(program, arm.output_expression_id)?;
            let (value, effect) = if let Some(effect_expression) = exact_effect {
                let HostEffectPlanParts {
                    operation,
                    gate: effect_gate,
                    intent_expressions,
                } = exact_host_effect_plan_parts(
                    program,
                    &index,
                    &mut row_expressions,
                    &mut constants,
                    &mut inputs,
                    &trigger,
                    Some(active_state),
                    arm.output_expression_id,
                    effect_expression,
                )?;
                let effect = effect_invocation_for_operation(
                    &operation,
                    &state.path,
                    effect_gate,
                    intent_expressions,
                    output.clone(),
                    plan_owner_for_static_owner(
                        program,
                        arm.owner,
                        &format!("state update `{}` from `{cause_path}`", state.path),
                    )?,
                )?;
                let result_transform = exact_host_effect_result_transform(
                    program,
                    &index,
                    &mut row_expressions,
                    &mut constants,
                    &mut inputs,
                    &trigger,
                    Some(active_state),
                    arm.output_expression_id,
                    effect_expression,
                )?;
                (result_transform, Some(effect))
            } else {
                let value = ExecutableRowLowerer::new(
                    program,
                    &index,
                    &mut row_expressions,
                    &mut constants,
                    &mut inputs,
                )
                .with_list_indexes(&mut list_indexes)
                .with_event_trigger(&trigger)
                .with_state_update(active_state)
                .lower(arm.output_expression_id)
                .map_err(|error| {
                    PlanError::new(format!(
                        "state update `{}` from `{cause_path}` failed exact lowering: {error}",
                        state.path
                    ))
                })?;
                (Some(value), None)
            };
            Ok(op(
                &mut next_op,
                PlanOpKind::StateUpdate {
                    trigger,
                    value,
                    effect,
                },
                unique_value_refs(inputs),
                output,
                state.indexed,
                0,
            ))
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    for (derived_index, schedule) in &transient_effect_schedules {
        let derived = program.derived_values.get(*derived_index).ok_or_else(|| {
            PlanError::new(format!(
                "transient host-effect schedule {} lost derived value index {derived_index}",
                schedule.id
            ))
        })?;
        let output = Some(derived_output_ref(derived, &scalar_fields)?);
        let contract = effects
            .iter()
            .find(|contract| contract.host_operation == schedule.operation)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "transient host effect `{}` for `{}` has no compiled contract",
                    schedule.operation, derived.path
                ))
            })?;
        if !matches!(
            contract.replay,
            EffectReplay::ReadOnly | EffectReplay::ProcessScoped
        ) {
            return Err(PlanError::new(format!(
                "transient host effect `{}` for `{}` has non-transient replay policy {:?}",
                schedule.operation, derived.path, contract.replay
            )));
        }
        let mut scheduled_arm_count = 0usize;
        for arm in &derived.trigger_arms {
            if !executable_expression_reaches(
                program,
                arm.output_expression_id,
                schedule.expression,
            ) {
                continue;
            }
            scheduled_arm_count += 1;
            let (trigger, cause_path) = event_cause_value_ref(program, arm.cause)?;
            let mut inputs = vec![trigger.clone()];
            let HostEffectPlanParts {
                operation,
                gate,
                intent_expressions,
            } = exact_host_effect_plan_parts(
                program,
                &index,
                &mut row_expressions,
                &mut constants,
                &mut inputs,
                &trigger,
                None,
                arm.output_expression_id,
                schedule.expression,
            )?;
            if operation != schedule.operation {
                return Err(PlanError::new(format!(
                    "transient host-effect schedule {} changed operation from `{}` to `{operation}`",
                    schedule.id, schedule.operation
                )));
            }
            let effect = effect_invocation_for_operation(
                &operation,
                &derived.path,
                gate,
                intent_expressions,
                output.clone(),
                plan_owner_for_static_owner(
                    program,
                    schedule.owner,
                    &format!(
                        "transient effect result `{}` from `{cause_path}`",
                        derived.path
                    ),
                )?,
            )?;
            let result_transform = exact_host_effect_result_transform(
                program,
                &index,
                &mut row_expressions,
                &mut constants,
                &mut inputs,
                &trigger,
                None,
                arm.output_expression_id,
                schedule.expression,
            )?;
            update_ops.push(op(
                &mut next_op,
                PlanOpKind::EffectUpdate {
                    trigger,
                    result_transform,
                    effect,
                },
                unique_value_refs(inputs),
                output.clone(),
                derived.indexed,
                0,
            ));
        }
        if scheduled_arm_count == 0 {
            return Err(PlanError::new(format!(
                "transient host-effect schedule {} for `{}` has no event-owned executable arm",
                schedule.id, derived.path
            )));
        }
        if derived.trigger_arms.iter().any(|arm| {
            !executable_expression_reaches(program, arm.output_expression_id, schedule.expression)
        }) {
            return Err(PlanError::new(format!(
                "transient host-effect result `{}` mixes effect and non-effect event arms",
                derived.path
            )));
        }
    }
    let list_mutation_ops = program
        .list_mutations
        .iter()
        .filter(|list_mutation| list_mutation_owns_authority(program, list_mutation))
        .map(|list_mutation| {
            let (trigger, cause_path) = event_cause_value_ref(program, list_mutation.cause)?;
            let mut inputs = vec![trigger.clone()];
            let output = Some(ValueRef::List(plan_list_id(list_mutation.list_id)));
            let mutation = match &list_mutation.kind {
                ListMutationKind::Append { gate, item } => {
                    let append_item_expression = *item;
                    let gate = ExecutableRowLowerer::new(
                        program,
                        &index,
                        &mut row_expressions,
                        &mut constants,
                        &mut inputs,
                    )
                    .with_event_trigger(&trigger)
                    .lower(*gate)
                    .map_err(|error| {
                        PlanError::new(format!(
                            "list {} append from `{cause_path}` failed exact gate lowering: {error}",
                            list_mutation.list_id
                        ))
                    })?;
                    let mut item_lowerer = ExecutableRowLowerer::new(
                        program,
                        &index,
                        &mut row_expressions,
                        &mut constants,
                        &mut inputs,
                    )
                    .with_event_trigger(&trigger);
                    let item = item_lowerer
                    .lower(*item)
                    .map_err(|error| {
                        PlanError::new(format!(
                            "list {} append from `{cause_path}` failed exact item lowering: {error}",
                            list_mutation.list_id
                        ))
                    })?;
                    let source_list = item_lowerer.direct_row_source(item)?;
                    let target_list = program
                        .lists
                        .iter()
                        .find(|list| list.id == list_mutation.list_id)
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "list append references missing list {}",
                                list_mutation.list_id
                            ))
                        })?;
                    let append_item_type = program
                        .executable
                        .expressions
                        .get(append_item_expression.as_usize())
                        .filter(|expression| expression.id == append_item_expression)
                        .map(|expression| &expression.flow_type.ty)
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "list {} append references missing item expression {}",
                                list_mutation.list_id, append_item_expression.0
                            ))
                        })?;
                    let fields = append_item_field_names(append_item_type)
                        .into_iter()
                        .map(|name| {
                            let field_id = storage_input_field_id(
                                program,
                                &target_list.name,
                                &name,
                                &authority_field_ids,
                            )
                            .ok_or_else(|| {
                                PlanError::new(format!(
                                    "list {} append item field `{name}` has no exact authority identity",
                                    list_mutation.list_id
                                ))
                            })?;
                            Ok(PlanListAppendField { name, field_id })
                        })
                        .collect::<Result<Vec<_>, PlanError>>()?;
                    let row_field_copies = source_list
                        .into_iter()
                        .flat_map(|source_list| {
                            fields.iter().map(move |target| {
                                row_input_field_id_for_list_id(program, source_list, &target.name)
                                    .map(|source_field| PlanMaterializedRowFieldCopy {
                                        source_list,
                                        source_field,
                                        target_field: target.field_id,
                                    })
                                    .ok_or_else(|| {
                                        PlanError::new(format!(
                                            "list {} append row from ListId {} has no exact source field `{}`",
                                            list_mutation.list_id, source_list.0, target.name
                                        ))
                                    })
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    PlanListMutation::Append(PlanListAppend {
                        site: list_mutation.site.as_usize(),
                        ordinal: list_mutation.ordinal,
                        owner: plan_event_cause_owner(program, list_mutation.cause)?,
                        trigger,
                        gate,
                        item,
                        fields,
                        row_field_copies,
                    })
                }
                ListMutationKind::Remove {
                    gate,
                    owner,
                    row_local,
                    predicate,
                    remove_when,
                } => {
                    let owner = PlanStaticOwnerId(owner.as_usize());
                    let gate = ExecutableRowLowerer::new(
                        program,
                        &index,
                        &mut row_expressions,
                        &mut constants,
                        &mut inputs,
                    )
                    .with_event_trigger(&trigger)
                    .with_materialization_owner(owner)
                    .lower(*gate)
                    .map_err(|error| {
                        PlanError::new(format!(
                            "list {} removal from `{cause_path}` failed exact gate lowering: {error}",
                            list_mutation.list_id
                        ))
                    })?;
                    let predicate = ExecutableRowLowerer::new(
                        program,
                        &index,
                        &mut row_expressions,
                        &mut constants,
                        &mut inputs,
                    )
                    .with_event_trigger(&trigger)
                    .with_materialization_owner(owner)
                    .lower(*predicate)
                    .map_err(|error| {
                        PlanError::new(format!(
                            "list {} removal from `{cause_path}` failed exact predicate lowering: {error}",
                            list_mutation.list_id
                        ))
                    })?;
                    PlanListMutation::Remove(PlanListRemove {
                        site: list_mutation.site.as_usize(),
                        ordinal: list_mutation.ordinal,
                        owner: plan_event_cause_owner(program, list_mutation.cause)?,
                        trigger,
                        gate,
                        local_owner: owner,
                        row_local: PlanLocalId(row_local.0 as usize),
                        predicate,
                        remove_when: *remove_when,
                    })
                }
            };
            Ok(op(
                &mut next_op,
                PlanOpKind::ListMutation { mutation },
                unique_value_refs(inputs),
                output,
                true,
                0,
            ))
        })
        .collect::<Result<Vec<_>, PlanError>>()?;

    let list_projection_ops = program
        .list_projections
        .iter()
        .map(|projection| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            let source_ref = match index.resolve(&projection.list) {
                Some(value_ref) => {
                    inputs.push(value_ref.clone());
                    Some(value_ref)
                }
                None => {
                    unresolved += 1;
                    unresolved_refs.insert(projection.list.clone());
                    None
                }
            };
            let source_list = match source_ref {
                Some(ValueRef::List(list_id)) => Some(list_id),
                _ => None,
            };
            let output = index.resolve(&projection.target);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(projection.target.clone());
            }
            let projection_plan = match (&projection.kind, source_ref.clone(), source_list) {
                (
                    ListProjectionKind::Chunk { size: Some(size) },
                    Some(ValueRef::List(source_list)),
                    _,
                ) => Some(PlanListProjection::Chunk {
                    source_list,
                    size: *size,
                }),
                (ListProjectionKind::Chunk { size: Some(size) }, Some(source), _) => {
                    Some(PlanListProjection::ChunkValue {
                        source,
                        size: *size,
                    })
                }
                (ListProjectionKind::Chunk { size: None, .. }, _, _) => {
                    unresolved += 1;
                    unresolved_refs.insert(format!("{}.List/chunk.size", projection.target));
                    None
                }
                _ => None,
            };
            op(
                &mut next_op,
                PlanOpKind::ListProjection {
                    projection: projection_plan.unwrap_or_else(|| PlanListProjection::Unknown {
                        summary: projection.target.clone(),
                    }),
                },
                unique_value_refs(inputs),
                output,
                true,
                unresolved,
            )
        })
        .collect::<Vec<_>>();

    let dependency_ops = program
        .dependencies
        .iter()
        .map(|dependency| {
            let mut inputs = Vec::new();
            let mut unresolved = 0usize;
            unresolved += resolve_path(&index, &dependency.from, &mut inputs, &mut unresolved_refs);
            let output = index.resolve(&dependency.to);
            if output.is_none() {
                unresolved += 1;
                unresolved_refs.insert(dependency.to.clone());
            }
            op(
                &mut next_op,
                PlanOpKind::DependencyEdge,
                unique_value_refs(inputs),
                output,
                dependency.indexed,
                unresolved,
            )
        })
        .collect::<Vec<_>>();

    let mut regions = vec![
        region(0, RegionKind::SourceRouting, source_ops),
        region(1, RegionKind::DerivedEvaluation, derived_ops),
        region(2, RegionKind::StateUpdates, update_ops),
        region(3, RegionKind::ListMutations, list_mutation_ops),
        region(4, RegionKind::ListProjections, list_projection_ops),
        region(5, RegionKind::DependencyEdges, dependency_ops),
    ];
    retarget_invocation_result_operations(
        program,
        &row_expressions,
        &mut regions,
        &distributed.invocation_result_sources,
    )?;
    for operation in regions.iter_mut().flat_map(|region| &mut region.ops) {
        operation.synchronize_expression_inputs(&row_expressions)?;
    }
    let root_computation_fields = root_computation_fields(&regions);
    let mut document = super::document_plan_backend::compile_document_plan(
        program,
        &index,
        &root_computation_fields,
        &mut row_expressions,
        &mut constants,
        &distributed.expression_refs,
        &distributed.path_refs,
    )?;
    if let Some(document) = document.as_mut() {
        for expression in &mut document.expressions {
            if let DocumentExprOp::RuntimeExpression { expression, .. } = &mut expression.op {
                *expression = lower_bounded_row_access(
                    program,
                    &index,
                    &mut row_expressions,
                    &constants,
                    *expression,
                    &mut list_indexes,
                )?;
            }
        }
    }
    let update_op_ids = regions
        .iter()
        .find(|region| region.kind == RegionKind::StateUpdates)
        .map(|region| {
            region
                .ops
                .iter()
                .filter_map(|op| matches!(op.kind, PlanOpKind::StateUpdate { .. }).then_some(op.id))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if update_op_ids.len() != program.state_update_arms.len() {
        return Err(PlanError::new(format!(
            "MachinePlan owns {} state-update ops for {} erased state-update arms",
            update_op_ids.len(),
            program.state_update_arms.len()
        )));
    }
    let mutation_ops_by_site = regions
        .iter()
        .find(|region| region.kind == RegionKind::ListMutations)
        .into_iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::ListMutation {
                mutation: PlanListMutation::Append(append),
            } => Some((append.site, op.id)),
            PlanOpKind::ListMutation {
                mutation: PlanListMutation::Remove(remove),
            } => Some((remove.site, op.id)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let derived_ops_by_output = regions
        .iter()
        .find(|region| region.kind == RegionKind::DerivedEvaluation)
        .into_iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| op.output.clone().map(|output| (output, op.id)))
        .collect::<BTreeMap<_, _>>();
    let mut pulse_batches = Vec::with_capacity(program.pulse_batches().len());
    for batch in program.pulse_batches() {
        let pulse_ref = ValueRef::Pulse(plan_pulse_batch_id(batch.id));
        let owner = plan_event_cause_owner(program, ir::EventCause::Pulse(batch.id))?;
        let mut count_inputs = Vec::new();
        let mut count_lowerer = ExecutableRowLowerer::new(
            program,
            &index,
            &mut row_expressions,
            &mut constants,
            &mut count_inputs,
        )
        .with_list_indexes(&mut list_indexes);
        if let Some(state) = batch.state {
            count_lowerer = count_lowerer.with_state_initializer(state);
        }
        let count = count_lowerer
            .lower(batch.count_expression)
            .map_err(|error| {
                PlanError::new(format!(
                    "pulse batch {} count failed exact lowering: {error}",
                    batch.id
                ))
            })?;
        let start = match &batch.start {
            ir::PulseStart::Startup => PlanPulseStart::Startup,
            ir::PulseStart::Triggered { arms } => {
                if arms.is_empty() {
                    return Err(PlanError::new(format!(
                        "pulse batch {} has an empty triggered start",
                        batch.id
                    )));
                }
                let arms = arms
                    .iter()
                    .map(|arm| {
                        if matches!(arm.cause, ir::EventCause::Pulse(_)) {
                            return Err(PlanError::new(format!(
                                "pulse batch {} has a pulse-recursive start",
                                batch.id
                            )));
                        }
                        let (trigger, cause_path) =
                            event_cause_value_ref(program, arm.cause)?;
                        let mut inputs = vec![trigger.clone()];
                        let mut gate_lowerer = ExecutableRowLowerer::new(
                            program,
                            &index,
                            &mut row_expressions,
                            &mut constants,
                            &mut inputs,
                        )
                        .with_list_indexes(&mut list_indexes)
                        .with_event_trigger(&trigger);
                        if let Some(state) = batch.state {
                            gate_lowerer = gate_lowerer.with_state_initializer(state);
                        }
                        let gate = gate_lowerer.lower(arm.gate_expression_id).map_err(|error| {
                            PlanError::new(format!(
                                "pulse batch {} start from `{cause_path}` failed exact gate lowering: {error}",
                                batch.id
                            ))
                        })?;
                        Ok(PlanPulseStartArm { trigger, gate })
                    })
                    .collect::<Result<Vec<_>, PlanError>>()?;
                PlanPulseStart::Triggered { arms }
            }
        };
        let state_update_ops = batch
            .state_update_arms
            .iter()
            .map(|arm| {
                program
                    .state_update_arms
                    .iter()
                    .position(|candidate| candidate == arm)
                    .and_then(|index| update_op_ids.get(index).copied())
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "pulse batch {} lost state-update arm for state {}",
                            batch.id, arm.state
                        ))
                    })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let list_mutation_ops = batch
            .list_mutations
            .iter()
            .filter(|mutation| list_mutation_owns_authority(program, mutation))
            .map(|mutation| {
                mutation_ops_by_site
                    .get(&mutation.site.as_usize())
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "pulse batch {} lost list-mutation site {}",
                            batch.id, mutation.site
                        ))
                    })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let mut derived_ops = Vec::new();
        for derived_index in &batch.derived_value_indices {
            let derived = program.derived_values.get(*derived_index).ok_or_else(|| {
                PlanError::new(format!(
                    "pulse batch {} references missing derived value index {}",
                    batch.id, derived_index
                ))
            })?;
            let outputs = if let Some(outputs) = split_pulse_outputs.get(derived_index) {
                outputs.clone()
            } else {
                vec![index.resolve(&derived.path).ok_or_else(|| {
                    PlanError::new(format!(
                        "pulse batch {} derived value `{}` has no typed output",
                        batch.id, derived.path
                    ))
                })?]
            };
            for output in outputs {
                let op_id = derived_ops_by_output.get(&output).copied().ok_or_else(|| {
                    PlanError::new(format!(
                        "pulse batch {} derived value `{}` output {output:?} has no MachinePlan op",
                        batch.id, derived.path
                    ))
                })?;
                if !derived_ops.contains(&op_id) {
                    derived_ops.push(op_id);
                }
            }
        }
        let mut emission_routes = batch
            .emission_routes
            .iter()
            .filter(|_| !derived_ops.is_empty())
            .map(|route| {
                let filter = match route.filter {
                    ir::PulseEmissionFilter::Passthrough => PlanPulseEmissionFilter::Passthrough,
                    ir::PulseEmissionFilter::Skip {
                        count_expression, ..
                    } => {
                        let mut inputs = Vec::new();
                        let mut count_lowerer = ExecutableRowLowerer::new(
                            program,
                            &index,
                            &mut row_expressions,
                            &mut constants,
                            &mut inputs,
                        )
                        .with_list_indexes(&mut list_indexes);
                        if let Some(state) = batch.state {
                            count_lowerer = count_lowerer.with_state_initializer(state);
                        }
                        let count = count_lowerer.lower(count_expression).map_err(|error| {
                            PlanError::new(format!(
                                "pulse batch {} Stream/skip count failed exact lowering: {error}",
                                batch.id
                            ))
                        })?;
                        PlanPulseEmissionFilter::Skip { count }
                    }
                };
                Ok(PlanPulseEmissionRoute {
                    derived_ops: derived_ops.clone(),
                    filter,
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        if emission_routes.is_empty() && !derived_ops.is_empty() {
            emission_routes.push(PlanPulseEmissionRoute {
                derived_ops: derived_ops.clone(),
                filter: PlanPulseEmissionFilter::Passthrough,
            });
        }
        if !state_update_ops.is_empty()
            && !state_update_ops.iter().all(|op| {
                regions
                    .iter()
                    .flat_map(|region| &region.ops)
                    .find(|candidate| candidate.id == *op)
                    .is_some_and(|candidate| {
                        matches!(
                            &candidate.kind,
                            PlanOpKind::StateUpdate { trigger, .. } if trigger == &pulse_ref
                        )
                    })
            })
        {
            return Err(PlanError::new(format!(
                "pulse batch {} state-update inventory contains a non-pulse op",
                batch.id
            )));
        }
        let fusion = match &batch.fusion {
            ir::PulseFusionEligibility::VerifiedActivationLocalRecurrence {
                activation,
                state,
                state_update_arm_index,
                proof,
            } => {
                if batch.enclosing_activation != Some(*activation) || batch.state != Some(*state) {
                    return Err(PlanError::new(format!(
                        "pulse batch {} verified fusion fact changed its activation-local state",
                        batch.id
                    )));
                }
                let state_update_op = state_update_ops
                    .get(*state_update_arm_index)
                    .copied()
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "pulse batch {} verified fusion fact references missing update arm index {}",
                            batch.id, state_update_arm_index
                        ))
                    })?;
                if state_update_ops.len() != 1 {
                    return Err(PlanError::new(format!(
                        "pulse batch {} verified fusion fact requires exactly one update op",
                        batch.id
                    )));
                }
                PlanPulseFusionEligibility::VerifiedActivationLocalRecurrence {
                    state_update_op,
                    proof: match proof {
                        ir::PulseFusionProof::FrozenRuntimeTargetGuardedFullTraceEmptySideLanes => {
                            PlanPulseFusionProof::FrozenRuntimeTargetGuardedFullTraceEmptySideLanes
                        }
                        ir::PulseFusionProof::FrozenRuntimeTargetGuardedFullTracePreservedListMutations => {
                            PlanPulseFusionProof::FrozenRuntimeTargetGuardedFullTracePreservedListMutations
                        }
                    },
                }
            }
            ir::PulseFusionEligibility::Ineligible { diagnostics } => {
                PlanPulseFusionEligibility::Ineligible {
                    diagnostics: diagnostics.clone(),
                }
            }
        };
        pulse_batches.push(PlanPulseBatch {
            id: plan_pulse_batch_id(batch.id),
            owner,
            enclosing_activation: batch.enclosing_activation.map(plan_activation_id),
            state: batch.state.map(plan_state_id),
            start,
            count,
            state_update_ops,
            list_mutation_ops,
            derived_ops,
            emission_routes,
            schedule: match batch.schedule {
                ir::PulseSchedule::StageArbitrateCommitPublishBeforeNext => {
                    PlanPulseSchedule::StageArbitrateCommitPublishBeforeNext
                }
            },
            flush_policy: match batch.flush_policy {
                ir::PulseFlushPolicy::DiscardCurrentStopRemainingKeepPriorCommits => {
                    PlanPulseFlushPolicy::DiscardCurrentStopRemainingKeepPriorCommits
                }
            },
            fusion,
            semantic_slice_digest: batch.semantic_slice_digest,
        });
    }
    let outputs = output_root_plans(program, document.as_ref(), &index)?;
    let host_ports = host_port_plans(program, &outputs)?;
    match program_role {
        ProgramRole::Client if document.is_none() => {
            return Err(PlanError::new(
                "client programs must expose one retained document or scene root",
            ));
        }
        ProgramRole::Session | ProgramRole::Server if document.is_some() => {
            return Err(PlanError::new(format!(
                "{} programs cannot contain retained document or scene roots",
                program_role.as_str()
            )));
        }
        ProgramRole::Client | ProgramRole::Session | ProgramRole::Server => {}
    }

    let operation_count = regions.iter().map(|region| region.ops.len()).sum::<usize>();
    let unresolved_executable_ref_count = regions
        .iter()
        .flat_map(|region| &region.ops)
        .map(|op| op.unresolved_executable_ref_count)
        .sum::<usize>();
    let typed_value_ref_count = regions
        .iter()
        .flat_map(|region| &region.ops)
        .map(|op| op.inputs.len() + usize::from(op.output.is_some()))
        .sum::<usize>();
    let unknown_region_op_count = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter(|op| is_unknown_op(op))
        .count();
    let unknown_storage_op_count = list_slots
        .iter()
        .filter(|slot| matches!(slot.initializer_kind, ListInitializerKind::Unknown))
        .count()
        + non_executable_constant_payload_count(&constants);
    let unknown_plan_op_count = unknown_region_op_count + unknown_storage_op_count;
    let graph_clones_per_item = program
        .lists
        .iter()
        .map(|list| list.graph_clones_per_item)
        .max()
        .unwrap_or_default();
    let constant_count = constants.len();
    let source_route_count = source_routes.len();
    let scalar_storage_count = scalar_slots.len();
    let list_storage_count = list_slots.len();
    let typed_lowering_executable =
        unresolved_executable_ref_count == 0 && unknown_plan_op_count == 0;
    let cpu_plan_executor_unsupported_op_count =
        cpu_plan_executor_unsupported_op_count(&row_expressions, &regions, &scalar_slots);
    let cpu_plan_executor_complete =
        typed_lowering_executable && cpu_plan_executor_unsupported_op_count == 0;
    bind_effect_outbox_invocations(&mut effect_outbox, &regions)?;
    let producer_function_instances = complete_producer_function_ownership(
        program,
        &distributed.producer_function_instances,
        &scalar_slots,
        &list_slots,
        &list_indexes,
        &regions,
    )?;
    validate_producer_function_effect_ownership(&producer_function_instances, &regions, &effects)?;
    let transient_effect_result_targets = regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::StateUpdate {
                effect: Some(effect),
                ..
            }
            | PlanOpKind::EffectUpdate { effect, .. }
                if effects.iter().any(|contract| {
                    contract.effect_id == effect.effect_id
                        && matches!(
                            contract.replay,
                            EffectReplay::ReadOnly | EffectReplay::ProcessScoped
                        )
                }) =>
            {
                match &effect.result {
                    EffectResultRoute::Target { target, .. } => Some(target.clone()),
                }
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let application = ApplicationPlan::new(application_identity.clone())?;
    let transient_producer_states = producer_function_instances
        .iter()
        .flat_map(|instance| instance.ownership.states.iter().copied())
        .collect::<BTreeSet<_>>();
    let transient_producer_lists = producer_function_instances
        .iter()
        .flat_map(|instance| instance.ownership.lists.iter().copied())
        .collect::<BTreeSet<_>>();
    let persistence = persistence_plan(
        program,
        &application,
        schema_version,
        &scalar_slots,
        &list_slots,
        &authority_field_ids,
        &transient_effect_result_targets,
        &transient_producer_states,
        &transient_producer_lists,
        effect_outbox,
        migration_predecessors,
    )?;
    let unresolved_dependency_edges =
        unresolved_region_op_count(&regions, RegionKind::DependencyEdges)?;
    let unresolved_state_update_count =
        unresolved_region_op_count(&regions, RegionKind::StateUpdates)?;

    let mut plan = MachinePlan {
        version: PlanVersion::default(),
        target_profile,
        program_role,
        distributed_endpoint: distributed.endpoint.clone(),
        producer_function_instances,
        application,
        persistence,
        effects,
        outputs,
        host_ports,
        list_indexes,
        demand: demand_plan(
            program,
            &scalar_fields,
            &root_computation_fields,
            document.as_ref(),
        )?,
        document,
        row_expressions,
        constants,
        source_routes,
        activations,
        pulse_batches,
        storage_layout: StorageLayout {
            scalar_slots,
            list_slots,
            byte_banks,
        },
        dirty_plan: DirtyPlan {
            dependency_edges: program.dependencies.len(),
            unresolved_dependency_edges,
        },
        commit_plan: CommitPlan {
            state_update_count: program.state_update_arms.len(),
            unresolved_state_update_count,
        },
        delta_plan: DeltaPlan {
            deltas: delta_routes(program, &scalar_fields)?,
        },
        capability_summary: CapabilitySummary {
            executable: cpu_plan_executor_complete,
            typed_lowering_executable,
            cpu_plan_executor_complete,
            constant_count,
            source_route_count,
            scalar_storage_count,
            list_storage_count,
            byte_bank_storage_count,
            operation_count,
            typed_value_ref_count,
            executable_string_path_count: unresolved_executable_ref_count,
            unresolved_executable_ref_count,
            unknown_plan_op_count,
            cpu_plan_executor_unsupported_op_count,
            runtime_ast_dependency_count: 0,
            graph_rebuild_count: 0,
            graph_clones_per_item,
        },
        debug_map: DebugMap {
            source_units: program
                .semantic_index
                .source_units
                .iter()
                .map(|unit| DebugEntry {
                    id: format!("source_unit:{}", unit.id),
                    label: unit.path.clone(),
                })
                .collect(),
            source_routes: program
                .sources
                .iter()
                .map(|source| DebugEntry {
                    id: format!("source:{}", source.id),
                    label: source.path.clone(),
                })
                .collect(),
            state_slots: program
                .state_cells
                .iter()
                .map(|state| DebugEntry {
                    id: format!("state:{}", state.id),
                    label: state.path.clone(),
                })
                .collect(),
            list_slots: program
                .lists
                .iter()
                .map(|list| DebugEntry {
                    id: format!("list:{}", list.id),
                    label: list.name.clone(),
                })
                .collect(),
            derived_values: program
                .derived_values
                .iter()
                .map(|value| DebugEntry {
                    id: format!("field:{}", value.id),
                    label: value.path.clone(),
                })
                .collect(),
            fields: program
                .semantic_index
                .fields
                .iter()
                .map(|field| DebugEntry {
                    id: format!("field:{}", field.id),
                    label: field.path.clone(),
                })
                .chain(
                    authority_field_ids
                        .iter()
                        .map(|((list_name, field_name), field_id)| DebugEntry {
                            id: format!("field:{}", field_id.0),
                            label: format!("{list_name}.{field_name} [authority]"),
                        }),
                )
                .collect(),
            unresolved_executable_refs: unresolved_refs.into_iter().collect(),
        },
        regions,
    };
    if !distributed_row_linking_pending(distributed) {
        finalize_machine_plan_row_expressions(&mut plan)?;
    }
    validate_resource_only_fields_excluded(program, &plan)?;
    Ok(plan)
}

fn distributed_row_linking_pending(distributed: &DistributedMachineContext) -> bool {
    !distributed.expression_refs.is_empty()
        || !distributed.path_refs.is_empty()
        || !distributed.producer_function_instances.is_empty()
        || !distributed.synthetic_source_routes.is_empty()
        || !distributed.invocation_result_sources.is_empty()
        || distributed.endpoint.is_some()
}

fn collect_derived_row_expression_roots(
    expression: &PlanDerivedExpression,
    roots: &mut Vec<PlanRowExpressionId>,
) {
    match expression {
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            roots.push(*default);
            roots.extend(arms.iter().map(|arm| arm.value));
        }
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            roots.push(*expression);
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
}

fn collect_machine_plan_row_expression_roots(
    plan: &MachinePlan,
    include_index_keys: bool,
) -> Vec<PlanRowExpressionId> {
    let mut roots = Vec::new();
    if include_index_keys {
        for index in &plan.list_indexes {
            roots.extend(index.keys.iter().map(|key| key.expression));
        }
    }
    for slot in &plan.storage_layout.scalar_slots {
        if let ScalarInitializerPlan::Expression { expression } = &slot.initializer {
            roots.push(*expression);
        }
    }
    for slot in &plan.storage_layout.list_slots {
        for field in slot.initial_rows.iter().flat_map(|row| &row.fields) {
            if let PlanInitialListFieldInitializer::Expression { expression } = &field.initializer {
                roots.push(*expression);
            }
        }
    }
    if let Some(endpoint) = &plan.distributed_endpoint {
        for call in &endpoint.endpoint.remote_call_sites {
            roots.extend(call.arguments.iter().map(|argument| argument.value));
            roots.extend(call.invocation_arms.iter().map(|arm| arm.gate));
        }
    }
    if let Some(document) = &plan.document {
        for expression in &document.expressions {
            if let DocumentExprOp::RuntimeExpression { expression, .. } = &expression.op {
                roots.push(*expression);
            }
        }
    }
    for batch in &plan.pulse_batches {
        roots.push(batch.count);
        if let PlanPulseStart::Triggered { arms } = &batch.start {
            roots.extend(arms.iter().map(|arm| arm.gate));
        }
        roots.extend(batch.emission_routes.iter().filter_map(|route| {
            if let PlanPulseEmissionFilter::Skip { count } = route.filter {
                Some(count)
            } else {
                None
            }
        }));
    }
    for op in plan.regions.iter().flat_map(|region| &region.ops) {
        match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => collect_derived_row_expression_roots(expression, &mut roots),
            PlanOpKind::StateUpdate { value, effect, .. } => {
                roots.extend(value.iter().copied());
                if let Some(effect) = effect {
                    roots.push(effect.gate);
                    roots.extend(effect.intent_fields.iter().map(|field| field.expression));
                }
            }
            PlanOpKind::EffectUpdate {
                result_transform,
                effect,
                ..
            } => {
                roots.extend(result_transform.iter().copied());
                roots.push(effect.gate);
                roots.extend(effect.intent_fields.iter().map(|field| field.expression));
            }
            PlanOpKind::ListMutation { mutation } => match mutation {
                PlanListMutation::Append(append) => {
                    roots.push(append.gate);
                    roots.push(append.item);
                }
                PlanListMutation::Remove(remove) => {
                    roots.push(remove.gate);
                    roots.push(remove.predicate);
                }
            },
            PlanOpKind::SourceRoute
            | PlanOpKind::DerivedValue {
                expression: None, ..
            }
            | PlanOpKind::ListProjection { .. }
            | PlanOpKind::DependencyEdge => {}
        }
    }
    roots
}

fn remap_derived_row_expression_roots(
    expression: &mut PlanDerivedExpression,
    replacements: &BTreeMap<PlanRowExpressionId, PlanRowExpressionId>,
) {
    let remap = |expression: &mut PlanRowExpressionId| {
        if let Some(replacement) = replacements.get(expression) {
            *expression = *replacement;
        }
    };
    match expression {
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            remap(default);
            for arm in arms {
                remap(&mut arm.value);
            }
        }
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => remap(expression),
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
}

fn remap_machine_plan_row_expression_roots(
    plan: &mut MachinePlan,
    replacements: &BTreeMap<PlanRowExpressionId, PlanRowExpressionId>,
) {
    let remap = |expression: &mut PlanRowExpressionId| {
        if let Some(replacement) = replacements.get(expression) {
            *expression = *replacement;
        }
    };
    for index in &mut plan.list_indexes {
        for key in &mut index.keys {
            remap(&mut key.expression);
        }
    }
    for slot in &mut plan.storage_layout.scalar_slots {
        if let ScalarInitializerPlan::Expression { expression } = &mut slot.initializer {
            remap(expression);
        }
    }
    for slot in &mut plan.storage_layout.list_slots {
        for field in slot.initial_rows.iter_mut().flat_map(|row| &mut row.fields) {
            if let PlanInitialListFieldInitializer::Expression { expression } =
                &mut field.initializer
            {
                remap(expression);
            }
        }
    }
    if let Some(endpoint) = &mut plan.distributed_endpoint {
        for call in &mut endpoint.endpoint.remote_call_sites {
            for argument in &mut call.arguments {
                remap(&mut argument.value);
            }
            for arm in &mut call.invocation_arms {
                remap(&mut arm.gate);
            }
        }
    }
    if let Some(document) = &mut plan.document {
        for expression in &mut document.expressions {
            if let DocumentExprOp::RuntimeExpression { expression, .. } = &mut expression.op {
                remap(expression);
            }
        }
    }
    for batch in &mut plan.pulse_batches {
        remap(&mut batch.count);
        if let PlanPulseStart::Triggered { arms } = &mut batch.start {
            for arm in arms {
                remap(&mut arm.gate);
            }
        }
        for route in &mut batch.emission_routes {
            if let PlanPulseEmissionFilter::Skip { count } = &mut route.filter {
                remap(count);
            }
        }
    }
    for op in plan.regions.iter_mut().flat_map(|region| &mut region.ops) {
        match &mut op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => remap_derived_row_expression_roots(expression, replacements),
            PlanOpKind::StateUpdate { value, effect, .. } => {
                if let Some(value) = value {
                    remap(value);
                }
                if let Some(effect) = effect {
                    remap(&mut effect.gate);
                    for field in &mut effect.intent_fields {
                        remap(&mut field.expression);
                    }
                }
            }
            PlanOpKind::EffectUpdate {
                result_transform,
                effect,
                ..
            } => {
                if let Some(result_transform) = result_transform {
                    remap(result_transform);
                }
                remap(&mut effect.gate);
                for field in &mut effect.intent_fields {
                    remap(&mut field.expression);
                }
            }
            PlanOpKind::ListMutation { mutation } => match mutation {
                PlanListMutation::Append(append) => {
                    remap(&mut append.gate);
                    remap(&mut append.item);
                }
                PlanListMutation::Remove(remove) => {
                    remap(&mut remove.gate);
                    remap(&mut remove.predicate);
                }
            },
            PlanOpKind::SourceRoute
            | PlanOpKind::DerivedValue {
                expression: None, ..
            }
            | PlanOpKind::ListProjection { .. }
            | PlanOpKind::DependencyEdge => {}
        }
    }
}

fn validate_resource_value_ref(
    value: &ValueRef,
    resource_fields: &BTreeSet<FieldId>,
) -> Result<(), PlanError> {
    if let ValueRef::Field(field) = value
        && resource_fields.contains(field)
    {
        return Err(PlanError::new(format!(
            "resource-only FieldId {} entered a final MachinePlan ValueRef",
            field.0
        )));
    }
    Ok(())
}

fn validate_resource_derived_expression(
    expression: &PlanDerivedExpression,
    resource_fields: &BTreeSet<FieldId>,
) -> Result<(), PlanError> {
    match expression {
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { state, .. } => {
            validate_resource_value_ref(state, resource_fields)
        }
        PlanDerivedExpression::SourceEventTransform { arms, .. } => {
            for arm in arms {
                validate_resource_value_ref(&arm.trigger, resource_fields)?;
            }
            Ok(())
        }
        PlanDerivedExpression::BoolNot { input } => {
            validate_resource_value_ref(input, resource_fields)
        }
        PlanDerivedExpression::NumberCompareConst { left, .. } => {
            validate_resource_value_ref(left, resource_fields)
        }
        PlanDerivedExpression::ValueCompare { left, right, .. } => {
            validate_resource_value_ref(left, resource_fields)?;
            validate_resource_value_ref(right, resource_fields)
        }
        PlanDerivedExpression::RowExpression { .. }
        | PlanDerivedExpression::MaterializedRowField { .. } => Ok(()),
    }
}

pub(crate) fn validate_resource_only_fields_excluded(
    program: &ErasedProgram,
    plan: &MachinePlan,
) -> Result<(), PlanError> {
    let resource_fields = program
        .scope_index
        .fields
        .iter()
        .filter(|field| field.resource_only)
        .map(|field| plan_field_id(field.id))
        .collect::<BTreeSet<_>>();
    if resource_fields.is_empty() {
        return Ok(());
    }

    fn validate_field_id(
        field: FieldId,
        resource_fields: &BTreeSet<FieldId>,
        location: &str,
    ) -> Result<(), PlanError> {
        if resource_fields.contains(&field) {
            return Err(PlanError::new(format!(
                "resource-only FieldId {} entered {location}",
                field.0
            )));
        }
        Ok(())
    }

    fn validate_captures(
        captures: &[PlanRowCapture],
        resource_fields: &BTreeSet<FieldId>,
        location: &str,
    ) -> Result<(), PlanError> {
        for capture in captures {
            validate_field_id(capture.field, resource_fields, location)?;
        }
        Ok(())
    }

    fn validate_list_access_captures(
        access: &PlanListAccess,
        resource_fields: &BTreeSet<FieldId>,
    ) -> Result<(), PlanError> {
        for map in &access.maps {
            validate_captures(
                &map.captures,
                resource_fields,
                "a typed list-access scalar capture",
            )?;
        }
        Ok(())
    }

    if let RootOutputDemand::Selected(fields) = &plan.demand.root_derived_outputs {
        for field in fields {
            validate_field_id(
                *field,
                &resource_fields,
                "selected root-output scalar demand",
            )?;
        }
    }
    for memory in &plan.persistence.memory {
        for leaf in &memory.leaves {
            if let Some(field) = leaf.runtime_field_id {
                validate_field_id(field, &resource_fields, "persistent scalar memory metadata")?;
            }
        }
    }
    for list in &plan.persistence.lists {
        for field in &list.row_fields {
            if let Some(field) = field.runtime_field_id {
                validate_field_id(
                    field,
                    &resource_fields,
                    "persistent list-row memory metadata",
                )?;
            }
        }
    }
    for slot in &plan.storage_layout.scalar_slots {
        if let Some(field) = slot.indexed_field_id {
            validate_field_id(field, &resource_fields, "indexed scalar storage metadata")?;
        }
    }

    if let Some(distributed) = &plan.distributed_endpoint {
        for export in &distributed.endpoint.value_exports {
            validate_resource_value_ref(&export.value, &resource_fields)?;
        }
        for call in &distributed.endpoint.remote_call_sites {
            for arm in &call.invocation_arms {
                validate_resource_value_ref(&arm.trigger, &resource_fields)?;
            }
        }
    }
    for instance in &plan.producer_function_instances {
        validate_resource_value_ref(&instance.result, &resource_fields)?;
        for field in &instance.ownership.fields {
            if resource_fields.contains(field) {
                return Err(PlanError::new(format!(
                    "resource-only FieldId {} entered producer scalar ownership",
                    field.0
                )));
            }
        }
    }
    for output in &plan.outputs {
        match &output.value {
            OutputValueRef::RetainedVisual { .. } => {}
            OutputValueRef::RuntimeValue { value, list_fields } => {
                validate_resource_value_ref(value, &resource_fields)?;
                for field in list_fields {
                    if resource_fields.contains(&field.field_id) {
                        return Err(PlanError::new(format!(
                            "resource-only FieldId {} entered output list metadata",
                            field.field_id.0
                        )));
                    }
                }
            }
        }
    }
    for (expression_id, expression) in plan.row_expressions.iter() {
        match expression {
            PlanRowExpressionNode::Field { input } => {
                validate_resource_value_ref(input, &resource_fields)?;
            }
            PlanRowExpressionNode::ListGetField { field, .. } => {
                validate_field_id(*field, &resource_fields, "a scalar ListGetField read")?;
            }
            PlanRowExpressionNode::ListRowField { field, .. } => {
                if resource_fields.contains(field) {
                    let definition = program
                        .scope_index
                        .fields
                        .iter()
                        .find(|candidate| plan_field_id(candidate.id) == *field);
                    return Err(PlanError::new(format!(
                        "resource-only FieldId {}{} entered scalar ListRowField expression {}: {expression:?}",
                        field.0,
                        definition
                            .map(|field| format!(" (`{}`)", field.diagnostic_path))
                            .unwrap_or_default(),
                        expression_id.0,
                    )));
                }
            }
            PlanRowExpressionNode::ContextualCollection { captures, .. } => {
                validate_captures(
                    captures,
                    &resource_fields,
                    "a contextual collection scalar capture",
                )?;
            }
            PlanRowExpressionNode::ListAccess { access } => {
                validate_list_access_captures(access, &resource_fields)?;
            }
            PlanRowExpressionNode::ListPage { page } => {
                validate_list_access_captures(&page.access, &resource_fields)?;
            }
            _ => {}
        }
    }
    for delta in &plan.delta_plan.deltas {
        validate_resource_value_ref(&delta.output, &resource_fields)?;
    }

    for slot in &plan.storage_layout.list_slots {
        for field in &slot.row_fields {
            if resource_fields.contains(&field.field_id) {
                return Err(PlanError::new(format!(
                    "resource-only FieldId {} entered ListId {} scalar storage",
                    field.field_id.0, slot.list_id.0
                )));
            }
        }
        for row in &slot.initial_rows {
            for field in &row.fields {
                if let Some(field) = field.field_id {
                    validate_field_id(field, &resource_fields, "a scalar initial-list row field")?;
                }
            }
        }
    }

    if let Some(document) = &plan.document {
        for expression in &document.expressions {
            let DocumentExprOp::Read { read } = &expression.op else {
                continue;
            };
            match read {
                DocumentRead::Field { field } => {
                    validate_field_id(
                        *field,
                        &resource_fields,
                        "a retained document scalar field read",
                    )?;
                }
                DocumentRead::Row {
                    field: Some(field), ..
                } => {
                    validate_field_id(
                        *field,
                        &resource_fields,
                        "a retained document row-field read",
                    )?;
                }
                _ => {}
            }
        }
        for materialization in &document.materializations {
            let field = match &materialization.source {
                DocumentMaterializationSource::Field { field }
                | DocumentMaterializationSource::ScopedField { field, .. }
                | DocumentMaterializationSource::ParameterField { field, .. } => Some(*field),
                _ => None,
            };
            if let Some(field) = field {
                validate_field_id(
                    field,
                    &resource_fields,
                    "a retained document materialization source",
                )?;
            }
        }
        for binding in &document.view_bindings {
            let field = match &binding.target {
                DocumentBindingTarget::Field { field }
                | DocumentBindingTarget::ScopedField { field, .. } => Some(*field),
                _ => None,
            };
            if let Some(field) = field {
                validate_field_id(
                    field,
                    &resource_fields,
                    "a retained document scalar binding target",
                )?;
            }
        }
    }

    fn validate_copy(
        copy: &PlanMaterializedRowFieldCopy,
        resource_fields: &BTreeSet<FieldId>,
    ) -> Result<(), PlanError> {
        if resource_fields.contains(&copy.source_field)
            || resource_fields.contains(&copy.target_field)
        {
            return Err(PlanError::new(format!(
                "resource-only field entered a materialized row copy from {} to {}",
                copy.source_field.0, copy.target_field.0
            )));
        }
        Ok(())
    }

    fn validate_materialization_resource_fields(
        materialization: &PlanListMaterialization,
        resource_fields: &BTreeSet<FieldId>,
    ) -> Result<(), PlanError> {
        for (name, field) in &materialization.fields {
            if resource_fields.contains(field) {
                return Err(PlanError::new(format!(
                    "resource-only FieldId {} entered ListId {} scalar materialization field `{name}`",
                    field.0, materialization.target_list.0
                )));
            }
        }
        for copy in &materialization.row_field_copies {
            validate_copy(copy, resource_fields)?;
        }
        for authority in &materialization.value_list_authorities {
            for (name, field) in &authority.fields {
                if resource_fields.contains(field) {
                    return Err(PlanError::new(format!(
                        "resource-only FieldId {} entered ListId {} value-list authority field `{name}`",
                        field.0, authority.list_id.0
                    )));
                }
            }
        }
        Ok(())
    }

    let mut invalid_resource_field = None;
    for root in collect_machine_plan_row_expression_roots(plan, true) {
        plan.row_expressions.visit(root, &mut |_, expression| {
            if invalid_resource_field.is_none()
                && let PlanRowExpressionNode::ListRowField { field, .. } = expression
                && resource_fields.contains(field)
            {
                invalid_resource_field = Some(*field);
            }
        })?;
    }
    if let Some(field) = invalid_resource_field {
        return Err(PlanError::new(format!(
            "resource-only FieldId {} entered a scalar ListRowField read",
            field.0
        )));
    }

    for region in &plan.regions {
        for op in &region.ops {
            for input in &op.inputs {
                validate_resource_value_ref(input, &resource_fields)?;
            }
            if let Some(output) = &op.output {
                validate_resource_value_ref(output, &resource_fields)?;
            }
            match &op.kind {
                PlanOpKind::DerivedValue {
                    materialization: Some(materialization),
                    expression,
                    ..
                } => {
                    validate_materialization_resource_fields(materialization, &resource_fields)?;
                    if let Some(expression) = expression {
                        validate_resource_derived_expression(expression, &resource_fields)?;
                    }
                }
                PlanOpKind::DerivedValue {
                    materialization: None,
                    expression,
                    ..
                } => {
                    if let Some(expression) = expression {
                        validate_resource_derived_expression(expression, &resource_fields)?;
                    }
                }
                PlanOpKind::StateUpdate {
                    trigger, effect, ..
                } => {
                    validate_resource_value_ref(trigger, &resource_fields)?;
                    if let Some(effect) = effect {
                        match &effect.result {
                            EffectResultRoute::Target { target, .. } => {
                                validate_resource_value_ref(target, &resource_fields)?;
                            }
                        }
                    }
                }
                PlanOpKind::EffectUpdate {
                    trigger, effect, ..
                } => {
                    validate_resource_value_ref(trigger, &resource_fields)?;
                    match &effect.result {
                        EffectResultRoute::Target { target, .. } => {
                            validate_resource_value_ref(target, &resource_fields)?;
                        }
                    }
                }
                PlanOpKind::ListMutation { mutation } => match mutation {
                    PlanListMutation::Append(append) => {
                        validate_resource_value_ref(&append.trigger, &resource_fields)?;
                        for field in &append.fields {
                            if resource_fields.contains(&field.field_id) {
                                return Err(PlanError::new(format!(
                                    "resource-only FieldId {} entered list append field `{}`",
                                    field.field_id.0, field.name
                                )));
                            }
                        }
                        for copy in &append.row_field_copies {
                            validate_copy(copy, &resource_fields)?;
                        }
                    }
                    PlanListMutation::Remove(remove) => {
                        validate_resource_value_ref(&remove.trigger, &resource_fields)?;
                    }
                },
                PlanOpKind::ListProjection { projection } => match projection {
                    PlanListProjection::Chunk { .. } | PlanListProjection::Unknown { .. } => {}
                    PlanListProjection::ChunkValue { source, .. } => {
                        validate_resource_value_ref(source, &resource_fields)?;
                    }
                },
                PlanOpKind::SourceRoute | PlanOpKind::DependencyEdge => {}
            }
        }
    }
    Ok(())
}

fn refresh_typed_list_view_fingerprints(plan: &mut MachinePlan) -> Result<(), PlanError> {
    let snapshot = plan.clone();
    let fingerprint_context = TypedListViewFingerprintContext::new(&snapshot)?;
    let mut roots = collect_machine_plan_row_expression_roots(&snapshot, true);
    roots.sort_unstable();
    roots.dedup();
    let mut replacements = BTreeMap::new();
    for root in roots {
        let replacement =
            rewrite_row_expression(&mut plan.row_expressions, root, |_, original, node| {
                match snapshot.row_expressions.node(original)? {
                    PlanRowExpressionNode::ListPage {
                        page: original_page,
                    } => {
                        let PlanRowExpressionNode::ListPage { page } = node else {
                            return Err(PlanError::new(
                                "typed List/page fingerprint rewrite changed node kind",
                            ));
                        };
                        let index = snapshot
                            .list_indexes
                            .get(original_page.access.index.0)
                            .filter(|index| index.id == original_page.access.index)
                            .ok_or_else(|| {
                                PlanError::new(
                                    "typed List/page fingerprint references a missing index",
                                )
                            })?;
                        page.view_fingerprint = fingerprint_context.fingerprint(
                            index.source_list,
                            &original_page.access.semantic_order,
                            &original_page.access.guard,
                            &original_page.access.filters,
                            &original_page.access.maps,
                            &original_page.view_limit,
                        )?;
                    }
                    PlanRowExpressionNode::BoundedListPage {
                        page: original_page,
                    } => {
                        let PlanRowExpressionNode::BoundedListPage { page } = node else {
                            return Err(PlanError::new(
                                "bounded List/page fingerprint rewrite changed node kind",
                            ));
                        };
                        page.view_fingerprint =
                            fingerprint_context.bounded_fingerprint(original_page.view)?;
                    }
                    _ => {}
                }
                Ok(())
            })?;
        replacements.insert(root, replacement);
    }
    remap_machine_plan_row_expression_roots(plan, &replacements);
    Ok(())
}

/// Run once after document and distributed roots have all been attached.
pub(crate) fn finalize_machine_plan_row_expressions(
    plan: &mut MachinePlan,
) -> Result<(), PlanError> {
    refresh_typed_list_view_fingerprints(plan)?;
    compact_machine_plan_row_expressions(plan)?;
    validate_machine_plan_row_expression_reachability(plan)?;
    validate_typed_list_index_resources(plan)
}

fn compact_machine_plan_row_expressions(plan: &mut MachinePlan) -> Result<(), PlanError> {
    let mut roots = collect_machine_plan_row_expression_roots(plan, false);
    let mut seen_roots = BTreeSet::new();
    roots.retain(|root| seen_roots.insert(*root));

    let source = std::mem::take(&mut plan.row_expressions);
    let source_indexes = std::mem::take(&mut plan.list_indexes);
    let mut reachable = Vec::new();
    let mut completed = BTreeSet::new();
    let mut used_indexes = BTreeSet::new();
    let mut pending_roots = roots;
    while let Some(root) = pending_roots.pop() {
        let mut pending = vec![(root, false)];
        while let Some((expression, expanded)) = pending.pop() {
            if completed.contains(&expression) {
                continue;
            }
            if expanded {
                completed.insert(expression);
                reachable.push(expression);
                continue;
            }
            let node = source.node(expression)?;
            let mut referenced_indexes = Vec::new();
            visit_row_node_indexes(node, &mut |index| referenced_indexes.push(index));
            for index_id in referenced_indexes {
                let index = source_indexes
                    .get(index_id.0)
                    .filter(|index| index.id == index_id)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "row expression {} references missing typed list index {}",
                            expression.0, index_id.0
                        ))
                    })?;
                if used_indexes.insert(index_id) {
                    pending_roots.extend(index.keys.iter().map(|key| key.expression));
                }
            }
            pending.push((expression, true));
            let mut children = Vec::new();
            node.visit_children(&mut |child| children.push(child));
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .filter(|child| !completed.contains(child))
                    .map(|child| (child, false)),
            );
        }
    }

    let mut index_replacements = BTreeMap::new();
    let mut compacted_indexes = Vec::with_capacity(used_indexes.len());
    for index in source_indexes {
        if used_indexes.contains(&index.id) {
            let replacement = PlanListIndexId(compacted_indexes.len());
            index_replacements.insert(index.id, replacement);
            let mut index = index;
            index.id = replacement;
            compacted_indexes.push(index);
        }
    }

    let mut compacted = PlanRowExpressionArena::new();
    let mut replacements = BTreeMap::new();
    for original in reachable {
        let mut node = source.node(original)?.clone();
        let mut missing_child = None;
        visit_row_node_children_mut(&mut node, &mut |child| {
            if let Some(replacement) = replacements.get(child).copied() {
                *child = replacement;
            } else {
                missing_child = Some(*child);
            }
        });
        if let Some(child) = missing_child {
            return Err(PlanError::new(format!(
                "row expression compaction reached parent {} before child {}",
                original.0, child.0
            )));
        }
        remap_row_node_indexes(&mut node, &index_replacements)?;
        replacements.insert(original, compacted.intern(node)?);
    }

    plan.row_expressions = compacted;
    plan.list_indexes = compacted_indexes;
    remap_machine_plan_row_expression_roots(plan, &replacements);
    for instance in &mut plan.producer_function_instances {
        instance.ownership.indexes = instance
            .ownership
            .indexes
            .iter()
            .filter_map(|index| index_replacements.get(index).copied())
            .collect();
    }
    Ok(())
}

fn visit_row_node_indexes(node: &PlanRowExpressionNode, visitor: &mut impl FnMut(PlanListIndexId)) {
    match node {
        PlanRowExpressionNode::ContextualCollection {
            indexed_access: Some(access),
            ..
        } => visitor(access.index),
        PlanRowExpressionNode::ListAccess { access } => visitor(access.index),
        PlanRowExpressionNode::ListPage { page } => visitor(page.access.index),
        _ => {}
    }
}

fn remap_row_node_indexes(
    node: &mut PlanRowExpressionNode,
    replacements: &BTreeMap<PlanListIndexId, PlanListIndexId>,
) -> Result<(), PlanError> {
    let remap = |index: &mut PlanListIndexId| -> Result<(), PlanError> {
        *index = replacements.get(index).copied().ok_or_else(|| {
            PlanError::new(format!(
                "live row expression references pruned typed list index {}",
                index.0
            ))
        })?;
        Ok(())
    };
    match node {
        PlanRowExpressionNode::ContextualCollection {
            indexed_access: Some(access),
            ..
        } => remap(&mut access.index),
        PlanRowExpressionNode::ListAccess { access } => remap(&mut access.index),
        PlanRowExpressionNode::ListPage { page } => remap(&mut page.access.index),
        _ => Ok(()),
    }
}

fn validate_machine_plan_row_expression_reachability(plan: &MachinePlan) -> Result<(), PlanError> {
    plan.row_expressions.validate()?;
    let mut reachable = BTreeSet::new();
    for root in collect_machine_plan_row_expression_roots(plan, true) {
        reachable.extend(plan.row_expressions.walk_postorder(root)?);
    }
    if reachable.len() != plan.row_expressions.len() {
        return Err(PlanError::new(format!(
            "row expression compaction retained {} unreachable nodes",
            plan.row_expressions.len() - reachable.len()
        )));
    }
    Ok(())
}

pub(crate) fn distributed_exportable_values(
    program: &ErasedProgram,
) -> Result<BTreeMap<String, (boon_typecheck::FlowType, ValueRef)>, PlanError> {
    let scalar_fields = ScalarFieldCatalog::new(program)?;
    let index = ValueIndex::new(program, &BTreeMap::new(), &BTreeMap::new(), &scalar_fields)?;
    Ok(program
        .named_value_types
        .entries
        .iter()
        .filter_map(|entry| {
            let value_ref = index.resolve(&entry.path)?;
            matches!(
                value_ref,
                ValueRef::Source(_)
                    | ValueRef::SourcePayload { .. }
                    | ValueRef::State(_)
                    | ValueRef::StateProjection { .. }
                    | ValueRef::Field(_)
                    | ValueRef::Constant(_)
            )
            .then(|| (entry.path.clone(), (entry.flow_type.clone(), value_ref)))
        })
        .collect())
}

pub(crate) fn lower_distributed_root_expression(
    program: &ErasedProgram,
    owner_path: &str,
    expression: ir::ExecutableExprId,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    distributed: &DistributedMachineContext,
) -> Result<PlanRowExpressionId, PlanError> {
    let scalar_fields = ScalarFieldCatalog::new(program)?;
    let index = ValueIndex::new(
        program,
        &distributed.expression_refs,
        &distributed.path_refs,
        &scalar_fields,
    )?;
    program
        .executable
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| {
            PlanError::new(format!(
                "distributed call argument expression {expression} in `{owner_path}` is missing"
            ))
        })?;
    let mut inputs = Vec::new();
    ExecutableRowLowerer::new(program, &index, arena, constants, &mut inputs)
        .lower(expression)
        .map_err(|error| {
            PlanError::new(format!(
                "distributed call argument expression {expression} in `{owner_path}` failed executable lowering: {error}"
            ))
        })
}

pub(crate) fn lower_distributed_invocation_gate(
    program: &ErasedProgram,
    owner_path: &str,
    root: ir::ExecutableExprId,
    call: ir::ExecutableExprId,
    trigger: &ValueRef,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    distributed: &DistributedMachineContext,
) -> Result<PlanRowExpressionId, PlanError> {
    let scalar_fields = ScalarFieldCatalog::new(program)?;
    let index = ValueIndex::new(
        program,
        &distributed.expression_refs,
        &distributed.path_refs,
        &scalar_fields,
    )?;
    let mut inputs = Vec::new();
    ExecutableRowLowerer::new(program, &index, arena, constants, &mut inputs)
        .with_event_trigger(trigger)
        .lower_reachability_gate(root, call)
        .map_err(|error| {
            PlanError::new(format!(
                "distributed invocation gate from {root} to call {call} in `{owner_path}` failed executable lowering: {error}"
            ))
        })
}

fn initial_constant_value(value: &InitialValue) -> Option<PlanConstantValue> {
    match value {
        InitialValue::Text { value } => Some(PlanConstantValue::Text {
            value: value.clone(),
        }),
        InitialValue::Number { value } => Some(PlanConstantValue::Number {
            value: value.clone(),
        }),
        InitialValue::Bytes { bytes, .. } => {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            Some(PlanConstantValue::Bytes {
                byte_len: bytes.len() as u64,
                sha256: format!("{:x}", hasher.finalize()),
                inline_bytes: (bytes.len() <= INLINE_BYTE_CONSTANT_LIMIT).then(|| bytes.clone()),
            })
        }
        InitialValue::Tag { name } => Some(PlanConstantValue::Tag { name: name.clone() }),
        InitialValue::Data { value } => Some(PlanConstantValue::Data {
            value: value.clone(),
        }),
        InitialValue::RootInitialField { .. }
        | InitialValue::RowInitialField { .. }
        | InitialValue::ExpressionAuthority
        | InitialValue::ResourceOnly
        | InitialValue::Unknown { .. } => None,
    }
}

fn executable_state_initializer(
    program: &ErasedProgram,
    state: &ir::StateCell,
) -> Option<ir::ExecutableExprId> {
    let executable_state_id = state.executable_state_id?;
    let executable_state = program
        .executable
        .states
        .get(executable_state_id.as_usize())
        .filter(|candidate| candidate.id == executable_state_id)?;
    program
        .executable
        .expressions
        .get(executable_state.initial.as_usize())
        .filter(|candidate| candidate.id == executable_state.initial)
        .map(|_| executable_state.initial)
}

fn constant_state_initial_expression_value(
    program: &ErasedProgram,
    state: &ir::StateCell,
) -> Option<PlanConstantValue> {
    constant_executable_expression_value(program, executable_state_initializer(program, state)?)
}

fn constant_executable_expression_value(
    program: &ErasedProgram,
    expression: ir::ExecutableExprId,
) -> Option<PlanConstantValue> {
    constant_executable_expression_value_inner(program, expression, &mut BTreeSet::new())
}

fn constant_executable_expression_value_inner(
    program: &ErasedProgram,
    expression_id: ir::ExecutableExprId,
    visiting: &mut BTreeSet<ir::ExecutableExprId>,
) -> Option<PlanConstantValue> {
    if !visiting.insert(expression_id) {
        return None;
    }
    let expression = program
        .executable
        .expressions
        .get(expression_id.as_usize())
        .filter(|candidate| candidate.id == expression_id)?;
    let value = match &expression.kind {
        ir::ExecutableExpressionKind::Text(value) => Some(PlanConstantValue::Text {
            value: value.clone(),
        }),
        ir::ExecutableExpressionKind::Number(value) => Some(PlanConstantValue::Number {
            value: value.clone(),
        }),
        ir::ExecutableExpressionKind::BytesByte(value) => bytes_plan_constant(&[*value]),
        ir::ExecutableExpressionKind::Absent => None,
        ir::ExecutableExpressionKind::Tag(value) => Some(PlanConstantValue::Tag {
            name: value.clone(),
        }),
        ir::ExecutableExpressionKind::Bytes { .. } => {
            bytes_plan_constant(&executable_static_bytes(program, expression_id)?)
        }
        ir::ExecutableExpressionKind::Call {
            name, arguments, ..
        } if arguments.is_empty() => match name.as_str() {
            "Text/empty" => Some(PlanConstantValue::Text {
                value: String::new(),
            }),
            "Text/space" => Some(PlanConstantValue::Text {
                value: " ".to_owned(),
            }),
            _ => None,
        },
        ir::ExecutableExpressionKind::Hold { initial, .. } => {
            constant_executable_expression_value_inner(program, *initial, visiting)
        }
        _ => None,
    };
    visiting.remove(&expression_id);
    value
}

fn migration_source_row_default_expression(
    program: &ErasedProgram,
    source: &ir::MigrationSourceLeaf,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
) -> Result<PlanRowExpressionId, PlanError> {
    let memory = program
        .semantic_memory
        .get(source.memory_id.as_usize())
        .ok_or_else(|| PlanError::new("indexed migration default source memory is absent"))?;
    let source_state = state_for_semantic_memory(program, memory)?;
    if let Some(constant) = constant_state_initial_expression_value(program, source_state) {
        return arena.intern(PlanRowExpressionNode::Constant {
            constant_id: push_plan_constant(constants, constant),
        });
    }
    initial_state_expression(program, source_state, index, arena, constants).map_err(|error| {
        PlanError::new(format!(
            "indexed migration default `{}` is not reconstructable: {error}",
            source.semantic_path
        ))
    })
}

fn migration_indexed_default_expression(
    program: &ErasedProgram,
    state: &ir::StateCell,
    edge: &ir::MigrationEdge,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
) -> Result<PlanRowExpressionId, PlanError> {
    let mut grouped =
        BTreeMap::<boon_typecheck::CheckedExprId, Vec<&ir::MigrationSourceLeaf>>::new();
    for source in &edge.source_leaves {
        grouped
            .entry(checked_expr_id(
                source.drain_expr_id,
                "indexed migration DRAIN source",
            )?)
            .or_default()
            .push(source);
    }
    let mut drain_values = BTreeMap::new();
    for (drain_checked_expr_id, sources) in grouped {
        let mut fields = Vec::with_capacity(sources.len());
        for source in sources {
            fields.push((
                source
                    .semantic_path
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                migration_source_row_default_expression(program, source, index, arena, constants)?,
            ));
        }
        let value = if fields.len() == 1 {
            fields.pop().expect("one migration source exists").1
        } else {
            arena.intern(PlanRowExpressionNode::Object {
                fields: fields
                    .into_iter()
                    .map(|(name, value)| PlanRowObjectField {
                        name,
                        value,
                        spread: false,
                    })
                    .collect(),
            })?
        };
        drain_values.insert(drain_checked_expr_id, value);
    }
    if edge.transform == ir::MigrationTransform::Identity {
        if drain_values.len() != 1 {
            return Err(PlanError::new(
                "identity indexed migration default is ambiguous",
            ));
        }
        return drain_values
            .into_values()
            .next()
            .ok_or_else(|| PlanError::new("identity indexed migration default is not scalar"));
    }
    let ir::MigrationTransform::PureExpression {
        expression_root, ..
    } = &edge.transform
    else {
        return Err(PlanError::new(
            "indexed migration default has an unsupported transform",
        ));
    };
    let destination_memory = program
        .semantic_memory
        .get(edge.destination.memory_id.as_usize())
        .ok_or_else(|| {
            PlanError::new(format!(
                "indexed migration default `{}` references missing destination semantic memory {}",
                state.path, edge.destination.memory_id
            ))
        })?;
    let root =
        migration_transform_executable_root(program, edge, destination_memory, *expression_root)?;
    let mut bindings = BTreeMap::new();
    let mut bound_drain_origins = BTreeSet::new();
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = program
            .executable
            .expressions
            .get(expression_id.as_usize())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "indexed migration default `{}` references missing executable expression {}",
                    state.path, expression_id.0
                ))
            })?;
        let origin = expression.checked_expr_id;
        if matches!(expression.kind, ir::ExecutableExpressionKind::Drain { .. })
            && let Some(value) = drain_values.get(&origin)
        {
            bindings.insert(expression_id, *value);
            bound_drain_origins.insert(origin);
        }
        pending.extend(migration_executable_children(
            program,
            &expression.kind,
            "indexed migration default",
        )?);
    }
    let missing = drain_values
        .keys()
        .filter(|origin| !bound_drain_origins.contains(origin))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(PlanError::new(format!(
            "indexed migration default `{}` cannot bind executable DRAIN expression(s) {:?}",
            state.path, missing
        )));
    }
    let mut inputs = Vec::new();
    ExecutableRowLowerer::new(program, index, arena, constants, &mut inputs)
        .with_bindings(bindings)
        .with_state_initializer(state.id)
        .lower(root)
        .map_err(|error| {
            PlanError::new(format!(
                "indexed migration default `{}` failed executable lowering: {error}",
                state.path
            ))
        })
}

fn initial_state_expression(
    program: &ErasedProgram,
    state: &boon_ir::StateCell,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
) -> Result<PlanRowExpressionId, PlanError> {
    let executable_state_id = state.executable_state_id.ok_or_else(|| {
        PlanError::new(format!(
            "state `{}` has no exact executable state identity",
            state.path
        ))
    })?;
    let executable_state = program
        .executable
        .states
        .get(executable_state_id.as_usize())
        .filter(|candidate| candidate.id == executable_state_id)
        .ok_or_else(|| {
            PlanError::new(format!(
                "state `{}` references missing executable state {}",
                state.path, executable_state_id.0
            ))
        })?;
    program
        .executable
        .expressions
        .get(executable_state.initial.as_usize())
        .filter(|candidate| candidate.id == executable_state.initial)
        .ok_or_else(|| {
            PlanError::new(format!(
                "state `{}` executable state {} references missing initial expression {}",
                state.path, executable_state_id.0, executable_state.initial.0
            ))
        })?;
    let mut inputs = Vec::new();
    ExecutableRowLowerer::new(program, index, arena, constants, &mut inputs)
        .with_state_initializer(state.id)
        .lower(executable_state.initial)
        .map_err(|error| {
            PlanError::new(format!(
                "state `{}` failed executable initial lowering: {error}",
                state.path
            ))
        })
}

fn byte_bank_capacity_hint(
    slot: &ScalarStorageSlot,
    list_slots: &[ListStorageSlot],
) -> Option<usize> {
    if !slot.indexed {
        return Some(1);
    }
    list_slots
        .iter()
        .find(|list_slot| list_slot.scope_id == slot.scope_id)
        .and_then(|list_slot| list_slot.capacity)
}

fn plan_value_type_is_concrete(value_type: PlanValueType) -> bool {
    matches!(
        value_type,
        PlanValueType::Text
            | PlanValueType::Number
            | PlanValueType::Bytes { .. }
            | PlanValueType::Bits { .. }
            | PlanValueType::Tag
            | PlanValueType::Data
    )
}

fn derived_value_output_type(
    program: &ErasedProgram,
    derived: &boon_ir::DerivedValue,
) -> Option<PlanValueType> {
    let root = derived.producer;
    program
        .executable
        .expressions
        .get(root.as_usize())
        .and_then(|expression| plan_value_type_from_typecheck_type(&expression.flow_type.ty))
        .filter(|value_type| plan_value_type_is_concrete(*value_type))
        .or_else(|| inferred_executable_expression_value_type(program, root))
}

fn inferred_executable_expression_value_type(
    program: &ErasedProgram,
    expression: ir::ExecutableExprId,
) -> Option<PlanValueType> {
    inferred_executable_expression_value_type_inner(program, expression, &mut BTreeSet::new())
}

fn inferred_executable_expression_value_type_inner(
    program: &ErasedProgram,
    expression_id: ir::ExecutableExprId,
    visiting: &mut BTreeSet<ir::ExecutableExprId>,
) -> Option<PlanValueType> {
    let expression = program
        .executable
        .expressions
        .get(expression_id.as_usize())
        .filter(|candidate| candidate.id == expression_id)?;
    if let Some(value_type) = plan_value_type_from_typecheck_type(&expression.flow_type.ty)
        && plan_value_type_is_concrete(value_type)
    {
        return Some(value_type);
    }
    if !visiting.insert(expression_id) {
        return None;
    }
    let value_type = match &expression.kind {
        ir::ExecutableExpressionKind::Text(_)
        | ir::ExecutableExpressionKind::TextTemplate { .. } => Some(PlanValueType::Text),
        ir::ExecutableExpressionKind::Number(_) => Some(PlanValueType::Number),
        ir::ExecutableExpressionKind::Bits(value) => Some(PlanValueType::Bits {
            width: value.width(),
        }),
        ir::ExecutableExpressionKind::BytesByte(_) => {
            Some(PlanValueType::Bytes { fixed_len: Some(1) })
        }
        ir::ExecutableExpressionKind::Absent => None,
        ir::ExecutableExpressionKind::Flush { payload }
        | ir::ExecutableExpressionKind::FlushBoundary { input: payload } => {
            inferred_executable_expression_value_type_inner(program, *payload, visiting)
        }
        ir::ExecutableExpressionKind::Tag(_)
        | ir::ExecutableExpressionKind::TaggedObject { .. } => Some(PlanValueType::Tag),
        ir::ExecutableExpressionKind::Bytes {
            fixed_size: Some(len),
            ..
        } => Some(PlanValueType::Bytes {
            fixed_len: Some(*len as u64),
        }),
        ir::ExecutableExpressionKind::Call { name, .. } => inferred_builtin_call_value_type(name),
        ir::ExecutableExpressionKind::Infix { left, op, right } if op == "+" => {
            let left_type =
                inferred_executable_expression_value_type_inner(program, *left, visiting);
            let right_type =
                inferred_executable_expression_value_type_inner(program, *right, visiting);
            match (left_type, right_type) {
                (Some(PlanValueType::Number), Some(PlanValueType::Number)) => {
                    Some(PlanValueType::Number)
                }
                (Some(PlanValueType::Text), _) | (_, Some(PlanValueType::Text)) => {
                    Some(PlanValueType::Text)
                }
                _ => None,
            }
        }
        ir::ExecutableExpressionKind::Infix { left, right, .. } => {
            let left_type =
                inferred_executable_expression_value_type_inner(program, *left, visiting);
            let right_type =
                inferred_executable_expression_value_type_inner(program, *right, visiting);
            (left_type == Some(PlanValueType::Number) && right_type == Some(PlanValueType::Number))
                .then_some(PlanValueType::Number)
        }
        ir::ExecutableExpressionKind::Hold { initial, .. } => {
            inferred_executable_expression_value_type_inner(program, *initial, visiting)
        }
        ir::ExecutableExpressionKind::Latest { branches } => branches.first().and_then(|branch| {
            inferred_executable_expression_value_type_inner(program, *branch, visiting)
        }),
        ir::ExecutableExpressionKind::Then { input, output } => {
            inferred_executable_expression_value_type_inner(
                program,
                output.unwrap_or(*input),
                visiting,
            )
        }
        ir::ExecutableExpressionKind::Project { input, .. } => {
            inferred_executable_expression_value_type_inner(program, *input, visiting)
        }
        ir::ExecutableExpressionKind::Block { result, .. } => {
            inferred_executable_expression_value_type_inner(program, *result, visiting)
        }
        ir::ExecutableExpressionKind::FunctionParameter { .. }
        | ir::ExecutableExpressionKind::LocalRead { .. }
        | ir::ExecutableExpressionKind::CanonicalRead { .. }
        | ir::ExecutableExpressionKind::ExternalRead { .. }
        | ir::ExecutableExpressionKind::ElementState { .. }
        | ir::ExecutableExpressionKind::Drain { .. }
        | ir::ExecutableExpressionKind::Source { .. }
        | ir::ExecutableExpressionKind::Materialize { .. }
        | ir::ExecutableExpressionKind::Draining { .. }
        | ir::ExecutableExpressionKind::When { .. }
        | ir::ExecutableExpressionKind::MatchArm { .. }
        | ir::ExecutableExpressionKind::Object(_)
        | ir::ExecutableExpressionKind::List { .. }
        | ir::ExecutableExpressionKind::MapEntry { .. }
        | ir::ExecutableExpressionKind::Map { .. }
        | ir::ExecutableExpressionKind::Set { .. }
        | ir::ExecutableExpressionKind::Bytes {
            fixed_size: None, ..
        }
        | ir::ExecutableExpressionKind::Delimiter
        | ir::ExecutableExpressionKind::MaterializationLocal { .. } => None,
    };
    visiting.remove(&expression_id);
    value_type
}

fn inferred_builtin_call_value_type(function: &str) -> Option<PlanValueType> {
    match function {
        "Text/empty"
        | "Text/space"
        | "Text/trim"
        | "Text/to_lowercase"
        | "Text/to_uppercase"
        | "Text/concat"
        | "Text/slice"
        | "Text/time_range_label"
        | "Number/to_text"
        | "Number/to_codepoint_text"
        | "Number/to_ascii_text"
        | "Bytes/to_text"
        | "Bytes/to_hex"
        | "Bytes/to_base64"
        | "File/write_bytes"
        | "File/read_text"
        | "Router/route"
        | "Router/go_to" => Some(PlanValueType::Text),
        "Number/add"
        | "Number/subtract"
        | "Number/min"
        | "Number/max"
        | "Number/bit_width"
        | "Number/ceil"
        | "Number/floor"
        | "Number/round"
        | "Number/truncate"
        | "Number/interpolate"
        | "Number/project_width"
        | "Number/project_offset"
        | "Number/project_time"
        | "List/count"
        | "List/length"
        | "List/sum"
        | "Text/length"
        | "Bytes/length"
        | "Bytes/read_unsigned"
        | "Bytes/read_signed"
        | "Bits/width"
        | "Bits/to_number" => Some(PlanValueType::Number),
        "Text/find" | "Text/to_number" | "Bytes/find" | "Bytes/get" | "Bits/get"
        | "Bits/compare" | "Bits/try_add" | "Bits/try_subtract" | "Number/to_bits" => {
            Some(PlanValueType::Tag)
        }
        "Bool/not" | "Bool/and" | "Bool/or" | "Bool/toggle" | "Text/is_empty"
        | "Text/is_not_empty" | "Text/starts_with" | "Text/contains" | "Text/all_chars_in"
        | "Bytes/is_empty" | "Bytes/equal" | "Bytes/starts_with" | "Bytes/ends_with" => {
            Some(PlanValueType::Tag)
        }
        "Bytes/set"
        | "Bytes/slice"
        | "Bytes/take"
        | "Bytes/drop"
        | "Bytes/concat"
        | "Bytes/zeros"
        | "Text/to_bytes"
        | "Bytes/from_hex"
        | "Bytes/from_base64"
        | "Bytes/write_unsigned"
        | "Bytes/write_signed"
        | "Bits/to_bytes"
        | "File/read_bytes" => Some(PlanValueType::Bytes { fixed_len: None }),
        _ => None,
    }
}

fn plan_value_type_from_typecheck_type(ty: &boon_typecheck::Type) -> Option<PlanValueType> {
    match ty {
        boon_typecheck::Type::Text => Some(PlanValueType::Text),
        boon_typecheck::Type::Number => Some(PlanValueType::Number),
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Dynamic) => {
            Some(PlanValueType::Bytes { fixed_len: None })
        }
        boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Fixed(len)) => {
            Some(PlanValueType::Bytes {
                fixed_len: Some(*len as u64),
            })
        }
        boon_typecheck::Type::Bits { width } => Some(PlanValueType::Bits { width: *width }),
        boon_typecheck::Type::VariantSet(_) => Some(PlanValueType::Tag),
        _ => None,
    }
}

fn plan_initial_list_rows(
    program: &ErasedProgram,
    list: &boon_ir::ListMemory,
    initializer: &ListInitializer,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    list_indexes: &mut Vec<PlanListIndex>,
) -> Result<Vec<PlanInitialListRow>, PlanError> {
    let ListInitializer::RecordLiteral { rows } = initializer else {
        return Ok(Vec::new());
    };
    rows.iter()
        .map(|row| {
            Ok(PlanInitialListRow {
                fields: row
                    .fields
                    .iter()
                    .map(|field| {
                        if field.value == InitialValue::ResourceOnly {
                            return Ok(None);
                        }
                        let field_id = list
                            .initializer_inputs
                            .iter()
                            .find(|input| input.name == field.name)
                            .map(|input| plan_field_id(input.field))
                            .ok_or_else(|| {
                                PlanError::new(format!(
                                    "list `{}` initial field `{}` has no verified initializer FieldId",
                                    list.name, field.name
                                ))
                            })?;
                        if program
                            .scope_index
                            .fields
                            .get(field_id.0)
                            .filter(|field| plan_field_id(field.id) == field_id)
                            .is_some_and(|field| field.resource_only)
                        {
                            return Ok(None);
                        }
                        let initializer = if let Some(value) = initial_constant_value(&field.value) {
                            PlanInitialListFieldInitializer::Constant { value }
                        } else if let Some(expression_id) = field.expression {
                            let mut inputs = Vec::new();
                            let expression = ExecutableRowLowerer::new(
                                program,
                                index,
                                arena,
                                constants,
                                &mut inputs,
                            )
                            .with_list_indexes(list_indexes)
                            .lower(expression_id)
                            .map_err(|error| {
                                PlanError::new(format!(
                                    "list `{}` initial field `{}` failed exact expression lowering: {error}",
                                    list.name, field.name
                                ))
                            })?;
                            if let Some(input) = inputs.iter().find(|input| {
                                matches!(input, ValueRef::Source(_) | ValueRef::SourcePayload { .. })
                            }) {
                                return Err(PlanError::new(format!(
                                    "list `{}` initial field `{}` cannot depend on transient input {input:?}",
                                    list.name, field.name
                                )));
                            }
                            PlanInitialListFieldInitializer::Expression { expression }
                        } else {
                            match &field.value {
                            InitialValue::RootInitialField { path } => {
                                let input = index.resolve(path).ok_or_else(|| {
                                    PlanError::new(format!(
                                        "list `{}` initial field `{}` cannot resolve root value `{path}`",
                                        list.name, field.name
                                    ))
                                })?;
                                if matches!(
                                    input,
                                    ValueRef::Source(_)
                                        | ValueRef::SourcePayload { .. }
                                        | ValueRef::List(_)
                                ) {
                                    return Err(PlanError::new(format!(
                                        "list `{}` initial field `{}` cannot use transient or list value `{path}`",
                                        list.name, field.name
                                    )));
                                }
                                PlanInitialListFieldInitializer::Expression {
                                    expression: arena
                                        .intern(PlanRowExpressionNode::Field { input })?,
                                }
                            }
                            InitialValue::RowInitialField { path } => {
                                let mut projection = path.split('.');
                                let source_name = projection.next().filter(|name| !name.is_empty()).ok_or_else(|| {
                                    PlanError::new(format!(
                                        "list `{}` initial field `{}` has an empty row-field reference",
                                        list.name, field.name
                                    ))
                                })?;
                                let source = list
                                    .initializer_inputs
                                    .iter()
                                    .find(|input| input.name == source_name)
                                    .map(|input| plan_field_id(input.field))
                                    .ok_or_else(|| {
                                        PlanError::new(format!(
                                            "list `{}` initial field `{}` cannot resolve verified row initializer `{source_name}`",
                                            list.name, field.name
                                        ))
                                    })?;
                                let mut expression =
                                    arena.intern(PlanRowExpressionNode::Field {
                                        input: ValueRef::Field(source),
                                    })?;
                                for field in projection {
                                    expression = arena.intern(
                                        PlanRowExpressionNode::ObjectField {
                                        object: expression,
                                        field: field.to_owned(),
                                    })?;
                                }
                                PlanInitialListFieldInitializer::Expression { expression }
                            }
                            InitialValue::Unknown { summary } => {
                                return Err(PlanError::new(format!(
                                    "list `{}` initial field `{}` has unsupported expression `{summary}`",
                                    list.name, field.name
                                )));
                            }
                            InitialValue::ExpressionAuthority => {
                                return Err(PlanError::new(format!(
                                    "list `{}` initial field `{}` lost its checked collection-authority expression",
                                    list.name, field.name
                                )));
                            }
                            InitialValue::ResourceOnly => {
                                return Err(PlanError::new(format!(
                                    "list `{}` initial field `{}` exposed private row-source metadata as scalar data",
                                    list.name, field.name
                                )));
                            }
                            InitialValue::Text { .. }
                            | InitialValue::Number { .. }
                            | InitialValue::Bytes { .. }
                            | InitialValue::Tag { .. }
                            | InitialValue::Data { .. } => {
                                unreachable!("constant initial values were matched above")
                            }
                            }
                        };
                        Ok(Some(PlanInitialListField {
                            name: field.name.clone(),
                            field_id: Some(field_id),
                            initializer,
                        }))
                    })
                    .collect::<Result<Vec<_>, PlanError>>()?
                    .into_iter()
                    .flatten()
                    .collect(),
            })
        })
        .collect()
}

fn row_field_id_for_list_field(
    program: &ErasedProgram,
    list_name: &str,
    field_name: &str,
) -> Option<FieldId> {
    let list = program
        .lists
        .iter()
        .find(|list| list.name == list_name)
        .map(|list| list.id)?;
    program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            erased_field_is_runtime_row_storage(program, field)
                && field.row.map(|row| row.list) == Some(list)
                && field.name == field_name
                && field.row_path.as_slice() == [field_name]
        })
        .min_by_key(|field| {
            let value_priority = match field.role {
                ir::ErasedFieldRole::Value => 0,
                ir::ErasedFieldRole::ValueAuthority => 1,
                ir::ErasedFieldRole::ListAuthority => 2,
                ir::ErasedFieldRole::Capture => 3,
            };
            (
                value_priority,
                erased_row_field_depth(program, field),
                field.id,
            )
        })
        .map(|field| plan_field_id(field.id))
}

fn erased_row_field_depth(program: &ErasedProgram, field: &ir::ErasedFieldDef) -> usize {
    let Some(row) = field.row else {
        return usize::MAX;
    };
    let mut depth = 0usize;
    let mut parent = field.parent;
    let mut remaining = program.scope_index.fields.len().saturating_add(1);
    while let Some(parent_id) = parent {
        if remaining == 0 {
            return usize::MAX;
        }
        remaining -= 1;
        let Some(parent_field) = program
            .scope_index
            .fields
            .get(parent_id.as_usize())
            .filter(|candidate| candidate.id == parent_id)
        else {
            return usize::MAX;
        };
        if parent_field.row != Some(row) {
            break;
        }
        depth = depth.saturating_add(1);
        parent = parent_field.parent;
    }
    depth
}

fn erased_field_is_runtime_row_storage(
    program: &ErasedProgram,
    field: &ir::ErasedFieldDef,
) -> bool {
    !field.resource_only
        && !program.scope_index.bindings.iter().any(|binding| {
            (matches!(binding.target, ir::ErasedBindingTarget::Source { .. })
                && field.producer == Some(binding.producer))
                || (matches!(
                    binding.target,
                    ir::ErasedBindingTarget::Value {
                        field: None,
                        row: Some(row),
                    } if Some(row) == field.row
                ) && field.producer == Some(binding.producer)
                    && field.declaration == Some(binding.declaration))
                || (matches!(
                    binding.target,
                    ir::ErasedBindingTarget::State {
                        field: Some(authority),
                        row: Some(row),
                        ..
                    } if Some(row) == field.row && authority != field.id
                ) && field.producer == Some(binding.producer)
                    && field.declaration == Some(binding.declaration))
        })
}

fn storage_input_field_id(
    program: &ErasedProgram,
    list_name: &str,
    field_name: &str,
    authority_field_ids: &BTreeMap<(String, String), FieldId>,
) -> Option<FieldId> {
    authority_field_ids
        .get(&(list_name.to_owned(), field_name.to_owned()))
        .copied()
        .or_else(|| row_field_id_for_list_field(program, list_name, field_name))
}

pub(super) fn row_input_field_id_for_list_id(
    program: &ErasedProgram,
    list_id: ListId,
    field_name: &str,
) -> Option<FieldId> {
    let list = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)?;
    row_field_id_for_list_field(program, &list.name, field_name)
}

fn indexed_state_field_ids(program: &ErasedProgram) -> BTreeSet<FieldId> {
    program
        .semantic_memory
        .iter()
        .filter_map(|memory| match memory.runtime_backing {
            ir::SemanticMemoryRuntimeBacking::IndexedState {
                field_id: Some(field_id),
                ..
            } => Some(plan_field_id(field_id)),
            _ => None,
        })
        .collect()
}

fn erased_field_is_list_constructor_authority(
    program: &ErasedProgram,
    field: &ir::ErasedFieldDef,
    indexed_state_fields: &BTreeSet<FieldId>,
) -> bool {
    if !field.role.is_authority() || indexed_state_fields.contains(&plan_field_id(field.id)) {
        return false;
    }
    !field.parent.is_some_and(|parent| {
        program
            .scope_index
            .fields
            .get(parent.as_usize())
            .filter(|candidate| candidate.id == parent)
            .is_some_and(|parent| parent.row == field.row && parent.role.is_authority())
    })
}

fn erased_constructor_authority_has_separate_value(
    program: &ErasedProgram,
    field: &ir::ErasedFieldDef,
) -> bool {
    program.scope_index.fields.iter().any(|candidate| {
        candidate.id != field.id
            && candidate.row == field.row
            && candidate.row_path == field.row_path
            && candidate.role == ir::ErasedFieldRole::Value
            && erased_field_is_runtime_row_storage(program, candidate)
    })
}

fn public_list_value_field_ids(
    program: &ErasedProgram,
    list: &boon_ir::ListMemory,
) -> BTreeSet<FieldId> {
    let mut names = BTreeSet::new();
    for memory in program.semantic_memory.iter().filter(|memory| {
        semantic_memory_is_runtime_active(program, memory)
            && matches!(
                memory.runtime_backing,
                ir::SemanticMemoryRuntimeBacking::List { list_id, .. }
                    if list_id == list.id
            )
    }) {
        let DataTypePlan::List { item } =
            semantic_data_type_plan(&memory.data_type).canonicalized()
        else {
            continue;
        };
        match item.as_ref() {
            DataTypePlan::Record { fields, .. } => {
                names.extend(
                    fields
                        .iter()
                        .filter(|field| !field.name.starts_with("@authority:"))
                        .map(|field| field.name.clone()),
                );
            }
            _ => {
                names.insert("value".to_owned());
            }
        }
    }
    names
        .into_iter()
        .filter_map(|name| row_field_id_for_list_field(program, &list.name, &name))
        .collect()
}

fn list_row_fields(program: &ErasedProgram, list: &boon_ir::ListMemory) -> Vec<PlanListRowField> {
    let indexed_state_fields = indexed_state_field_ids(program);
    let public_value_fields = public_list_value_field_ids(program, list);
    program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            erased_field_is_runtime_row_storage(program, field)
                && field.row.map(|row| row.list) == Some(list.id)
        })
        .map(|field| PlanListRowField {
            field_id: plan_field_id(field.id),
            name: field.name.clone(),
            role: if field.role == ir::ErasedFieldRole::Capture {
                PlanListRowFieldRole::Capture
            } else {
                let constructor_authority = erased_field_is_list_constructor_authority(
                    program,
                    field,
                    &indexed_state_fields,
                );
                let value = if constructor_authority {
                    public_value_fields.contains(&plan_field_id(field.id))
                        && !erased_constructor_authority_has_separate_value(program, field)
                } else {
                    field.role.is_value()
                };
                match (value, constructor_authority) {
                    (true, true) => PlanListRowFieldRole::ValueAuthority,
                    (false, true) => PlanListRowFieldRole::Authority,
                    // Nested structural authorities and indexed HOLD
                    // authorities remain exact row values. Only top-level list
                    // schema fields participate in record construction.
                    (_, false) => PlanListRowFieldRole::Value,
                }
            },
        })
        .collect()
}

fn list_authority_field_ids(program: &ErasedProgram) -> BTreeMap<(String, String), FieldId> {
    let indexed_fields = indexed_state_field_ids(program);
    let mut fields = BTreeMap::<(String, String), (u8, usize, FieldId)>::new();
    for field in
        program.scope_index.fields.iter().filter(|field| {
            erased_field_is_list_constructor_authority(program, field, &indexed_fields)
        })
    {
        let Some(row) = field.row else {
            continue;
        };
        let Some(list) = program
            .lists
            .get(row.list.as_usize())
            .filter(|list| list.id == row.list)
        else {
            continue;
        };
        let key = (list.name.clone(), field.name.clone());
        let field_id = plan_field_id(field.id);
        let candidate = (0, 0, field_id);
        match fields.get(&key) {
            Some(current) if *current <= candidate => {}
            _ => {
                fields.insert(key, candidate);
            }
        }
    }
    fields
        .into_iter()
        .map(|(key, (_, _, field))| (key, field))
        .collect()
}

fn bytes_plan_constant(bytes: &[u8]) -> Option<PlanConstantValue> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(PlanConstantValue::Bytes {
        byte_len: bytes.len() as u64,
        sha256: format!("{:x}", hasher.finalize()),
        inline_bytes: (bytes.len() <= INLINE_BYTE_CONSTANT_LIMIT).then(|| bytes.to_vec()),
    })
}

fn derived_expression_for_value(
    program: &ErasedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    distributed: &DistributedMachineContext,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    list_indexes: &mut Vec<PlanListIndex>,
    _unresolved_refs: &mut BTreeSet<String>,
) -> Result<Option<PlanDerivedExpression>, PlanError> {
    if !derived.trigger_arms.is_empty() {
        return source_event_transform_expression(
            program,
            derived,
            index,
            arena,
            constants,
            inputs,
            list_indexes,
        )
        .map(Some);
    }
    row_expression_for_value(
        program,
        derived,
        index,
        distributed,
        arena,
        constants,
        inputs,
        list_indexes,
    )
}

fn derived_materialized_list_id(
    _program: &ErasedProgram,
    derived: &boon_ir::DerivedValue,
) -> Option<ListId> {
    (derived.kind == DerivedValueKind::ListView)
        .then_some(derived.materialized_list_id)
        .flatten()
        .map(plan_list_id)
}

struct MaterializedRowFieldPlan {
    name: String,
    local: Option<PlanMaterializedRowLocal>,
    output: FieldId,
    expression: PlanRowExpressionId,
    inputs: Vec<ValueRef>,
}

fn materialized_output_field<'a>(
    program: &'a ErasedProgram,
    target_list: ListId,
    name: &str,
) -> Result<&'a ir::ErasedFieldDef, PlanError> {
    program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == target_list)
        .ok_or_else(|| PlanError::new(format!("materialized list {} is missing", target_list.0)))?;
    let fields = program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            erased_field_is_runtime_row_storage(program, field)
                && field.row.map(|row| plan_list_id(row.list)) == Some(target_list)
                && erased_field_is_direct_list_member(field)
                && field.name == name
                && field.role.is_value()
        })
        .collect::<Vec<_>>();
    let value_fields = fields
        .iter()
        .copied()
        .filter(|field| field.role == ir::ErasedFieldRole::Value)
        .collect::<Vec<_>>();
    let candidates = if value_fields.is_empty() {
        fields.as_slice()
    } else {
        value_fields.as_slice()
    };
    let [field] = candidates else {
        return Err(PlanError::new(format!(
            "materialized list {} field `{name}` resolves to {} preferred semantic value fields \
             among {} candidates: {:?}",
            target_list.0,
            candidates.len(),
            fields.len(),
            fields
                .iter()
                .map(|field| (
                    field.id,
                    field.role,
                    field.declaration,
                    field.statement,
                    field.producer,
                    field.diagnostic_path.as_str(),
                ))
                .collect::<Vec<_>>()
        )));
    };
    Ok(*field)
}

fn materialized_output_fields(
    program: &ErasedProgram,
    target_list: ListId,
    state_fields: &BTreeMap<String, StateId>,
) -> Result<BTreeMap<String, FieldId>, PlanError> {
    program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == target_list)
        .ok_or_else(|| PlanError::new(format!("materialized list {} is missing", target_list.0)))?;
    let names = program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            erased_field_is_runtime_row_storage(program, field)
                && field.row.map(|row| plan_list_id(row.list)) == Some(target_list)
                && erased_field_is_direct_list_member(field)
                && field.role.is_value()
                && !field.resource_only
                && !state_fields.contains_key(&field.name)
        })
        .map(|field| field.name.clone())
        .collect::<BTreeSet<_>>();

    names
        .into_iter()
        .map(|name| {
            let field = materialized_output_field(program, target_list, &name)?;
            Ok((name, plan_field_id(field.id)))
        })
        .collect()
}

/// Splits an authority-backed map into independent row-field computations.
/// The logical rows already exist in the list slot, so evaluating the complete
/// map would only duplicate row iteration and eagerly compute unrelated fields.
fn take_authority_mapped_row_fields(
    program: &ErasedProgram,
    target_list: ListId,
    expression: &mut PlanDerivedExpression,
    arena: &mut PlanRowExpressionArena,
) -> Result<Option<Vec<MaterializedRowFieldPlan>>, PlanError> {
    let PlanDerivedExpression::RowExpression { expression } = expression else {
        return Ok(None);
    };
    let mut collection = arena.node(*expression)?.clone();
    let PlanRowExpressionNode::ContextualCollection {
        owner,
        operation: PlanContextualOperationKind::Map,
        source,
        row_local,
        body,
        captures,
        indexed_access: None,
    } = &mut collection
    else {
        return Ok(None);
    };
    if !captures.is_empty() {
        return Ok(None);
    }
    if !matches!(
        arena.node(*source)?,
        PlanRowExpressionNode::AuthorityListRef { list_id } if *list_id == target_list
    ) {
        return Ok(None);
    }
    let mut body_node = arena.node(*body)?.clone();
    let PlanRowExpressionNode::Object { fields } = &mut body_node else {
        return Ok(None);
    };
    if fields.iter().any(|field| field.spread) {
        return Ok(None);
    }

    let mut extracted = Vec::new();
    for field in std::mem::take(fields) {
        let target = materialized_output_field(program, target_list, &field.name)?;
        if target.resource_only {
            continue;
        }
        let mut inputs = BTreeSet::new();
        arena.visit_inputs(field.value, &mut |input| {
            inputs.insert(input);
        })?;
        extracted.push(MaterializedRowFieldPlan {
            name: field.name,
            local: Some(PlanMaterializedRowLocal {
                owner: *owner,
                row_local: *row_local,
            }),
            output: plan_field_id(target.id),
            expression: field.value,
            inputs: inputs.into_iter().collect(),
        });
    }
    *body = arena.intern(body_node)?;
    *expression = arena.intern(collection)?;
    Ok((!extracted.is_empty()).then_some(extracted))
}

fn project_materialized_map_field(
    arena: &mut PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    source_list: ListId,
    source_field: FieldId,
    name: &str,
) -> Result<Option<PlanRowExpressionId>, PlanError> {
    let node = arena.node(expression)?.clone();
    match node {
        PlanRowExpressionNode::Object { fields }
        | PlanRowExpressionNode::TaggedObject { fields, .. } => {
            if fields.iter().any(|field| field.spread) {
                return Ok(None);
            }
            Ok(fields
                .into_iter()
                .find(|field| field.name == name)
                .map(|field| field.value))
        }
        PlanRowExpressionNode::LocalRow {
            owner: candidate_owner,
            local,
        } if candidate_owner == owner && local == row_local => {
            Ok(Some(arena.intern(PlanRowExpressionNode::ListRowField {
                row: expression,
                list_id: source_list,
                field: source_field,
            })?))
        }
        PlanRowExpressionNode::Select { input, arms } => {
            let mut projected = Vec::with_capacity(arms.len());
            let mut found = false;
            for arm in arms {
                let value = match project_materialized_map_field(
                    arena,
                    arm.value,
                    owner,
                    row_local,
                    source_list,
                    source_field,
                    name,
                )? {
                    Some(value) => {
                        found = true;
                        value
                    }
                    None => arena.push(PlanRowExpressionNode::Absent)?,
                };
                projected.push(PlanRowSelectArm {
                    pattern: arm.pattern,
                    value,
                });
            }
            if !found {
                return Ok(None);
            }
            Ok(Some(arena.intern(PlanRowExpressionNode::Select {
                input,
                arms: projected,
            })?))
        }
        _ => Ok(None),
    }
}

fn projected_materialized_row_field(
    program: &ErasedProgram,
    arena: &mut PlanRowExpressionArena,
    expression: &PlanDerivedExpression,
    materializations: &[&ir::ContextualMaterialization],
    name: &str,
) -> Result<Option<PlanRowExpressionId>, PlanError> {
    let PlanDerivedExpression::RowExpression { expression } = expression else {
        return Ok(None);
    };
    let order = arena.walk_postorder(*expression)?;
    let mut projected = Vec::new();
    for materialization in materializations {
        let Some(source_list) = materialization.source_list_id.map(plan_list_id) else {
            continue;
        };
        let owner = PlanStaticOwnerId(materialization.owner.as_usize());
        let row_local = PlanLocalId(materialization.row_local.0 as usize);
        let Some(source_field) = row_input_field_id_for_list_id(program, source_list, name) else {
            continue;
        };
        for candidate in &order {
            let PlanRowExpressionNode::ContextualCollection {
                owner: candidate_owner,
                operation: PlanContextualOperationKind::Map,
                row_local: candidate_local,
                body,
                ..
            } = arena.node(*candidate)?
            else {
                continue;
            };
            if *candidate_owner != owner || *candidate_local != row_local {
                continue;
            }
            if let Some(value) = project_materialized_map_field(
                arena,
                *body,
                owner,
                row_local,
                source_list,
                source_field,
                name,
            )? {
                projected.push(value);
            }
        }
    }
    projected.sort();
    projected.dedup();
    match projected.as_slice() {
        [] => Ok(None),
        [expression] => Ok(Some(*expression)),
        _ => Err(PlanError::new(format!(
            "materialized row field `{name}` has {} distinct guarded map projections",
            projected.len()
        ))),
    }
}

fn state_dependent_materialized_row_fields(
    program: &ErasedProgram,
    target_list: ListId,
    state_fields: &BTreeMap<String, StateId>,
    materialization_expression: Option<&PlanDerivedExpression>,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    list_indexes: &mut Vec<PlanListIndex>,
    include_state_independent: bool,
) -> Result<Vec<MaterializedRowFieldPlan>, PlanError> {
    let target_states = state_fields.values().copied().collect::<BTreeSet<_>>();
    if target_states.is_empty() && !include_state_independent {
        return Ok(Vec::new());
    }
    program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == target_list)
        .ok_or_else(|| PlanError::new(format!("materialized list {} is missing", target_list.0)))?;
    let candidates = program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            erased_field_is_runtime_row_storage(program, field)
                && field.row.map(|row| plan_list_id(row.list)) == Some(target_list)
                && erased_field_is_direct_list_member(field)
                && field.role == ir::ErasedFieldRole::Value
                && !field.resource_only
                && !state_fields.contains_key(&field.name)
                && field.producer.is_some()
        })
        .map(|field| {
            (
                field.name.clone(),
                plan_field_id(field.id),
                field
                    .producer
                    .expect("filtered materialized field producer"),
            )
        })
        .collect::<Vec<_>>();
    let target_derived_values = program
        .derived_values
        .iter()
        .enumerate()
        .filter(|(_, derived)| {
            derived
                .materialized_list_id
                .is_some_and(|list| plan_list_id(list) == target_list)
        })
        .map(|(index, _)| index)
        .collect::<BTreeSet<_>>();
    let pulse_triggers = program
        .pulse_batches()
        .iter()
        .filter(|batch| {
            batch
                .state
                .is_some_and(|state| target_states.contains(&plan_state_id(state)))
                && batch
                    .derived_value_indices
                    .iter()
                    .any(|index| target_derived_values.contains(index))
        })
        .map(|batch| ValueRef::Pulse(plan_pulse_batch_id(batch.id)))
        .collect::<Vec<_>>();
    let materializations = program
        .materializations
        .iter()
        .filter(|materialization| {
            materialization.operation == ir::ContextualOperationKind::Map
                && materialization
                    .target_list_id
                    .is_some_and(|list| plan_list_id(list) == target_list)
        })
        .collect::<Vec<_>>();

    let mut trial = Vec::new();
    for (name, output, producer) in candidates {
        let projected = materialization_expression
            .map(|expression| {
                projected_materialized_row_field(
                    program,
                    arena,
                    expression,
                    &materializations,
                    &name,
                )
            })
            .transpose()?
            .flatten();
        if let Some(projected) = projected {
            let mut dependencies = BTreeSet::new();
            arena.visit_inputs(projected, &mut |input| {
                dependencies.insert(input);
            })?;
            trial.push((name, output, producer, dependencies, None, Some(projected)));
            continue;
        }
        let mut lowered = None;
        for event_trigger in std::iter::once(None).chain(pulse_triggers.iter().cloned().map(Some)) {
            let mut trial_arena = arena.clone();
            let mut trial_constants = constants.clone();
            let mut trial_indexes = list_indexes.clone();
            let mut trial_inputs = Vec::new();
            let mut lowerer = ExecutableRowLowerer::new(
                program,
                index,
                &mut trial_arena,
                &mut trial_constants,
                &mut trial_inputs,
            )
            .with_list_indexes(&mut trial_indexes);
            if let Some(trigger) = event_trigger.as_ref() {
                lowerer = lowerer.with_event_trigger(trigger);
            }
            match lowerer.lower(producer) {
                Ok(_) => {
                    lowered = Some((event_trigger, trial_inputs));
                    break;
                }
                Err(_) => {}
            }
        }
        let Some((event_trigger, trial_inputs)) = lowered else {
            continue;
        };
        let dependencies = trial_inputs.into_iter().collect::<BTreeSet<_>>();
        trial.push((name, output, producer, dependencies, event_trigger, None));
    }

    let mut deferred = BTreeSet::new();
    loop {
        let before = deferred.len();
        for (_, output, _, dependencies, _, _) in &trial {
            if dependencies.iter().any(|dependency| match dependency {
                ValueRef::State(state) => target_states.contains(state),
                ValueRef::StateProjection { state_id, .. } => target_states.contains(state_id),
                ValueRef::Field(field) => deferred.contains(field),
                _ => false,
            }) {
                deferred.insert(*output);
            }
        }
        if deferred.len() == before {
            break;
        }
    }

    let mut result = Vec::new();
    for (name, output, producer, _, event_trigger, projected) in trial {
        if !include_state_independent && !deferred.contains(&output) {
            continue;
        }
        let mut inputs = Vec::new();
        let mut expression = if let Some(projected) = projected {
            arena.visit_inputs(projected, &mut |input| inputs.push(input))?;
            projected
        } else {
            let mut lowerer =
                ExecutableRowLowerer::new(program, index, arena, constants, &mut inputs)
                    .with_list_indexes(list_indexes);
            if let Some(trigger) = event_trigger.as_ref() {
                lowerer = lowerer.with_event_trigger(trigger);
            }
            lowerer.lower(producer).map_err(|error| {
                PlanError::new(format!(
                    "materialized list {} state-dependent field `{name}` failed exact lowering: {error}",
                    target_list.0
                ))
            })?
        };
        let local = if arena.contextual_locals_resolve(expression)? {
            None
        } else {
            let mut resolved = None;
            for materialization in &materializations {
                let owner = PlanStaticOwnerId(materialization.owner.as_usize());
                let row_local = PlanLocalId(materialization.row_local.0 as usize);
                if materialization.source_list_id.is_none()
                    && arena.contextual_locals_resolve_with(expression, owner, row_local)?
                {
                    resolved = Some(PlanMaterializedRowLocal { owner, row_local });
                    break;
                }
                let Some(source_list) = materialization.source_list_id.map(plan_list_id) else {
                    continue;
                };
                let candidate = retarget_materialized_source_local(
                    program,
                    arena,
                    expression,
                    owner,
                    row_local,
                    source_list,
                    target_list,
                )?;
                if arena.contextual_locals_resolve_with(candidate, owner, row_local)? {
                    expression = candidate;
                    resolved = Some(PlanMaterializedRowLocal { owner, row_local });
                    break;
                }
            }
            Some(resolved.ok_or_else(|| {
                PlanError::new(format!(
                    "materialized list {} state-dependent field `{name}` retains contextual rows that cannot be represented by its keyed output row",
                    target_list.0
                ))
            })?)
        };
        inputs.sort();
        inputs.dedup();
        result.push(MaterializedRowFieldPlan {
            name,
            local,
            output,
            expression,
            inputs,
        });
    }
    Ok(result)
}

fn retarget_materialized_source_local(
    program: &ErasedProgram,
    arena: &mut PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    source_list: ListId,
    target_list: ListId,
) -> Result<PlanRowExpressionId, PlanError> {
    let target_list_name = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == target_list)
        .map(|list| list.name.clone())
        .ok_or_else(|| {
            PlanError::new(format!(
                "materialized target list {} is missing",
                target_list.0
            ))
        })?;
    let authority_fields = list_authority_field_ids(program);
    rewrite_row_expression(arena, expression, |arena, _, node| {
        if let PlanRowExpressionNode::ListRowField {
            row,
            list_id,
            field,
        } = node
            && *list_id == source_list
            && matches!(
                arena.node(*row)?,
                PlanRowExpressionNode::LocalRow {
                    owner: local_owner,
                    local,
                } if *local_owner == owner && *local == row_local
            )
        {
            let source_name = program
                .scope_index
                .fields
                .iter()
                .find(|candidate| {
                    erased_field_is_runtime_row_storage(program, candidate)
                        && candidate.row.map(|row| plan_list_id(row.list)) == Some(source_list)
                        && plan_field_id(candidate.id) == *field
                })
                .map(|field| field.name.clone())
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "materialized source list {} field {} has no semantic name",
                        source_list.0, field.0
                    ))
                })?;
            *field = if let Some(field) = authority_fields
                .get(&(target_list_name.clone(), source_name.clone()))
                .copied()
            {
                field
            } else {
                plan_field_id(materialized_output_field(program, target_list, &source_name)?.id)
            };
            *list_id = target_list;
        }
        Ok(())
    })
}

fn materialized_state_fields(
    program: &ErasedProgram,
    target_list: ListId,
) -> Result<BTreeMap<String, StateId>, PlanError> {
    let mut result = BTreeMap::new();
    for binding in &program.scope_index.bindings {
        let ir::ErasedBindingTarget::State {
            runtime,
            field: Some(field),
            row: Some(row),
            ..
        } = binding.target
        else {
            continue;
        };
        if plan_list_id(row.list) != target_list {
            continue;
        }
        let erased_field = program
            .scope_index
            .fields
            .iter()
            .find(|candidate| candidate.id == field)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "state binding {} references missing erased field {}",
                    binding.id, field
                ))
            })?;
        let state = plan_state_id(runtime);
        if let Some(previous) = result.insert(erased_field.name.clone(), state)
            && previous != state
        {
            return Err(PlanError::new(format!(
                "materialized list {} field `{}` belongs to states {} and {}",
                target_list.0, erased_field.name, previous.0, state.0
            )));
        }
    }
    Ok(result)
}

fn materialized_resource_fields(
    program: &ErasedProgram,
    target_list: ListId,
) -> impl Iterator<Item = String> + '_ {
    program
        .scope_index
        .fields
        .iter()
        .filter(move |field| {
            field.row.map(|row| plan_list_id(row.list)) == Some(target_list)
                && field.role.is_value()
                && field.resource_only
        })
        .map(|field| field.name.clone())
}

fn strip_materialized_non_value_fields(
    expression: &mut PlanDerivedExpression,
    omitted_fields: &BTreeSet<String>,
    arena: &mut PlanRowExpressionArena,
) -> Result<(), PlanError> {
    match expression {
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            *default = strip_materialized_non_value_row_fields(arena, *default, omitted_fields)?;
            for arm in arms {
                arm.value =
                    strip_materialized_non_value_row_fields(arena, arm.value, omitted_fields)?;
            }
        }
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            *expression =
                strip_materialized_non_value_row_fields(arena, *expression, omitted_fields)?;
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
    Ok(())
}

fn detach_startup_pulse_materialization(
    program: &ErasedProgram,
    expression: &mut PlanDerivedExpression,
    arena: &PlanRowExpressionArena,
) -> Result<bool, PlanError> {
    let PlanDerivedExpression::SourceEventTransform { default, arms, .. } = expression else {
        return Ok(false);
    };
    if !matches!(arena.node(*default)?, PlanRowExpressionNode::Absent) {
        return Ok(false);
    }
    let Some(first) = arms.first() else {
        return Ok(false);
    };
    let startup_pulse_arm = |arm: &PlanSourceEventTransformArm| {
        let ValueRef::Pulse(pulse) = arm.trigger else {
            return false;
        };
        program
            .pulse_batches()
            .get(pulse.0)
            .filter(|batch| plan_pulse_batch_id(batch.id) == pulse)
            .is_some_and(|batch| matches!(batch.start, ir::PulseStart::Startup))
    };
    if !startup_pulse_arm(first)
        || arms
            .iter()
            .any(|arm| !startup_pulse_arm(arm) || arm.value != first.value)
    {
        return Ok(false);
    }
    if !arena.contextual_locals_resolve(first.value)? {
        return Err(PlanError::new(
            "startup pulse materialization seed retains unresolved contextual locals",
        ));
    }
    *expression = PlanDerivedExpression::RowExpression {
        expression: first.value,
    };
    Ok(true)
}

fn strip_materialized_non_value_row_fields(
    arena: &mut PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    omitted_fields: &BTreeSet<String>,
) -> Result<PlanRowExpressionId, PlanError> {
    fn collect_targets(
        arena: &PlanRowExpressionArena,
        expression: PlanRowExpressionId,
        visited: &mut BTreeSet<PlanRowExpressionId>,
        targets: &mut BTreeSet<PlanRowExpressionId>,
    ) -> Result<(), PlanError> {
        if !visited.insert(expression) {
            return Ok(());
        }
        match arena.node(expression)? {
            PlanRowExpressionNode::Object { .. } | PlanRowExpressionNode::TaggedObject { .. } => {
                targets.insert(expression);
            }
            PlanRowExpressionNode::ContextualCollection {
                operation: PlanContextualOperationKind::Map,
                body,
                ..
            } => collect_targets(arena, *body, visited, targets)?,
            PlanRowExpressionNode::Select { arms, .. } => {
                for arm in arms {
                    collect_targets(arena, arm.value, visited, targets)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    let mut targets = BTreeSet::new();
    collect_targets(arena, expression, &mut BTreeSet::new(), &mut targets)?;
    rewrite_row_expression(arena, expression, |_, original, node| {
        if targets.contains(&original)
            && let PlanRowExpressionNode::Object { fields }
            | PlanRowExpressionNode::TaggedObject { fields, .. } = node
        {
            fields.retain(|field| !omitted_fields.contains(&field.name));
        }
        Ok(())
    })
}

fn derived_expression_reads_state(
    expression: &PlanDerivedExpression,
    state: StateId,
    arena: &PlanRowExpressionArena,
) -> Result<bool, PlanError> {
    let mut found = false;
    let mut visit_row = |expression: PlanRowExpressionId| -> Result<(), PlanError> {
        arena.visit_value_refs(expression, &mut |value| {
            found |= *value == ValueRef::State(state);
        })
    };
    match expression {
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            visit_row(*default)?;
            for arm in arms {
                visit_row(arm.value)?;
            }
        }
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => visit_row(*expression)?,
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { state: input, .. }
        | PlanDerivedExpression::BoolNot { input } => {
            found |= *input == ValueRef::State(state);
        }
        PlanDerivedExpression::NumberCompareConst { left, .. } => {
            found |= *left == ValueRef::State(state);
        }
        PlanDerivedExpression::ValueCompare { left, right, .. } => {
            found |= *left == ValueRef::State(state) || *right == ValueRef::State(state);
        }
    }
    Ok(found)
}

fn collect_materialized_list_field_names(
    expression: &PlanDerivedExpression,
    names: &mut BTreeSet<String>,
    arena: &PlanRowExpressionArena,
) -> Result<(), PlanError> {
    match expression {
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            collect_materialized_list_row_names(arena, *expression, names)?;
        }
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            collect_materialized_list_row_names(arena, *default, names)?;
            for arm in arms {
                collect_materialized_list_row_names(arena, arm.value, names)?;
            }
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
    Ok(())
}

#[derive(Clone)]
struct OrderedKeyCandidate {
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    expression: PlanRowExpressionId,
    direction: PlanOrderDirection,
    multiplicity: PlanListIndexKeyMultiplicity,
}

#[derive(Clone)]
struct UnresolvedOrderedKeyCandidate {
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    expression: PlanRowExpressionId,
    direction: PlanRowExpressionId,
}

struct ResolvedOrderDirectionVariants {
    selectors: Vec<PlanRowExpressionId>,
    key_variants: Vec<(Vec<PlanOrderDirection>, Vec<OrderedKeyCandidate>)>,
}

struct PlannedListAccessSet {
    selectors: Vec<PlanRowExpressionId>,
    variants: Vec<(Vec<PlanOrderDirection>, PlanListAccess)>,
}

const MAX_DYNAMIC_ORDER_SELECTORS: usize = 3;
const MAX_BOUNDED_LIST_PAGE_ITEMS: u32 = 10_000;
const MAX_EXHAUSTIVE_LIST_CANDIDATES: u32 = 10_000;

fn lower_bounded_list_access(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: &mut PlanDerivedExpression,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<(), PlanError> {
    match expression {
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            *default = lower_bounded_row_access(
                program,
                value_index,
                arena,
                constants,
                *default,
                indexes,
            )?;
            for arm in arms {
                arm.value = lower_bounded_row_access(
                    program,
                    value_index,
                    arena,
                    constants,
                    arm.value,
                    indexes,
                )?;
            }
            Ok(())
        }
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            *expression = lower_bounded_row_access(
                program,
                value_index,
                arena,
                constants,
                *expression,
                indexes,
            )?;
            Ok(())
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => Ok(()),
    }
}

fn lower_bounded_row_access(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: PlanRowExpressionId,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<PlanRowExpressionId, PlanError> {
    let order = arena.walk_postorder(expression)?;
    let mut rewritten = BTreeMap::new();
    for original in order {
        let mut node = arena.node(original)?.clone();
        let mut missing_child = None;
        visit_row_node_children_mut(&mut node, &mut |child| {
            if let Some(replacement) = rewritten.get(child).copied() {
                *child = replacement;
            } else {
                missing_child = Some(*child);
            }
        });
        if let Some(child) = missing_child {
            return Err(PlanError::new(format!(
                "bounded row lowering reached parent {} before child {}",
                original.0, child.0
            )));
        }

        if let PlanRowExpressionNode::ContextualCollection {
            owner,
            operation,
            source,
            row_local,
            body,
            indexed_access,
            ..
        } = &mut node
            && indexed_access.is_none()
        {
            *indexed_access = plan_contextual_indexed_access(
                program,
                value_index,
                arena,
                constants,
                *owner,
                *operation,
                *source,
                *row_local,
                *body,
                indexes,
            )?;
        }

        let terminal_take = match &node {
            PlanRowExpressionNode::BuiltinCall {
                function: PlanRowBuiltin::ListTake,
                input: Some(input),
                args,
            } => Some((*input, args.clone())),
            _ => None,
        };
        let replacement = if let Some((input, args)) = terminal_take {
            let limit = required_list_terminal_argument(&args, "count", "List/take")?;
            let requires_proven_access = typed_list_view_requires_proven_access(arena, input)?;
            if let Some(accesses) = plan_typed_list_access(
                program,
                value_index,
                arena,
                constants,
                input,
                limit,
                indexes,
            )? {
                build_directional_access_expression(arena, accesses, |access| {
                    PlanRowExpressionNode::ListAccess {
                        access: Box::new(access),
                    }
                })?
            } else if requires_proven_access {
                let (candidate_input, lowered) = lower_exhaustive_candidate_view(
                    program,
                    value_index,
                    arena,
                    constants,
                    input,
                    limit,
                    indexes,
                )?;
                if !lowered {
                    return Err(PlanError::new(
                        "typed List/take has no compiler-proven bounded source-order or keyed access path",
                    ));
                }
                arena.intern(PlanRowExpressionNode::BuiltinCall {
                    function: PlanRowBuiltin::ListTake,
                    input: Some(candidate_input),
                    args,
                })?
            } else {
                arena.intern(node)?
            }
        } else {
            arena.intern(node)?
        };
        rewritten.insert(original, replacement);
    }
    rewritten.get(&expression).copied().ok_or_else(|| {
        PlanError::new(format!(
            "bounded row lowering did not produce expression {}",
            expression.0
        ))
    })
}

fn lower_exhaustive_candidate_view(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: PlanRowExpressionId,
    terminal_limit: PlanRowExpressionId,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<(PlanRowExpressionId, bool), PlanError> {
    let mut node = arena.node(expression)?.clone();
    if matches!(
        &node,
        PlanRowExpressionNode::ContextualCollection {
            operation: PlanContextualOperationKind::Filter,
            ..
        }
    ) && let Some(accesses) = plan_typed_list_access(
        program,
        value_index,
        arena,
        constants,
        expression,
        terminal_limit,
        indexes,
    )? {
        let expression = build_directional_access_expression(arena, accesses, |mut access| {
            access.exhaustive_candidate_limit = Some(MAX_EXHAUSTIVE_LIST_CANDIDATES);
            PlanRowExpressionNode::ListAccess {
                access: Box::new(access),
            }
        })?;
        return Ok((expression, true));
    }

    let child = match &node {
        PlanRowExpressionNode::ContextualCollection { source, .. }
        | PlanRowExpressionNode::ContextualOrder { source, .. } => Some(*source),
        PlanRowExpressionNode::BuiltinCall {
            input: Some(input), ..
        } => Some(*input),
        _ => None,
    };
    let Some(child) = child else {
        return Ok((expression, false));
    };
    let (replacement, lowered) = lower_exhaustive_candidate_view(
        program,
        value_index,
        arena,
        constants,
        child,
        terminal_limit,
        indexes,
    )?;
    if !lowered {
        return Ok((expression, false));
    }
    visit_row_node_children_mut(&mut node, &mut |candidate| {
        if *candidate == child {
            *candidate = replacement;
        }
    });
    Ok((arena.intern(node)?, true))
}

fn plan_contextual_indexed_access(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &[PlanConstant],
    owner: PlanStaticOwnerId,
    operation: PlanContextualOperationKind,
    source: PlanRowExpressionId,
    row_local: PlanLocalId,
    body: PlanRowExpressionId,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<Option<Box<PlanContextualIndexedAccess>>, PlanError> {
    if !matches!(
        operation,
        PlanContextualOperationKind::Filter
            | PlanContextualOperationKind::Retain
            | PlanContextualOperationKind::Any
            | PlanContextualOperationKind::Find
    ) {
        return Ok(None);
    }
    let PlanRowExpressionNode::ListRef { list_id } = arena.node(source)? else {
        return Ok(None);
    };
    let list_id = *list_id;
    let PlanRowExpressionNode::NumberInfix { op, left, right } = arena.node(body)? else {
        return Ok(None);
    };
    if *op != PlanInfixOp::Equal {
        return Ok(None);
    }
    let indexed_operand =
        |candidate: PlanRowExpressionId,
         value: PlanRowExpressionId|
         -> Result<Option<(PlanRowExpressionId, PlanRowExpressionId)>, PlanError> {
            let PlanRowExpressionNode::ListRowField {
                row,
                list_id: candidate_list,
                ..
            } = arena.node(candidate)?
            else {
                return Ok(None);
            };
            if *candidate_list != list_id
                || !matches!(
                    arena.node(*row)?,
                    PlanRowExpressionNode::LocalRow {
                        owner: candidate_owner,
                        local: candidate_local,
                    } if *candidate_owner == owner && *candidate_local == row_local
                )
                || arena.references_contextual_local(value, owner, row_local)?
            {
                return Ok(None);
            }
            Ok(Some((candidate, value)))
        };
    let Some((key_expression, value)) =
        indexed_operand(*left, *right)?.or(indexed_operand(*right, *left)?)
    else {
        return Ok(None);
    };
    let Some(key) = plan_typed_list_index_key(
        program,
        value_index,
        arena,
        constants,
        owner,
        row_local,
        key_expression,
        PlanOrderDirection::Ascending,
        PlanListIndexKeyMultiplicity::One,
    )?
    else {
        return Ok(None);
    };
    let index = ensure_typed_list_index(arena, list_id, vec![key], indexes)?;
    Ok(Some(Box::new(PlanContextualIndexedAccess {
        index,
        selection: PlanListAccessSelection::KeyPrefix {
            values: vec![value],
        },
    })))
}

fn typed_list_view_requires_proven_access(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
) -> Result<bool, PlanError> {
    Ok(match arena.node(expression)? {
        PlanRowExpressionNode::ContextualCollection {
            operation: PlanContextualOperationKind::Map,
            source,
            ..
        } => typed_list_view_requires_proven_access(arena, *source)?,
        PlanRowExpressionNode::ContextualCollection {
            operation: PlanContextualOperationKind::Filter,
            ..
        }
        | PlanRowExpressionNode::ContextualOrder { .. } => true,
        _ => false,
    })
}

fn validate_literal_page_size(
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    size: PlanRowExpressionId,
) -> Result<(), PlanError> {
    let PlanRowExpressionNode::Constant { constant_id } = arena.node(size)? else {
        return Ok(());
    };
    let Some(PlanConstant {
        value: PlanConstantValue::Number { value },
        ..
    }) = constants
        .iter()
        .find(|constant| constant.id == *constant_id)
    else {
        return Ok(());
    };
    let value = value.to_i64_exact();
    if !matches!(value, Ok(1..=10_000)) {
        return Err(PlanError::new(
            "`List/page` size must be a whole Number between 1 and 10000",
        ));
    }
    Ok(())
}

fn bounded_list_page_view(
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: PlanRowExpressionId,
) -> Result<bool, PlanError> {
    Ok(match arena.node(expression)? {
        PlanRowExpressionNode::Field { .. } | PlanRowExpressionNode::ListAccess { .. } => true,
        PlanRowExpressionNode::ListLiteral { items } => {
            items.len() <= MAX_BOUNDED_LIST_PAGE_ITEMS as usize
        }
        PlanRowExpressionNode::ListRange { from, to } => {
            let Some(from) = static_whole_number(arena, constants, *from)? else {
                return Ok(false);
            };
            let Some(to) = static_whole_number(arena, constants, *to)? else {
                return Ok(false);
            };
            from.abs_diff(to) <= u64::from(MAX_BOUNDED_LIST_PAGE_ITEMS)
        }
        PlanRowExpressionNode::ContextualCollection { source, .. }
        | PlanRowExpressionNode::ContextualOrder { source, .. } => {
            bounded_list_page_view(arena, constants, *source)?
        }
        PlanRowExpressionNode::BuiltinCall {
            function: PlanRowBuiltin::ListTake,
            input: Some(source),
            ..
        } => bounded_list_page_view(arena, constants, *source)?,
        PlanRowExpressionNode::Select { arms, .. } => {
            if arms.is_empty() {
                false
            } else {
                let mut bounded = true;
                for arm in arms {
                    bounded &= bounded_list_page_view(arena, constants, arm.value)?;
                }
                bounded
            }
        }
        _ => false,
    })
}

fn static_whole_number(
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: PlanRowExpressionId,
) -> Result<Option<u64>, PlanError> {
    let PlanRowExpressionNode::Constant { constant_id } = arena.node(expression)? else {
        return Ok(None);
    };
    let Some(constant) = constants
        .iter()
        .find(|constant| constant.id == *constant_id)
    else {
        return Ok(None);
    };
    let PlanConstantValue::Number { value } = &constant.value else {
        return Ok(None);
    };
    Ok(value.to_u64_exact().ok())
}

fn required_list_terminal_argument(
    args: &[PlanRowCallArg],
    name: &str,
    function: &str,
) -> Result<PlanRowExpressionId, PlanError> {
    args.iter()
        .find(|argument| argument.name == name)
        .map(|argument| argument.value)
        .ok_or_else(|| {
            PlanError::new(format!(
                "typed {function} lowering lost its exact `{name}` expression"
            ))
        })
}

fn split_terminal_take(
    arena: &PlanRowExpressionArena,
    input: PlanRowExpressionId,
) -> Result<(PlanRowExpressionId, Option<PlanRowExpressionId>), PlanError> {
    let PlanRowExpressionNode::BuiltinCall {
        function,
        input: Some(source),
        args,
    } = arena.node(input)?
    else {
        return Ok((input, None));
    };
    if *function != PlanRowBuiltin::ListTake {
        return Ok((input, None));
    }
    let limit = required_list_terminal_argument(args, "count", "List/take")?;
    Ok((*source, Some(limit)))
}

fn peel_trailing_list_maps(
    arena: &PlanRowExpressionArena,
    mut view: PlanRowExpressionId,
) -> Result<(PlanRowExpressionId, Vec<PlanListMap>), PlanError> {
    let mut maps = Vec::new();
    loop {
        let PlanRowExpressionNode::ContextualCollection {
            owner,
            operation: PlanContextualOperationKind::Map,
            source,
            row_local,
            body,
            captures,
            indexed_access: None,
        } = arena.node(view)?
        else {
            maps.reverse();
            return Ok((view, maps));
        };
        maps.push(PlanListMap {
            owner: *owner,
            row_local: *row_local,
            body: *body,
            captures: captures.clone(),
        });
        view = *source;
    }
}

fn plan_typed_list_access(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &[PlanConstant],
    view: PlanRowExpressionId,
    limit: PlanRowExpressionId,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<Option<PlannedListAccessSet>, PlanError> {
    let (view, maps) = peel_trailing_list_maps(arena, view)?;
    let mut unresolved_keys = Vec::new();
    let mut filters = Vec::new();
    let Some(source_list) = collect_access_view(arena, view, &mut unresolved_keys, &mut filters)?
    else {
        return Ok(None);
    };
    let ResolvedOrderDirectionVariants {
        selectors,
        key_variants,
    } = resolve_order_direction_variants(arena, constants, &unresolved_keys)?;
    let mut variants = Vec::with_capacity(key_variants.len());
    for (directions, keys) in key_variants {
        let Some(access) = plan_static_typed_list_access(
            program,
            value_index,
            arena,
            constants,
            source_list,
            keys,
            filters.clone(),
            maps.clone(),
            limit,
            indexes,
        )?
        else {
            return Ok(None);
        };
        variants.push((directions, access));
    }
    Ok(Some(PlannedListAccessSet {
        selectors,
        variants,
    }))
}

#[allow(clippy::too_many_arguments)]
fn plan_static_typed_list_access(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &[PlanConstant],
    source_list: ListId,
    mut keys: Vec<OrderedKeyCandidate>,
    filters: Vec<PlanListFilter>,
    maps: Vec<PlanListMap>,
    limit: PlanRowExpressionId,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<Option<PlanListAccess>, PlanError> {
    let semantic_order_candidates = keys.clone();
    let (selection, guard) = if filters.is_empty() {
        (PlanListAccessSelection::OrderedStart, None)
    } else {
        let Some((selection, guard)) = plan_filter_access_selection(arena, &filters, &mut keys)?
        else {
            return Ok(None);
        };
        (selection, guard)
    };

    let plan_keys = |keys: &[OrderedKeyCandidate]| -> Result<Option<Vec<_>>, PlanError> {
        let mut planned = Vec::with_capacity(keys.len());
        for key in keys {
            let Some(key) = plan_typed_list_index_key(
                program,
                value_index,
                arena,
                constants,
                key.owner,
                key.row_local,
                key.expression,
                key.direction,
                key.multiplicity,
            )?
            else {
                return Ok(None);
            };
            planned.push(key);
        }
        Ok(Some(planned))
    };
    let Some(planned_keys) = plan_keys(&keys)? else {
        return Ok(None);
    };
    let Some(mut semantic_order) = plan_keys(&semantic_order_candidates)? else {
        return Ok(None);
    };
    for key in &mut semantic_order {
        let canonical_owner = PlanStaticOwnerId(0);
        let canonical_local = PlanLocalId(0);
        let Some(expression) = remap_row_expression_contextual_local(
            arena,
            key.expression,
            (key.owner, key.row_local),
            (canonical_owner, canonical_local),
        )?
        else {
            return Ok(None);
        };
        key.owner = canonical_owner;
        key.row_local = canonical_local;
        key.expression = expression;
    }

    let index_id = ensure_typed_list_index(arena, source_list, planned_keys, indexes)?;
    Ok(Some(PlanListAccess {
        index: index_id,
        semantic_order,
        exhaustive_candidate_limit: None,
        guard,
        filters,
        maps,
        selection,
        limit,
    }))
}

fn resolve_order_direction_variants(
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    keys: &[UnresolvedOrderedKeyCandidate],
) -> Result<ResolvedOrderDirectionVariants, PlanError> {
    enum DirectionSource {
        Static(PlanOrderDirection),
        Selector(usize),
    }

    let mut selectors = Vec::<PlanRowExpressionId>::new();
    let mut directions = Vec::with_capacity(keys.len());
    for key in keys {
        if let Some(direction) = static_order_direction(arena, constants, key.direction)? {
            directions.push(DirectionSource::Static(direction));
            continue;
        }
        if arena.references_contextual_local(key.direction, key.owner, key.row_local)? {
            return Err(PlanError::new(
                "typed list order direction must be row-independent",
            ));
        }
        let selector = selectors
            .iter()
            .position(|selector| *selector == key.direction)
            .unwrap_or_else(|| {
                let selector = selectors.len();
                selectors.push(key.direction);
                selector
            });
        directions.push(DirectionSource::Selector(selector));
    }
    if selectors.len() > MAX_DYNAMIC_ORDER_SELECTORS {
        return Err(PlanError::new(format!(
            "typed order uses {} independent dynamic directions; maximum is {} so every direction vector can remain preindexed",
            selectors.len(),
            MAX_DYNAMIC_ORDER_SELECTORS
        )));
    }

    let variant_count = 1usize << selectors.len();
    let mut variants = Vec::with_capacity(variant_count);
    for mask in 0..variant_count {
        let selector_directions = (0..selectors.len())
            .map(|selector| {
                if mask & (1usize << selector) == 0 {
                    PlanOrderDirection::Ascending
                } else {
                    PlanOrderDirection::Descending
                }
            })
            .collect::<Vec<_>>();
        let resolved = keys
            .iter()
            .zip(&directions)
            .map(|(key, direction)| OrderedKeyCandidate {
                owner: key.owner,
                row_local: key.row_local,
                expression: key.expression,
                direction: match direction {
                    DirectionSource::Static(direction) => *direction,
                    DirectionSource::Selector(selector) => selector_directions[*selector],
                },
                multiplicity: PlanListIndexKeyMultiplicity::One,
            })
            .collect();
        variants.push((selector_directions, resolved));
    }
    Ok(ResolvedOrderDirectionVariants {
        selectors,
        key_variants: variants,
    })
}

fn build_directional_access_expression(
    arena: &mut PlanRowExpressionArena,
    accesses: PlannedListAccessSet,
    mut make_leaf: impl FnMut(PlanListAccess) -> PlanRowExpressionNode,
) -> Result<PlanRowExpressionId, PlanError> {
    let leaves = accesses
        .variants
        .into_iter()
        .map(|(directions, access)| Ok((directions, arena.intern(make_leaf(access))?)))
        .collect::<Result<Vec<_>, PlanError>>()?;

    fn build(
        arena: &mut PlanRowExpressionArena,
        selectors: &[PlanRowExpressionId],
        depth: usize,
        leaves: &[(Vec<PlanOrderDirection>, PlanRowExpressionId)],
    ) -> Result<PlanRowExpressionId, PlanError> {
        if depth == selectors.len() {
            let [(_, value)] = leaves else {
                return Err(PlanError::new(
                    "dynamic typed order did not resolve to one physical access variant",
                ));
            };
            return Ok(*value);
        }
        let mut arms = Vec::with_capacity(2);
        for (direction, label) in [
            (PlanOrderDirection::Ascending, "Ascending"),
            (PlanOrderDirection::Descending, "Descending"),
        ] {
            let selected = leaves
                .iter()
                .filter(|(directions, _)| directions.get(depth) == Some(&direction))
                .cloned()
                .collect::<Vec<_>>();
            arms.push(PlanRowSelectArm {
                pattern: PlanRowSelectPattern::Tag {
                    name: label.to_owned(),
                },
                value: build(arena, selectors, depth + 1, &selected)?,
            });
        }
        arena.intern(PlanRowExpressionNode::Select {
            input: selectors[depth],
            arms,
        })
    }

    build(arena, &accesses.selectors, 0, &leaves)
}

fn plan_typed_list_index_key(
    program: &ErasedProgram,
    value_index: &ValueIndex,
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    expression: PlanRowExpressionId,
    direction: PlanOrderDirection,
    multiplicity: PlanListIndexKeyMultiplicity,
) -> Result<Option<PlanListIndexKey>, PlanError> {
    let (kind, closed_tags) = match multiplicity {
        PlanListIndexKeyMultiplicity::One => {
            match row_expression_value_type(program, value_index, arena, constants, expression)? {
                Some(PlanValueType::Number) => (PlanListIndexKeyKind::Number, Vec::new()),
                Some(PlanValueType::Text) => (PlanListIndexKeyKind::Text, Vec::new()),
                Some(PlanValueType::Tag) => {
                    let Some(tags) = closed_tag_index_tags(value_index, arena, expression)? else {
                        return Ok(None);
                    };
                    let Some(type_id) = closed_tag_type_id(&tags) else {
                        return Ok(None);
                    };
                    (PlanListIndexKeyKind::ClosedTag { type_id }, tags)
                }
                _ => return Ok(None),
            }
        }
        PlanListIndexKeyMultiplicity::ListItems { .. } => {
            let Some(DataTypePlan::List { item }) =
                row_expression_data_type(value_index, arena, expression)?
            else {
                return Ok(None);
            };
            match item.as_ref() {
                DataTypePlan::Number => (PlanListIndexKeyKind::Number, Vec::new()),
                DataTypePlan::Text => (PlanListIndexKeyKind::Text, Vec::new()),
                data_type @ DataTypePlan::Variant { .. } => {
                    let Some(tags) = closed_tag_index_tags_for_data_type(data_type) else {
                        return Ok(None);
                    };
                    let Some(type_id) = closed_tag_type_id(&tags) else {
                        return Ok(None);
                    };
                    (PlanListIndexKeyKind::ClosedTag { type_id }, tags)
                }
                DataTypePlan::Bytes { .. }
                | DataTypePlan::Bits { .. }
                | DataTypePlan::Record { .. }
                | DataTypePlan::List { .. }
                | DataTypePlan::Map { .. }
                | DataTypePlan::Set { .. }
                | DataTypePlan::Union { .. }
                | DataTypePlan::Unknown => return Ok(None),
            }
        }
    };
    Ok(Some(PlanListIndexKey {
        owner,
        row_local,
        expression,
        kind,
        closed_tags,
        direction,
        multiplicity,
    }))
}

fn ensure_typed_list_index(
    arena: &mut PlanRowExpressionArena,
    source_list: ListId,
    keys: Vec<PlanListIndexKey>,
    indexes: &mut Vec<PlanListIndex>,
) -> Result<PlanListIndexId, PlanError> {
    for index in indexes.iter() {
        if index.source_list == source_list
            && typed_list_index_keys_equivalent(arena, &index.keys, &keys)?
        {
            return Ok(index.id);
        }
    }
    let id = PlanListIndexId(indexes.len());
    indexes.push(PlanListIndex {
        id,
        source_list,
        keys,
    });
    Ok(id)
}

fn typed_list_index_keys_equivalent(
    arena: &mut PlanRowExpressionArena,
    left: &[PlanListIndexKey],
    right: &[PlanListIndexKey],
) -> Result<bool, PlanError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        if left.kind != right.kind
            || left.closed_tags != right.closed_tags
            || left.direction != right.direction
            || left.multiplicity != right.multiplicity
            || remap_row_expression_contextual_local(
                arena,
                right.expression,
                (right.owner, right.row_local),
                (left.owner, left.row_local),
            )? != Some(left.expression)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn closed_tag_index_tags(
    value_index: &ValueIndex,
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
) -> Result<Option<Vec<String>>, PlanError> {
    let data_type = match arena.node(expression)? {
        PlanRowExpressionNode::Field {
            input: ValueRef::Field(field),
        }
        | PlanRowExpressionNode::ListRowField { field, .. } => {
            let Some(data_type) = value_index.field_data_type(*field) else {
                return Ok(None);
            };
            data_type
        }
        _ => return Ok(None),
    };
    Ok(closed_tag_index_tags_for_data_type(data_type))
}

fn row_expression_data_type<'a>(
    value_index: &'a ValueIndex,
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
) -> Result<Option<&'a DataTypePlan>, PlanError> {
    Ok(match arena.node(expression)? {
        PlanRowExpressionNode::Field {
            input: ValueRef::Field(field),
        }
        | PlanRowExpressionNode::ListRowField { field, .. } => value_index.field_data_type(*field),
        _ => None,
    })
}

fn closed_tag_index_tags_for_data_type(data_type: &DataTypePlan) -> Option<Vec<String>> {
    let DataTypePlan::Variant { variants } = data_type else {
        return None;
    };
    if variants
        .iter()
        .any(|variant| variant.open || !variant.fields.is_empty())
    {
        return None;
    }
    let mut tags = variants
        .iter()
        .map(|variant| variant.tag.clone())
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();
    (tags.len() == variants.len()).then_some(tags)
}

fn collect_access_view(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    keys: &mut Vec<UnresolvedOrderedKeyCandidate>,
    filters: &mut Vec<PlanListFilter>,
) -> Result<Option<ListId>, PlanError> {
    Ok(match arena.node(expression)? {
        PlanRowExpressionNode::ListRef { list_id }
        | PlanRowExpressionNode::AuthorityListRef { list_id } => Some(*list_id),
        PlanRowExpressionNode::ContextualCollection {
            owner,
            operation: PlanContextualOperationKind::Filter,
            source,
            row_local,
            body,
            ..
        } => {
            let Some(source_list) = collect_access_view(arena, *source, keys, filters)? else {
                return Ok(None);
            };
            filters.push(PlanListFilter {
                owner: *owner,
                row_local: *row_local,
                predicate: *body,
            });
            Some(source_list)
        }
        PlanRowExpressionNode::ContextualOrder {
            owner,
            operation,
            source,
            row_local,
            key,
            direction,
        } => {
            let Some(source_list) = collect_access_view(arena, *source, keys, filters)? else {
                return Ok(None);
            };
            if *operation == PlanOrderOperationKind::SortBy {
                keys.clear();
            } else if keys.is_empty() {
                return Ok(None);
            }
            keys.push(UnresolvedOrderedKeyCandidate {
                owner: *owner,
                row_local: *row_local,
                expression: *key,
                direction: *direction,
            });
            Some(source_list)
        }
        _ => None,
    })
}

fn static_order_direction(
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: PlanRowExpressionId,
) -> Result<Option<PlanOrderDirection>, PlanError> {
    let PlanRowExpressionNode::Constant { constant_id } = arena.node(expression)? else {
        return Ok(None);
    };
    let Some(value) = constants
        .iter()
        .find(|constant| constant.id == *constant_id)
        .map(|constant| &constant.value)
    else {
        return Ok(None);
    };
    Ok(match value {
        PlanConstantValue::Tag { name } if name == "Ascending" => {
            Some(PlanOrderDirection::Ascending)
        }
        PlanConstantValue::Tag { name } if name == "Descending" => {
            Some(PlanOrderDirection::Descending)
        }
        _ => None,
    })
}

#[derive(Clone)]
enum FilterSeekAtom {
    Exact {
        key: PlanRowExpressionId,
        value: PlanRowExpressionId,
    },
    TextPrefix {
        key: PlanRowExpressionId,
        prefix: PlanRowExpressionId,
    },
    Range {
        key: PlanRowExpressionId,
        operator: PlanInfixOp,
        value: PlanRowExpressionId,
    },
    ListMembership {
        key: PlanRowExpressionId,
        value: PlanRowExpressionId,
    },
}

#[derive(Clone)]
struct ScopedFilterSeekAtom {
    local: (PlanStaticOwnerId, PlanLocalId),
    atom: FilterSeekAtom,
}

impl ScopedFilterSeekAtom {
    fn key(&self) -> PlanRowExpressionId {
        match &self.atom {
            FilterSeekAtom::Exact { key, .. }
            | FilterSeekAtom::TextPrefix { key, .. }
            | FilterSeekAtom::Range { key, .. }
            | FilterSeekAtom::ListMembership { key, .. } => *key,
        }
    }
}

fn plan_filter_access_selection(
    arena: &mut PlanRowExpressionArena,
    filters: &[PlanListFilter],
    keys: &mut Vec<OrderedKeyCandidate>,
) -> Result<Option<(PlanListAccessSelection, Option<PlanRowExpressionId>)>, PlanError> {
    let explicit_order = keys.clone();
    let mut conjunctive_atoms = Vec::new();
    let mut disjunctions = Vec::new();
    for filter in filters {
        let local = (filter.owner, filter.row_local);
        if boolean_call_parts(arena, filter.predicate, PlanRowBuiltin::BoolOr)?.is_some() {
            let mut parts = Vec::new();
            flatten_boolean_call(arena, filter.predicate, PlanRowBuiltin::BoolOr, &mut parts)?;
            let mut atoms = Vec::with_capacity(parts.len());
            for part in parts {
                let Some(atom) = scoped_filter_seek_atom(arena, part, local)? else {
                    return Ok(None);
                };
                atoms.push(atom);
            }
            if atoms.len() < 2 {
                return Ok(None);
            }
            disjunctions.push(atoms);
            continue;
        }
        let mut parts = Vec::new();
        flatten_boolean_call(arena, filter.predicate, PlanRowBuiltin::BoolAnd, &mut parts)?;
        for part in parts {
            if let Some(atom) = scoped_filter_seek_atom(arena, part, local)? {
                conjunctive_atoms.push(atom);
            }
        }
    }

    let exact = conjunctive_atoms
        .iter()
        .filter_map(|atom| match &atom.atom {
            FilterSeekAtom::Exact { key, value } => Some((atom.local, *key, *value)),
            FilterSeekAtom::TextPrefix { .. }
            | FilterSeekAtom::Range { .. }
            | FilterSeekAtom::ListMembership { .. } => None,
        })
        .collect::<Vec<_>>();
    let memberships = conjunctive_atoms
        .iter()
        .filter(|atom| matches!(atom.atom, FilterSeekAtom::ListMembership { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let bounded = conjunctive_atoms
        .iter()
        .filter(|atom| {
            !matches!(
                &atom.atom,
                FilterSeekAtom::Exact { .. } | FilterSeekAtom::ListMembership { .. }
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if exact.is_empty() && memberships.is_empty() && bounded.is_empty() && disjunctions.is_empty() {
        return Ok(None);
    }

    let membership_atoms = memberships
        .iter()
        .chain(disjunctions.iter().flatten())
        .filter(|atom| matches!(atom.atom, FilterSeekAtom::ListMembership { .. }))
        .collect::<Vec<_>>();
    let membership_key = membership_atoms.first().copied();
    if let Some(first) = membership_key {
        for candidate in membership_atoms.iter().skip(1) {
            if remap_row_expression_contextual_local(
                arena,
                candidate.key(),
                candidate.local,
                first.local,
            )? != Some(first.key())
            {
                return Ok(None);
            }
        }
    }

    let leading = exact.iter().map(|(_, _, value)| *value).collect::<Vec<_>>();
    let mut access_order = explicit_order.clone();
    if let Some(membership) = membership_key {
        access_order.insert(
            0,
            OrderedKeyCandidate {
                owner: membership.local.0,
                row_local: membership.local.1,
                expression: membership.key(),
                direction: PlanOrderDirection::Ascending,
                multiplicity: PlanListIndexKeyMultiplicity::ListItems {
                    max_items: MAX_TYPED_LIST_EXPANDED_KEYS_PER_ROW,
                },
            },
        );
    }
    if access_order.is_empty()
        && let Some(primary) = bounded
            .first()
            .or_else(|| disjunctions.first().and_then(|atoms| atoms.first()))
    {
        access_order.push(OrderedKeyCandidate {
            owner: primary.local.0,
            row_local: primary.local.1,
            expression: primary.key(),
            direction: PlanOrderDirection::Ascending,
            multiplicity: PlanListIndexKeyMultiplicity::One,
        });
    }
    if !exact.is_empty() {
        let mut prefixed = exact
            .iter()
            .map(|(local, key, _)| OrderedKeyCandidate {
                owner: local.0,
                row_local: local.1,
                expression: *key,
                direction: PlanOrderDirection::Ascending,
                multiplicity: PlanListIndexKeyMultiplicity::One,
            })
            .collect::<Vec<_>>();
        prefixed.extend(access_order.iter().cloned());
        *keys = prefixed;
    } else if (membership_key.is_some() || explicit_order.is_empty()) && !access_order.is_empty() {
        *keys = access_order.clone();
    }

    let selection = if memberships.is_empty() && bounded.is_empty() && disjunctions.is_empty() {
        PlanListAccessSelection::KeyPrefix { values: leading }
    } else {
        let Some(first_order) = access_order.first() else {
            return Ok(None);
        };
        let mut matching_bounded = Vec::new();
        for atom in bounded {
            if filter_atom_matches_order_key(arena, &atom, first_order)? {
                matching_bounded.push(atom);
            }
        }
        let Some(mut branches) = memberships
            .iter()
            .map(|atom| selection_for_filter_atom(atom, leading.clone(), first_order.direction))
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        let Some(bounded_branches) =
            selections_for_bounded_atoms(&matching_bounded, leading.clone(), first_order.direction)
        else {
            return Ok(None);
        };
        branches.extend(bounded_branches);
        for atoms in disjunctions {
            let mut all_match = true;
            for atom in &atoms {
                all_match &= filter_atom_matches_order_key(arena, atom, first_order)?;
            }
            if !all_match {
                continue;
            }
            let Some(union) = atoms
                .iter()
                .map(|atom| selection_for_filter_atom(atom, leading.clone(), first_order.direction))
                .collect::<Option<Vec<_>>>()
            else {
                return Ok(None);
            };
            branches.push(PlanListAccessSelection::Union { branches: union });
        }
        match branches.as_slice() {
            [] if !leading.is_empty() => PlanListAccessSelection::KeyPrefix { values: leading },
            [] => return Ok(None),
            [selection] => selection.clone(),
            _ => PlanListAccessSelection::Intersection { branches },
        }
    };
    let mut guard = None;
    for filter in filters {
        if let Some(expression) = find_row_independent_conjunct(
            arena,
            filter.predicate,
            (filter.owner, filter.row_local),
        )? {
            guard = Some(expression);
            break;
        }
    }
    Ok(Some((selection, guard)))
}

fn selections_for_bounded_atoms(
    atoms: &[ScopedFilterSeekAtom],
    leading: Vec<PlanRowExpressionId>,
    direction: PlanOrderDirection,
) -> Option<Vec<PlanListAccessSelection>> {
    let semantic_lower = atoms.iter().position(|atom| {
        matches!(
            &atom.atom,
            FilterSeekAtom::Range { operator, .. }
                if matches!(
                    operator,
                    PlanInfixOp::Greater | PlanInfixOp::GreaterOrEqual
                )
        )
    });
    let semantic_upper = atoms.iter().position(|atom| {
        matches!(
            &atom.atom,
            FilterSeekAtom::Range { operator, .. }
                if matches!(operator, PlanInfixOp::Less | PlanInfixOp::LessOrEqual)
        )
    });
    let paired = semantic_lower.zip(semantic_upper);
    let mut selections = Vec::new();
    if let Some((lower, upper)) = paired {
        selections.push(selection_for_range_pair(
            &atoms[lower],
            &atoms[upper],
            leading.clone(),
            direction,
        )?);
    }
    for (position, atom) in atoms.iter().enumerate() {
        if paired.is_some_and(|(lower, upper)| position == lower || position == upper) {
            continue;
        }
        selections.push(selection_for_filter_atom(atom, leading.clone(), direction)?);
    }
    Some(selections)
}

fn selection_for_range_pair(
    semantic_lower: &ScopedFilterSeekAtom,
    semantic_upper: &ScopedFilterSeekAtom,
    leading: Vec<PlanRowExpressionId>,
    direction: PlanOrderDirection,
) -> Option<PlanListAccessSelection> {
    let FilterSeekAtom::Range {
        operator: lower_operator,
        value: lower_value,
        ..
    } = &semantic_lower.atom
    else {
        return None;
    };
    let FilterSeekAtom::Range {
        operator: upper_operator,
        value: upper_value,
        ..
    } = &semantic_upper.atom
    else {
        return None;
    };
    if !matches!(
        lower_operator,
        PlanInfixOp::Greater | PlanInfixOp::GreaterOrEqual
    ) || !matches!(upper_operator, PlanInfixOp::Less | PlanInfixOp::LessOrEqual)
    {
        return None;
    }
    let lower = PlanListAccessBound {
        value: *lower_value,
        inclusive: *lower_operator == PlanInfixOp::GreaterOrEqual,
    };
    let upper = PlanListAccessBound {
        value: *upper_value,
        inclusive: *upper_operator == PlanInfixOp::LessOrEqual,
    };
    let (lower, upper) = if direction == PlanOrderDirection::Ascending {
        (lower, upper)
    } else {
        (upper, lower)
    };
    Some(PlanListAccessSelection::ComponentRange {
        leading,
        lower: Some(lower),
        upper: Some(upper),
    })
}

fn selection_for_filter_atom(
    atom: &ScopedFilterSeekAtom,
    leading: Vec<PlanRowExpressionId>,
    direction: PlanOrderDirection,
) -> Option<PlanListAccessSelection> {
    match &atom.atom {
        FilterSeekAtom::Exact { value, .. } => {
            let mut values = leading;
            values.push(*value);
            Some(PlanListAccessSelection::KeyPrefix { values })
        }
        FilterSeekAtom::ListMembership { value, .. } => {
            let mut values = leading;
            values.push(*value);
            Some(PlanListAccessSelection::KeyPrefix { values })
        }
        FilterSeekAtom::TextPrefix { prefix, .. } => Some(PlanListAccessSelection::TextPrefix {
            leading,
            prefix: *prefix,
        }),
        FilterSeekAtom::Range {
            operator, value, ..
        } => {
            let bound = PlanListAccessBound {
                value: *value,
                inclusive: matches!(
                    operator,
                    PlanInfixOp::GreaterOrEqual | PlanInfixOp::LessOrEqual
                ),
            };
            let ascending = direction == PlanOrderDirection::Ascending;
            let semantic_lower =
                matches!(operator, PlanInfixOp::Greater | PlanInfixOp::GreaterOrEqual);
            let directed_lower = if ascending {
                semantic_lower
            } else {
                !semantic_lower
            };
            Some(PlanListAccessSelection::ComponentRange {
                leading,
                lower: directed_lower.then_some(bound.clone()),
                upper: (!directed_lower).then_some(bound),
            })
        }
    }
}

fn filter_atom_matches_order_key(
    arena: &mut PlanRowExpressionArena,
    atom: &ScopedFilterSeekAtom,
    order: &OrderedKeyCandidate,
) -> Result<bool, PlanError> {
    Ok(remap_row_expression_contextual_local(
        arena,
        atom.key(),
        atom.local,
        (order.owner, order.row_local),
    )? == Some(order.expression))
}

fn scoped_filter_seek_atom(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    local: (PlanStaticOwnerId, PlanLocalId),
) -> Result<Option<ScopedFilterSeekAtom>, PlanError> {
    Ok(
        filter_seek_atom(arena, expression, local)?
            .map(|atom| ScopedFilterSeekAtom { local, atom }),
    )
}

fn filter_seek_atom(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    local: (PlanStaticOwnerId, PlanLocalId),
) -> Result<Option<FilterSeekAtom>, PlanError> {
    if let Some(atom) = list_membership_filter_seek_atom(arena, expression, local)? {
        return Ok(Some(atom));
    }
    if let PlanRowExpressionNode::TextStartsWith { input, prefix } = arena.node(expression)?
        && arena.references_contextual_local(*input, local.0, local.1)?
        && !arena.references_contextual_local(*prefix, local.0, local.1)?
    {
        return Ok(Some(FilterSeekAtom::TextPrefix {
            key: *input,
            prefix: *prefix,
        }));
    }
    let PlanRowExpressionNode::NumberInfix { op, left, right } = arena.node(expression)? else {
        return Ok(None);
    };
    if !matches!(
        op,
        PlanInfixOp::Equal
            | PlanInfixOp::Greater
            | PlanInfixOp::GreaterOrEqual
            | PlanInfixOp::Less
            | PlanInfixOp::LessOrEqual
    ) {
        return Ok(None);
    }
    let left_row = arena.references_contextual_local(*left, local.0, local.1)?;
    let right_row = arena.references_contextual_local(*right, local.0, local.1)?;
    if left_row == right_row {
        return Ok(None);
    }
    let (key, value, operator) = if left_row {
        (*left, *right, *op)
    } else {
        let Some(operator) = reverse_comparison_operator(*op) else {
            return Ok(None);
        };
        (*right, *left, operator)
    };
    if operator == PlanInfixOp::Equal {
        Ok(Some(FilterSeekAtom::Exact { key, value }))
    } else {
        Ok(Some(FilterSeekAtom::Range {
            key,
            operator,
            value,
        }))
    }
}

fn list_membership_filter_seek_atom(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    outer_local: (PlanStaticOwnerId, PlanLocalId),
) -> Result<Option<FilterSeekAtom>, PlanError> {
    let PlanRowExpressionNode::ContextualCollection {
        owner,
        operation: PlanContextualOperationKind::Any,
        source,
        row_local,
        body,
        captures,
        indexed_access: None,
    } = arena.node(expression)?
    else {
        return Ok(None);
    };
    if !captures.is_empty()
        || !arena.references_contextual_local(*source, outer_local.0, outer_local.1)?
        || arena.references_contextual_local(*source, *owner, *row_local)?
    {
        return Ok(None);
    }
    let PlanRowExpressionNode::NumberInfix { op, left, right } = arena.node(*body)? else {
        return Ok(None);
    };
    if *op != PlanInfixOp::Equal {
        return Ok(None);
    }
    let is_item = |candidate: PlanRowExpressionId| -> Result<bool, PlanError> {
        Ok(matches!(
            arena.node(candidate)?,
            PlanRowExpressionNode::Local {
                owner: candidate_owner,
                local: candidate_local,
                ..
            } if *candidate_owner == *owner && *candidate_local == *row_local
        ))
    };
    let value = if is_item(*left)?
        && !arena.references_contextual_local(*right, *owner, *row_local)?
    {
        *right
    } else if is_item(*right)? && !arena.references_contextual_local(*left, *owner, *row_local)? {
        *left
    } else {
        return Ok(None);
    };
    if arena.references_contextual_local(value, outer_local.0, outer_local.1)? {
        return Ok(None);
    }
    Ok(Some(FilterSeekAtom::ListMembership {
        key: *source,
        value,
    }))
}

fn reverse_comparison_operator(operator: PlanInfixOp) -> Option<PlanInfixOp> {
    Some(match operator {
        PlanInfixOp::Equal => PlanInfixOp::Equal,
        PlanInfixOp::Greater => PlanInfixOp::Less,
        PlanInfixOp::GreaterOrEqual => PlanInfixOp::LessOrEqual,
        PlanInfixOp::Less => PlanInfixOp::Greater,
        PlanInfixOp::LessOrEqual => PlanInfixOp::GreaterOrEqual,
        _ => return None,
    })
}

fn boolean_call_parts(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    builtin: PlanRowBuiltin,
) -> Result<Option<Vec<PlanRowExpressionId>>, PlanError> {
    let PlanRowExpressionNode::BuiltinCall {
        function,
        input,
        args,
    } = arena.node(expression)?
    else {
        return Ok(None);
    };
    Ok((*function == builtin).then(|| {
        input
            .iter()
            .copied()
            .chain(args.iter().map(|argument| argument.value))
            .collect()
    }))
}

fn flatten_boolean_call(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    builtin: PlanRowBuiltin,
    output: &mut Vec<PlanRowExpressionId>,
) -> Result<(), PlanError> {
    let Some(parts) = boolean_call_parts(arena, expression, builtin)? else {
        output.push(expression);
        return Ok(());
    };
    for part in parts {
        flatten_boolean_call(arena, part, builtin, output)?;
    }
    Ok(())
}

fn find_row_independent_conjunct(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    local: (PlanStaticOwnerId, PlanLocalId),
) -> Result<Option<PlanRowExpressionId>, PlanError> {
    if !arena.references_contextual_local(expression, local.0, local.1)? {
        return Ok(Some(expression));
    }
    let parts = match arena.node(expression)? {
        PlanRowExpressionNode::BuiltinCall {
            function: PlanRowBuiltin::BoolAnd,
            input,
            args,
        } => input
            .iter()
            .copied()
            .chain(args.iter().map(|argument| argument.value))
            .collect::<Vec<_>>(),
        _ => return Ok(None),
    };
    for part in parts {
        if let Some(independent) = find_row_independent_conjunct(arena, part, local)? {
            return Ok(Some(independent));
        }
    }
    Ok(None)
}

fn materialized_list_row_field_copies(
    program: &ErasedProgram,
    target_list: ListId,
    expression: &PlanDerivedExpression,
    arena: &PlanRowExpressionArena,
    list_indexes: &[PlanListIndex],
) -> Result<Vec<PlanMaterializedRowFieldCopy>, PlanError> {
    let mut source_lists = BTreeSet::new();
    collect_materialized_row_sources(expression, arena, list_indexes, &mut source_lists)?;
    if source_lists.is_empty() {
        return Ok(Vec::new());
    }
    let target_fields = materialized_copy_target_fields_by_name(program, target_list);
    let mut copies = Vec::new();
    for source_list in source_lists {
        let source_fields = materialized_copy_source_fields_by_name(program, source_list);
        let before = copies.len();
        for (name, target_field) in &target_fields {
            if let Some(source_field) = source_fields.get(name) {
                copies.push(PlanMaterializedRowFieldCopy {
                    source_list,
                    source_field: *source_field,
                    target_field: *target_field,
                });
            }
        }
        if copies.len() == before && !target_fields.is_empty() {
            return Err(PlanError::new(format!(
                "materialized list {} cannot copy typed rows from list {}",
                target_list.0, source_list.0
            )));
        }
    }
    Ok(copies)
}

fn top_level_materialized_data_field_names(
    program: &ErasedProgram,
    list_id: ListId,
) -> BTreeSet<String> {
    let Some(list) = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)
    else {
        return BTreeSet::new();
    };
    program
        .scope_index
        .fields
        .iter()
        .filter(|field| {
            erased_field_is_runtime_row_storage(program, field)
                && field.row.map(|row| row.list) == Some(list.id)
                && erased_field_is_direct_list_member(field)
                && !erased_field_contains_resource(program, field)
        })
        .map(|field| field.name.clone())
        .collect()
}

fn erased_field_is_direct_list_member(field: &ir::ErasedFieldDef) -> bool {
    field.row_path.len() == 1
}

fn erased_field_contains_resource(program: &ErasedProgram, field: &ir::ErasedFieldDef) -> bool {
    let prefix = format!("{}.", field.diagnostic_path);
    program
        .sources
        .iter()
        .any(|source| source.path == field.diagnostic_path || source.path.starts_with(&prefix))
}

fn materialized_copy_source_fields_by_name(
    program: &ErasedProgram,
    list_id: ListId,
) -> BTreeMap<String, FieldId> {
    let Some(list) = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)
    else {
        return BTreeMap::new();
    };
    let authority_fields = list_authority_field_ids(program);
    top_level_materialized_data_field_names(program, list_id)
        .into_iter()
        .filter_map(|name| {
            storage_input_field_id(program, &list.name, &name, &authority_fields)
                .map(|field| (name, field))
        })
        .collect()
}

fn materialized_copy_target_fields_by_name(
    program: &ErasedProgram,
    list_id: ListId,
) -> BTreeMap<String, FieldId> {
    let Some(list) = program
        .lists
        .iter()
        .find(|list| plan_list_id(list.id) == list_id)
    else {
        return BTreeMap::new();
    };
    let authority_fields = list_authority_field_ids(program);
    top_level_materialized_data_field_names(program, list_id)
        .into_iter()
        .filter_map(|name| {
            storage_input_field_id(program, &list.name, &name, &authority_fields)
                .map(|field| (name, field))
        })
        .collect()
}

fn collect_materialized_row_sources(
    expression: &PlanDerivedExpression,
    arena: &PlanRowExpressionArena,
    list_indexes: &[PlanListIndex],
    sources: &mut BTreeSet<ListId>,
) -> Result<(), PlanError> {
    match expression {
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            collect_row_result_sources(arena, *expression, list_indexes, sources)?;
        }
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            collect_row_result_sources(arena, *default, list_indexes, sources)?;
            for arm in arms {
                collect_row_result_sources(arena, arm.value, list_indexes, sources)?;
            }
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
    Ok(())
}

fn collect_materialized_iteration_sources(
    expression: &PlanDerivedExpression,
    arena: &PlanRowExpressionArena,
    list_indexes: &[PlanListIndex],
    sources: &mut BTreeSet<ListId>,
) -> Result<(), PlanError> {
    match expression {
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            collect_row_iteration_sources(arena, *expression, list_indexes, sources)?;
        }
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            collect_row_iteration_sources(arena, *default, list_indexes, sources)?;
            for arm in arms {
                collect_row_iteration_sources(arena, arm.value, list_indexes, sources)?;
            }
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => {}
    }
    Ok(())
}

fn collect_row_iteration_sources(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    list_indexes: &[PlanListIndex],
    sources: &mut BTreeSet<ListId>,
) -> Result<(), PlanError> {
    match arena.node(expression)? {
        PlanRowExpressionNode::ListAccess { access } => {
            let index = list_indexes
                .get(access.index.0)
                .filter(|index| index.id == access.index)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "typed list access references missing index {} while deriving iteration sources",
                        access.index.0
                    ))
                })?;
            sources.insert(index.source_list);
        }
        PlanRowExpressionNode::ContextualCollection { source, .. }
        | PlanRowExpressionNode::ContextualOrder { source, .. } => {
            collect_row_result_sources(arena, *source, list_indexes, sources)?;
        }
        PlanRowExpressionNode::BuiltinCall {
            function,
            input: Some(input),
            ..
        } if *function == PlanRowBuiltin::ListTake => {
            collect_row_iteration_sources(arena, *input, list_indexes, sources)?;
        }
        PlanRowExpressionNode::Select { arms, .. } => {
            for arm in arms {
                collect_row_iteration_sources(arena, arm.value, list_indexes, sources)?;
            }
        }
        _ => collect_row_result_sources(arena, expression, list_indexes, sources)?,
    }
    Ok(())
}

fn derived_expression_reads_authority_list(
    expression: &PlanDerivedExpression,
    list_id: ListId,
    arena: &PlanRowExpressionArena,
) -> Result<bool, PlanError> {
    match expression {
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            arena.reads_authority_list(*expression, list_id)
        }
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            if arena.reads_authority_list(*default, list_id)? {
                return Ok(true);
            }
            for arm in arms {
                if arena.reads_authority_list(arm.value, list_id)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }
        | PlanDerivedExpression::BoolNot { .. }
        | PlanDerivedExpression::NumberCompareConst { .. }
        | PlanDerivedExpression::ValueCompare { .. } => Ok(false),
    }
}

fn collect_row_result_sources(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    list_indexes: &[PlanListIndex],
    sources: &mut BTreeSet<ListId>,
) -> Result<(), PlanError> {
    match arena.node(expression)? {
        PlanRowExpressionNode::ListRef { list_id }
        | PlanRowExpressionNode::AuthorityListRef { list_id } => {
            sources.insert(*list_id);
        }
        PlanRowExpressionNode::ListAccess { access } => {
            let index = list_indexes
                .get(access.index.0)
                .filter(|index| index.id == access.index)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "typed list access references missing index {} while deriving row copies",
                        access.index.0
                    ))
                })?;
            let mut forwards_source = true;
            for map in &access.maps {
                if !map_body_may_forward_local(arena, map.owner, map.row_local, map.body)? {
                    forwards_source = false;
                    break;
                }
            }
            if forwards_source {
                sources.insert(index.source_list);
            }
        }
        PlanRowExpressionNode::ContextualCollection {
            operation:
                PlanContextualOperationKind::Filter
                | PlanContextualOperationKind::Retain
                | PlanContextualOperationKind::Remove,
            source,
            ..
        } => collect_row_result_sources(arena, *source, list_indexes, sources)?,
        PlanRowExpressionNode::ContextualOrder { source, .. } => {
            collect_row_result_sources(arena, *source, list_indexes, sources)?;
        }
        PlanRowExpressionNode::BuiltinCall {
            function,
            input: Some(input),
            ..
        } if *function == PlanRowBuiltin::ListTake => {
            collect_row_result_sources(arena, *input, list_indexes, sources)?;
        }
        PlanRowExpressionNode::ContextualCollection {
            owner,
            operation: PlanContextualOperationKind::Map,
            source,
            row_local,
            body,
            ..
        } => collect_mapped_row_result_sources(
            arena,
            *owner,
            *row_local,
            *source,
            *body,
            list_indexes,
            sources,
        )?,
        PlanRowExpressionNode::Select { arms, .. } => {
            for arm in arms {
                collect_row_result_sources(arena, arm.value, list_indexes, sources)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn map_body_may_forward_local(
    arena: &PlanRowExpressionArena,
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    expression: PlanRowExpressionId,
) -> Result<bool, PlanError> {
    match arena.node(expression)? {
        PlanRowExpressionNode::LocalRow {
            owner: body_owner,
            local,
        } => Ok(*body_owner == owner && *local == row_local),
        PlanRowExpressionNode::Select { arms, .. } => {
            for arm in arms {
                if map_body_may_forward_local(arena, owner, row_local, arm.value)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn collect_mapped_row_result_sources(
    arena: &PlanRowExpressionArena,
    owner: PlanStaticOwnerId,
    row_local: PlanLocalId,
    source: PlanRowExpressionId,
    body: PlanRowExpressionId,
    list_indexes: &[PlanListIndex],
    sources: &mut BTreeSet<ListId>,
) -> Result<(), PlanError> {
    match arena.node(body)? {
        PlanRowExpressionNode::LocalRow {
            owner: body_owner,
            local,
        } if *body_owner == owner && *local == row_local => {
            collect_row_result_sources(arena, source, list_indexes, sources)?;
        }
        PlanRowExpressionNode::Select { arms, .. } => {
            for arm in arms {
                collect_mapped_row_result_sources(
                    arena,
                    owner,
                    row_local,
                    source,
                    arm.value,
                    list_indexes,
                    sources,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_materialized_list_row_names(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    names: &mut BTreeSet<String>,
) -> Result<(), PlanError> {
    match arena.node(expression)? {
        PlanRowExpressionNode::Object { .. } | PlanRowExpressionNode::TaggedObject { .. } => {
            collect_materialized_record_field_names(arena, expression, names)?;
        }
        PlanRowExpressionNode::ContextualCollection { body, .. } => {
            collect_materialized_record_field_names(arena, *body, names)?;
        }
        PlanRowExpressionNode::ContextualOrder { source, .. } => {
            collect_materialized_list_row_names(arena, *source, names)?;
        }
        PlanRowExpressionNode::ListAccess { access } => {
            collect_list_access_map_row_names(arena, &access.maps, names)?;
        }
        PlanRowExpressionNode::BuiltinCall {
            function,
            input: Some(input),
            ..
        } if *function == PlanRowBuiltin::ListTake => {
            collect_materialized_list_row_names(arena, *input, names)?;
        }
        PlanRowExpressionNode::Select { arms, .. } => {
            for arm in arms {
                collect_materialized_list_row_names(arena, arm.value, names)?;
            }
        }
        PlanRowExpressionNode::ListLiteral { items } => {
            for item in items {
                collect_materialized_record_field_names(arena, *item, names)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_list_access_map_row_names(
    arena: &PlanRowExpressionArena,
    maps: &[PlanListMap],
    names: &mut BTreeSet<String>,
) -> Result<(), PlanError> {
    let mut may_forward_previous = true;
    for map in maps.iter().rev() {
        if !may_forward_previous {
            break;
        }
        collect_materialized_record_field_names(arena, map.body, names)?;
        may_forward_previous =
            map_body_may_forward_local(arena, map.owner, map.row_local, map.body)?;
    }
    Ok(())
}

fn collect_materialized_record_field_names(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    names: &mut BTreeSet<String>,
) -> Result<(), PlanError> {
    match arena.node(expression)? {
        PlanRowExpressionNode::Object { fields }
        | PlanRowExpressionNode::TaggedObject { fields, .. } => {
            names.extend(fields.iter().map(|field| field.name.clone()));
        }
        PlanRowExpressionNode::Select { arms, .. } => {
            for arm in arms {
                collect_materialized_record_field_names(arena, arm.value, names)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn source_event_transform_expression(
    program: &ErasedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    list_indexes: &mut Vec<PlanListIndex>,
) -> Result<PlanDerivedExpression, PlanError> {
    if derived.trigger_arms.is_empty() {
        return Err(PlanError::new(format!(
            "derived value `{}` has no exact event-owned arms",
            derived.path
        )));
    }

    let root = program
        .executable
        .expressions
        .get(derived.producer.as_usize())
        .filter(|expression| expression.id == derived.producer)
        .map(|_| derived.producer)
        .ok_or_else(|| {
            PlanError::new(format!(
                "source-event derived value `{}` has no executable statement root",
                derived.path
            ))
        })?;
    let mut local_constants = constants.clone();
    let mut local_inputs = inputs.clone();
    let mut arm_values = Vec::new();
    for arm in &derived.trigger_arms {
        let gate = program
            .executable
            .expressions
            .get(arm.gate_expression_id.as_usize())
            .filter(|expression| {
                expression.id == arm.gate_expression_id
                    && expression.checked_expr_id == arm.gate_checked_expr_id
                    && expression.owner == arm.owner
            })
            .ok_or_else(|| {
                PlanError::new(format!(
                    "source-event derived value `{}` has a stale trigger-owned gate {}",
                    derived.path, arm.gate_expression_id
                ))
            })?;
        let output_expression = program
            .executable
            .expressions
            .get(arm.output_expression_id.as_usize())
            .filter(|expression| expression.id == arm.output_expression_id)
            .cloned()
            .ok_or_else(|| {
                PlanError::new(format!(
                    "source-event derived value `{}` trigger gate {} has missing output {}",
                    derived.path, gate.id, arm.output_expression_id
                ))
            })?;
        let output = output_expression.id;
        let cause = arm.cause;
        let (trigger, cause_path) = event_cause_value_ref(program, cause)?;
        let value = ExecutableRowLowerer::new(
            program,
            index,
            arena,
            &mut local_constants,
            &mut local_inputs,
        )
        .with_list_indexes(list_indexes)
        .with_event_trigger(&trigger)
        .lower(output)
        .map_err(|error| {
            PlanError::new(format!(
                "source-event derived value `{}` trigger `{cause_path}` output {} ({:?}) failed executable lowering: {error}",
                derived.path, output, output_expression.kind
            ))
        })?;
        if !local_inputs.contains(&trigger) {
            local_inputs.push(trigger.clone());
        }
        arm_values.push((trigger, value));
    }
    if arm_values.is_empty() {
        return Err(PlanError::new(format!(
            "source-event derived value `{}` has no trigger-owned executable arm",
            derived.path
        )));
    }
    let declared_output_type = derived_value_output_type(program, derived);
    let inferred_output_type =
        source_event_transform_output_type(program, index, arena, &local_constants, &arm_values)?;
    if let (Some(declared), Some(inferred)) = (declared_output_type, inferred_output_type)
        && declared != inferred
    {
        return Err(PlanError::new(format!(
            "source-event derived value `{}` declares {declared:?} but its arms lower as {inferred:?}",
            derived.path
        )));
    }
    let output_type = declared_output_type.or(inferred_output_type);
    let mut default = None;
    for candidate in derived.default_roots.iter().copied() {
        let mut candidate_arena = arena.clone();
        let mut candidate_constants = local_constants.clone();
        let mut candidate_inputs = local_inputs.clone();
        let mut candidate_indexes = list_indexes.clone();
        let Ok(value) = ExecutableRowLowerer::new(
            program,
            index,
            &mut candidate_arena,
            &mut candidate_constants,
            &mut candidate_inputs,
        )
        .with_list_indexes(&mut candidate_indexes)
        .lower(candidate) else {
            continue;
        };
        let candidate_type = row_expression_value_type(
            program,
            index,
            &candidate_arena,
            &candidate_constants,
            value,
        )?;
        let candidate_is_absent =
            matches!(candidate_arena.node(value)?, PlanRowExpressionNode::Absent);
        if !candidate_is_absent
            && output_type.is_some_and(|expected| candidate_type != Some(expected))
        {
            continue;
        }
        *arena = candidate_arena;
        local_constants = candidate_constants;
        local_inputs = candidate_inputs;
        *list_indexes = candidate_indexes;
        default = Some(value);
        break;
    }
    let default = match default {
        Some(default) => default,
        None => arena.push(PlanRowExpressionNode::Absent)?,
    };
    let arms = arm_values
        .into_iter()
        .map(|(trigger, value)| PlanSourceEventTransformArm { trigger, value })
        .collect::<Vec<_>>();

    *constants = local_constants;
    *inputs = local_inputs;
    Ok(PlanDerivedExpression::SourceEventTransform {
        default,
        arms,
        router_route: executable_expression_calls(program, root, "Router/go_to"),
    })
}

fn event_cause_path(program: &ErasedProgram, cause: ir::EventCause) -> Option<String> {
    match cause {
        ir::EventCause::Source(source_id) => program
            .sources
            .get(source_id.as_usize())
            .filter(|source| source.id == source_id)
            .map(|source| source.path.clone()),
        ir::EventCause::State(state_id) => program
            .state_cells
            .get(state_id.as_usize())
            .filter(|state| state.id == state_id)
            .map(|state| state.path.clone()),
        ir::EventCause::Pulse(pulse_id) => program
            .pulse_batches()
            .get(pulse_id.as_usize())
            .filter(|pulse| pulse.id == pulse_id)
            .map(|_| format!("$pulse.p{}", pulse_id.as_usize())),
    }
}

fn event_cause_value_ref(
    program: &ErasedProgram,
    cause: ir::EventCause,
) -> Result<(ValueRef, String), PlanError> {
    let path = event_cause_path(program, cause)
        .ok_or_else(|| PlanError::new(format!("event cause {cause:?} has no runtime resource")))?;
    let value = match cause {
        ir::EventCause::Source(source_id) => ValueRef::Source(plan_source_id(source_id)),
        ir::EventCause::State(state_id) => ValueRef::State(plan_state_id(state_id)),
        ir::EventCause::Pulse(pulse_id) => ValueRef::Pulse(plan_pulse_batch_id(pulse_id)),
    };
    Ok((value, path))
}

fn executable_expression_calls(
    program: &ErasedProgram,
    root: ir::ExecutableExprId,
    function: &str,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        let Some(expression) = program.executable.expressions.get(current.as_usize()) else {
            continue;
        };
        if matches!(
            &expression.kind,
            ir::ExecutableExpressionKind::Call { name, .. } if name == function
        ) {
            return true;
        }
        pending.extend(executable_children(program, &expression.kind));
    }
    false
}

fn executable_children(
    program: &ErasedProgram,
    kind: &ir::ExecutableExpressionKind,
) -> Vec<ir::ExecutableExprId> {
    if let ir::ExecutableExpressionKind::Materialize { materialization } = kind {
        return program
            .materializations
            .get(*materialization)
            .map(ir::ContextualMaterialization::expression_roots)
            .unwrap_or_default();
    }
    ir::executable_expression_children(kind)
}

fn source_event_transform_output_type(
    program: &ErasedProgram,
    index: &ValueIndex,
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    arms: &[(ValueRef, PlanRowExpressionId)],
) -> Result<Option<PlanValueType>, PlanError> {
    let mut output_type = None;
    for (_, value) in arms {
        let Some(value_type) = row_expression_value_type(program, index, arena, constants, *value)?
        else {
            continue;
        };
        match output_type {
            Some(existing) if existing != value_type => return Ok(None),
            Some(_) => {}
            None => output_type = Some(value_type),
        }
    }
    Ok(output_type)
}

struct ExecutableRowLowerer<'a> {
    program: &'a ErasedProgram,
    index: &'a ValueIndex,
    arena: &'a mut PlanRowExpressionArena,
    constants: &'a mut Vec<PlanConstant>,
    inputs: &'a mut Vec<ValueRef>,
    list_indexes: Option<&'a mut Vec<PlanListIndex>>,
    event_trigger: Option<ValueRef>,
    event_row: Option<(SourceId, ListId)>,
    active_state_update: Option<ir::ExecutableStateId>,
    state_initializer: Option<ir::StateId>,
    active_materialization_owners: Vec<PlanStaticOwnerId>,
    lexical_bindings: Vec<BTreeMap<ir::ExecutableLocalBindingId, ir::ExecutableExprId>>,
    active_lexical_bindings: BTreeSet<ir::ExecutableLocalBindingId>,
    active_storage_bindings: BTreeSet<ir::ErasedBindingId>,
    bindings: BTreeMap<ir::ExecutableExprId, PlanRowExpressionId>,
    memo: BTreeMap<
        (
            ir::ExecutableExprId,
            Option<PlanStaticOwnerId>,
            Vec<PlanStaticOwnerId>,
        ),
        PlanRowExpressionId,
    >,
}

impl<'a> ExecutableRowLowerer<'a> {
    fn new(
        program: &'a ErasedProgram,
        index: &'a ValueIndex,
        arena: &'a mut PlanRowExpressionArena,
        constants: &'a mut Vec<PlanConstant>,
        inputs: &'a mut Vec<ValueRef>,
    ) -> Self {
        Self {
            program,
            index,
            arena,
            constants,
            inputs,
            list_indexes: None,
            event_trigger: None,
            event_row: None,
            active_state_update: None,
            state_initializer: None,
            active_materialization_owners: Vec::new(),
            lexical_bindings: Vec::new(),
            active_lexical_bindings: BTreeSet::new(),
            active_storage_bindings: BTreeSet::new(),
            bindings: BTreeMap::new(),
            memo: BTreeMap::new(),
        }
    }

    fn with_bindings(
        mut self,
        bindings: BTreeMap<ir::ExecutableExprId, PlanRowExpressionId>,
    ) -> Self {
        self.bindings = bindings;
        self
    }

    fn with_list_indexes(mut self, list_indexes: &'a mut Vec<PlanListIndex>) -> Self {
        self.list_indexes = Some(list_indexes);
        self
    }

    fn with_event_trigger(mut self, trigger: &ValueRef) -> Self {
        self.event_trigger = Some(trigger.clone());
        self.event_row = match trigger {
            ValueRef::Source(source_id) => self
                .program
                .sources
                .iter()
                .find(|source| plan_source_id(source.id) == *source_id)
                .and_then(|source| source.scope_id)
                .and_then(|scope| {
                    self.program
                        .lists
                        .iter()
                        .find(|list| list.row_scope_id == Some(scope))
                })
                .map(|list| (*source_id, plan_list_id(list.id))),
            _ => None,
        };
        self
    }

    fn with_materialization_owner(mut self, owner: PlanStaticOwnerId) -> Self {
        self.active_materialization_owners.push(owner);
        self
    }

    fn with_state_update(mut self, state: ir::ExecutableStateId) -> Self {
        self.active_state_update = Some(state);
        self
    }

    fn with_state_initializer(mut self, state: ir::StateId) -> Self {
        self.state_initializer = Some(state);
        self
    }

    fn materialization_captures(
        &mut self,
        materialization: &ir::ContextualMaterialization,
    ) -> Result<Vec<PlanRowCapture>, PlanError> {
        let captures = self
            .program
            .scope_index
            .locals
            .iter()
            .find(|local| {
                local.owner == materialization.owner && local.local == materialization.row_local
            })
            .map(|local| local.captures.clone())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "contextual owner {} local {} has no erased row binding",
                    materialization.owner, materialization.row_local.0
                ))
            })?;
        if captures.is_empty() {
            return Ok(Vec::new());
        }
        let mut fields = Vec::with_capacity(captures.len());
        for capture in captures {
            let erased_field = self
                .program
                .scope_index
                .fields
                .get(capture.field.as_usize())
                .filter(|field| {
                    field.id == capture.field && field.role == ir::ErasedFieldRole::Capture
                })
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "contextual owner {} local {} references missing capture field {}",
                        materialization.owner, materialization.row_local.0, capture.field
                    ))
                })?;
            fields.push(PlanRowCapture {
                field: plan_field_id(erased_field.id),
                value: self.active_local_capture_expression(&capture)?,
            });
        }
        Ok(fields)
    }

    fn materialized_value_list_authorities(
        program: &ErasedProgram,
        target_list: ListId,
        expression: &PlanDerivedExpression,
        fields: &BTreeMap<String, FieldId>,
        arena: &PlanRowExpressionArena,
    ) -> Result<Vec<PlanValueListAuthority>, PlanError> {
        let candidates = program
            .materializations
            .iter()
            .filter(|materialization| {
                materialization.source_list_id.map(plan_list_id) == Some(target_list)
                    && materialization.target_list_id.map(plan_list_id) == Some(target_list)
                    && matches!(
                        materialization.operation,
                        ir::ContextualOperationKind::Filter
                            | ir::ContextualOperationKind::Retain
                            | ir::ContextualOperationKind::Remove
                            | ir::ContextualOperationKind::SortBy
                            | ir::ContextualOperationKind::ThenBy
                    )
            })
            .map(|materialization| PlanStaticOwnerId(materialization.owner.as_usize()))
            .collect::<BTreeSet<_>>();
        let mut owners = BTreeSet::new();
        let mut visit_row = |root: PlanRowExpressionId| -> Result<(), PlanError> {
            fn walk_outer_collection_spine(
                arena: &PlanRowExpressionArena,
                expression: PlanRowExpressionId,
                candidates: &BTreeSet<PlanStaticOwnerId>,
                owners: &mut BTreeSet<PlanStaticOwnerId>,
            ) -> Result<(), PlanError> {
                match arena.node(expression)? {
                    PlanRowExpressionNode::ContextualCollection { owner, source, .. } => {
                        if candidates.contains(owner)
                            && !matches!(
                                arena.node(*source)?,
                                PlanRowExpressionNode::AuthorityListRef { .. }
                                    | PlanRowExpressionNode::ListRef { .. }
                                    | PlanRowExpressionNode::ContextualCollection { .. }
                                    | PlanRowExpressionNode::ContextualOrder { .. }
                                    | PlanRowExpressionNode::ListAccess { .. }
                                    | PlanRowExpressionNode::ListPage { .. }
                                    | PlanRowExpressionNode::BoundedListPage { .. }
                            )
                        {
                            owners.insert(*owner);
                        }
                        walk_outer_collection_spine(arena, *source, candidates, owners)?;
                    }
                    PlanRowExpressionNode::ContextualOrder { source, .. } => {
                        walk_outer_collection_spine(arena, *source, candidates, owners)?;
                    }
                    PlanRowExpressionNode::Select { arms, .. } => {
                        for arm in arms {
                            walk_outer_collection_spine(arena, arm.value, candidates, owners)?;
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            walk_outer_collection_spine(arena, root, &candidates, &mut owners)
        };
        match expression {
            PlanDerivedExpression::RowExpression { expression }
            | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
                visit_row(*expression)?;
            }
            PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
                visit_row(*default)?;
                for arm in arms {
                    visit_row(arm.value)?;
                }
            }
            _ => {}
        }
        Ok(owners
            .into_iter()
            .map(|owner| PlanValueListAuthority {
                owner,
                list_id: target_list,
                fields: fields.clone(),
            })
            .collect())
    }

    fn active_local_capture_expression(
        &mut self,
        capture: &ir::ErasedLocalCapture,
    ) -> Result<PlanRowExpressionId, PlanError> {
        let owner = PlanStaticOwnerId(capture.source_owner.as_usize());
        if !self.active_materialization_owners.contains(&owner) {
            return Err(PlanError::new(format!(
                "capture source owner {} local {} is not active while its target row is materialized",
                capture.source_owner, capture.source_local.0
            )));
        }
        let local = self
            .program
            .scope_index
            .locals
            .iter()
            .find(|local| {
                local.owner == capture.source_owner && local.local == capture.source_local
            })
            .cloned()
            .ok_or_else(|| {
                PlanError::new(format!(
                    "capture source owner {} local {} is missing",
                    capture.source_owner, capture.source_local.0
                ))
            })?;
        let plan_local = PlanLocalId(capture.source_local.0 as usize);
        if local.row.is_none() {
            return self.intern(PlanRowExpressionNode::Local {
                owner,
                local: plan_local,
                projection: capture.projection.clone(),
            });
        }
        if capture.projection.is_empty() {
            return match &local.item_type {
                boon_typecheck::Type::Object(shape) => {
                    let names = if shape.field_order.is_empty() {
                        shape.fields.keys().cloned().collect::<Vec<_>>()
                    } else {
                        shape.field_order.clone()
                    };
                    let row = self.intern(PlanRowExpressionNode::LocalRow {
                        owner,
                        local: plan_local,
                    })?;
                    let mut fields = Vec::with_capacity(names.len());
                    for name in names {
                        fields.push(PlanRowObjectField {
                            value: self.project_field(row, name.clone())?,
                            name,
                            spread: false,
                        });
                    }
                    self.intern(PlanRowExpressionNode::Object { fields })
                }
                _ => {
                    let row = self.intern(PlanRowExpressionNode::LocalRow {
                        owner,
                        local: plan_local,
                    })?;
                    self.project_field(row, "value".to_owned())
                }
            };
        }
        let mut value = self.intern(PlanRowExpressionNode::LocalRow {
            owner,
            local: plan_local,
        })?;
        for field in &capture.projection {
            value = self.project_field(value, field.clone())?;
        }
        Ok(value)
    }

    fn detached_state_local(
        &mut self,
        erased_owner: ir::StaticOwnerId,
        local: ir::MaterializationLocalId,
        projection: &[String],
        constructor_projection: &[String],
    ) -> Result<PlanRowExpressionId, PlanError> {
        let (state_label, state_rows) = if let Some(state) = self.state_initializer {
            (
                format!("runtime state {} initializer", state.0),
                self.program
                    .scope_index
                    .bindings
                    .iter()
                    .filter_map(|binding| match binding.target {
                        ir::ErasedBindingTarget::State {
                            runtime,
                            row: Some(row),
                            ..
                        } if runtime == state => Some(row),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>(),
            )
        } else if let Some(state) = self.active_state_update {
            (
                format!("executable state {} update", state.0),
                self.program
                    .scope_index
                    .bindings
                    .iter()
                    .filter_map(|binding| match binding.target {
                        ir::ErasedBindingTarget::State {
                            executable,
                            row: Some(row),
                            ..
                        } if executable == state => Some(row),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>(),
            )
        } else {
            return Err(PlanError::new(
                "detached contextual local has no active state owner",
            ));
        };
        if state_rows.len() != 1 {
            return Err(PlanError::new(format!(
                "{state_label} has {} exact storage rows",
                state_rows.len()
            )));
        }
        let state_row = *state_rows
            .iter()
            .next()
            .expect("one indexed state storage row exists");
        let materialization = self
            .program
            .materializations
            .iter()
            .find(|materialization| {
                materialization.owner == erased_owner && materialization.row_local == local
            })
            .ok_or_else(|| {
                PlanError::new(format!(
                    "{state_label} local {}:{} has no exact materialization",
                    erased_owner.0, local.0
                ))
            })?
            .clone();
        let capture_fields = self
            .program
            .scope_index
            .locals
            .iter()
            .flat_map(|target| &target.captures)
            .filter(|capture| {
                capture.source_owner == erased_owner
                    && capture.source_local == local
                    && capture.projection == projection
            })
            .filter_map(|capture| {
                self.program
                    .scope_index
                    .fields
                    .get(capture.field.as_usize())
                    .filter(|field| {
                        field.id == capture.field
                            && field.role == ir::ErasedFieldRole::Capture
                            && field.row == Some(state_row)
                    })
            })
            .collect::<Vec<_>>();
        match capture_fields.as_slice() {
            [capture] => {
                return self.intern(PlanRowExpressionNode::Field {
                    input: ValueRef::Field(plan_field_id(capture.id)),
                });
            }
            [] => {}
            captures => {
                return Err(PlanError::new(format!(
                    "{state_label} local {}:{} projection `{}` resolves to {} hidden captures",
                    erased_owner.0,
                    local.0,
                    projection.join("."),
                    captures.len()
                )));
            }
        }
        if materialization.target_list_id != Some(state_row.list)
            || materialization.target_scope_id != Some(state_row.scope)
        {
            return Err(PlanError::new(format!(
                "{state_label} local {}:{} targets row {:?}/{:?}, expected {}/{} and has no exact hidden capture",
                erased_owner.0,
                local.0,
                materialization.target_list_id,
                materialization.target_scope_id,
                state_row.list,
                state_row.scope
            )));
        }
        let list_id = plan_list_id(state_row.list);
        let program = self.program;
        let list_name = program
            .lists
            .iter()
            .find(|list| plan_list_id(list.id) == list_id)
            .map(|list| list.name.as_str())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "{state_label} local {}:{} references missing target list {}",
                    erased_owner.0, local.0, list_id.0
                ))
            })?;
        let authority_fields = list_authority_field_ids(program);
        let field_id = |field_name: &str| {
            storage_input_field_id(program, list_name, field_name, &authority_fields).ok_or_else(
                || {
                    PlanError::new(format!(
                        "{state_label} local {}:{} member `{field_name}` has no exact target-row constructor field",
                        erased_owner.0, local.0
                    ))
                },
            )
        };
        let body = self
            .program
            .executable
            .expressions
            .get(materialization.body.as_usize())
            .filter(|expression| expression.id == materialization.body)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "{state_label} local {}:{} references missing materialization body {}",
                    erased_owner.0, local.0, materialization.body.0
                ))
            })?;
        let mut target_paths = body
            .provenance
            .members
            .iter()
            .filter_map(|member| match &member.origin {
                ir::ExecutableValueOrigin::MaterializationLocal {
                    owner,
                    local: candidate,
                    projection: source_projection,
                } if *owner == erased_owner
                    && *candidate == local
                    && projection.starts_with(source_projection) =>
                {
                    let mut target = member.path.clone();
                    target.extend_from_slice(&projection[source_projection.len()..]);
                    Some((source_projection.len(), target))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Some(depth) = target_paths.iter().map(|(depth, _)| *depth).max() {
            target_paths.retain(|(candidate, _)| *candidate == depth);
            let mut candidates = target_paths
                .into_iter()
                .map(|(_, path)| path)
                .collect::<BTreeSet<_>>();
            if candidates.len() > 1 && !constructor_projection.is_empty() {
                let exact_constructor = candidates
                    .iter()
                    .filter(|candidate| candidate.as_slice() == constructor_projection)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                if exact_constructor.len() == 1 {
                    candidates = exact_constructor;
                }
            }
            if candidates.len() != 1 {
                return Err(PlanError::new(format!(
                    "{state_label} local {}:{} source projection `{}` with constructor \
                     projection `{}` maps to {} target-row constructor paths: {:?}",
                    erased_owner.0,
                    local.0,
                    projection.join("."),
                    constructor_projection.join("."),
                    candidates.len(),
                    candidates
                )));
            }
            let target = candidates
                .iter()
                .next()
                .expect("one target-row constructor path remains");
            let (field_name, nested) = target
                .split_first()
                .map_or(("value", &[][..]), |(field, nested)| {
                    (field.as_str(), nested)
                });
            let mut value = self.intern(PlanRowExpressionNode::Field {
                input: ValueRef::Field(field_id(field_name)?),
            })?;
            for field in nested {
                value = self.project_field(value, field.clone())?;
            }
            return Ok(value);
        }
        if let Some((first, nested)) = projection.split_first() {
            let mut value = self.intern(PlanRowExpressionNode::Field {
                input: ValueRef::Field(field_id(first)?),
            })?;
            for field in nested {
                value = self.project_field(value, field.clone())?;
            }
            return Ok(value);
        }
        match &materialization.item_type {
            boon_typecheck::Type::Object(shape) => {
                let names = if shape.field_order.is_empty() {
                    shape.fields.keys().cloned().collect::<Vec<_>>()
                } else {
                    shape.field_order.clone()
                };
                let mut fields = Vec::with_capacity(names.len());
                for name in names {
                    fields.push(PlanRowObjectField {
                        value: self.intern(PlanRowExpressionNode::Field {
                            input: ValueRef::Field(field_id(&name)?),
                        })?,
                        name,
                        spread: false,
                    });
                }
                self.intern(PlanRowExpressionNode::Object { fields })
            }
            _ => self.intern(PlanRowExpressionNode::Field {
                input: ValueRef::Field(field_id("value")?),
            }),
        }
    }

    fn lower(&mut self, root: ir::ExecutableExprId) -> Result<PlanRowExpressionId, PlanError> {
        self.lower_scoped(root, None)
    }

    fn lower_reachability_gate(
        &mut self,
        root: ir::ExecutableExprId,
        target: ir::ExecutableExprId,
    ) -> Result<PlanRowExpressionId, PlanError> {
        self.lower_reachability_gate_scoped(root, target, None)
    }

    fn lower_reachability_gate_scoped(
        &mut self,
        root: ir::ExecutableExprId,
        target: ir::ExecutableExprId,
        inherited_owner: Option<PlanStaticOwnerId>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        if root == target {
            return self.constant(PlanConstantValue::Tag {
                name: "True".to_owned(),
            });
        }
        let expression = self
            .program
            .executable
            .expressions
            .get(root.as_usize())
            .filter(|expression| expression.id == root)
            .cloned()
            .ok_or_else(|| PlanError::new(format!("executable expression {root} is missing")))?;
        let owner = expression
            .owner
            .map(|owner| PlanStaticOwnerId(owner.as_usize()))
            .or(inherited_owner);
        if let ir::ExecutableExpressionKind::When { input, arms } = expression.kind {
            if executable_expression_reaches(self.program, input, target) {
                return self.lower_reachability_gate_scoped(input, target, owner);
            }
            let input = self.lower_scoped(input, owner)?;
            let mut reaches_effect = false;
            let mut has_wildcard = false;
            let mut lowered_arms = Vec::with_capacity(arms.len() + 1);
            for arm in arms {
                let reaches = executable_expression_reaches(self.program, arm.output, target);
                reaches_effect |= reaches;
                let pattern = executable_select_pattern(&arm.pattern)?;
                has_wildcard |= matches!(pattern, PlanRowSelectPattern::Wildcard);
                let value = if reaches {
                    self.lower_reachability_gate_scoped(arm.output, target, owner)?
                } else {
                    self.constant(PlanConstantValue::Tag {
                        name: "False".to_owned(),
                    })?
                };
                lowered_arms.push(PlanRowSelectArm { pattern, value });
            }
            if !reaches_effect {
                return Err(PlanError::new(format!(
                    "target expression {target} is not reachable from conditional output {root}"
                )));
            }
            if !has_wildcard {
                lowered_arms.push(PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Wildcard,
                    value: self.constant(PlanConstantValue::Tag {
                        name: "False".to_owned(),
                    })?,
                });
            }
            return self.intern(PlanRowExpressionNode::Select {
                input,
                arms: lowered_arms,
            });
        }

        let children = ir::executable_expression_children(&expression.kind)
            .into_iter()
            .filter(|child| executable_expression_reaches(self.program, *child, target))
            .collect::<Vec<_>>();
        let [child] = children.as_slice() else {
            return Err(PlanError::new(format!(
                "target expression {target} has {} executable control paths from output {root}",
                children.len()
            )));
        };
        if let ir::ExecutableExpressionKind::Materialize { materialization } = expression.kind {
            let materialization = self
                .program
                .materializations
                .get(materialization)
                .ok_or_else(|| PlanError::new("executable materialization is missing"))?;
            let materialization_owner = PlanStaticOwnerId(materialization.owner.as_usize());
            self.active_materialization_owners
                .push(materialization_owner);
            let result =
                self.lower_reachability_gate_scoped(*child, target, Some(materialization_owner));
            self.active_materialization_owners.pop();
            result
        } else {
            self.lower_reachability_gate_scoped(*child, target, owner)
        }
    }

    fn lower_scoped(
        &mut self,
        root: ir::ExecutableExprId,
        inherited_owner: Option<PlanStaticOwnerId>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        if let Some(value) = self.bindings.get(&root) {
            return Ok(*value);
        }
        let key = (
            root,
            inherited_owner,
            self.active_materialization_owners.clone(),
        );
        if let Some(value) = self.memo.get(&key) {
            return Ok(*value);
        }
        let expression = self
            .program
            .executable
            .expressions
            .get(root.as_usize())
            .cloned()
            .ok_or_else(|| {
                PlanError::new(format!("executable expression {} is missing", root.0))
            })?;
        if let Some(value) = self.index.resolve_distributed_expression(root) {
            return self.value_ref(value);
        }
        if let Some(state) = self
            .program
            .executable
            .states
            .iter()
            .find(|state| state.expression == root)
            && self.active_state_update != Some(state.id)
        {
            let value = self
                .index
                .resolve_executable_state(state.id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "executable state {} (`{}`) has no unique typed StateId",
                        state.id, state.binding_path
                    ))
                })?;
            return self.value_ref(value);
        }
        let owner = expression
            .owner
            .map(|owner| PlanStaticOwnerId(owner.as_usize()))
            .or(inherited_owner);
        if let Some(region) = self
            .program
            .transient_collections
            .iter()
            .find(|region| region.result.expression() == root)
            .cloned()
        {
            let value = self.lower_transient_collection(&region, owner)?;
            self.memo.insert(key, value);
            return Ok(value);
        }
        let value = match expression.kind {
            ir::ExecutableExpressionKind::CanonicalRead { .. }
            | ir::ExecutableExpressionKind::LocalRead { .. }
            | ir::ExecutableExpressionKind::ExternalRead { .. }
            | ir::ExecutableExpressionKind::Drain { .. } => self.lower_erased_read(root, owner)?,
            ir::ExecutableExpressionKind::ElementState { .. } => {
                return Err(PlanError::new(format!(
                    "element state expression {} cannot be lowered into the machine plan",
                    root.0
                )));
            }
            ir::ExecutableExpressionKind::Text(value) => {
                self.constant(PlanConstantValue::Text { value })?
            }
            ir::ExecutableExpressionKind::TextTemplate { segments } => {
                let parts = segments
                    .into_iter()
                    .map(|segment| match segment {
                        ir::ExecutableTextSegment::Static { value } => {
                            self.constant(PlanConstantValue::Text { value })
                        }
                        ir::ExecutableTextSegment::Dynamic { value } => {
                            self.lower_scoped(value, owner)
                        }
                    })
                    .collect::<Result<Vec<_>, PlanError>>()?;
                self.intern(PlanRowExpressionNode::TextConcat { parts })?
            }
            ir::ExecutableExpressionKind::Number(value) => {
                self.constant(PlanConstantValue::Number { value })?
            }
            ir::ExecutableExpressionKind::Bits(value) => {
                self.constant(PlanConstantValue::Bits { value })?
            }
            ir::ExecutableExpressionKind::BytesByte(value) => self.bytes_constant(vec![value])?,
            ir::ExecutableExpressionKind::Absent => self.intern(PlanRowExpressionNode::Absent)?,
            ir::ExecutableExpressionKind::Flush { payload } => {
                let payload = self.lower_scoped(payload, owner)?;
                self.intern(PlanRowExpressionNode::Flush { payload })?
            }
            ir::ExecutableExpressionKind::FlushBoundary { input } => {
                let input = self.lower_scoped(input, owner)?;
                self.intern(PlanRowExpressionNode::FlushBoundary { input })?
            }
            ir::ExecutableExpressionKind::Tag(value) => {
                self.constant(PlanConstantValue::Tag { name: value })?
            }
            ir::ExecutableExpressionKind::TaggedObject { tag, fields } => {
                let fields = self.lower_fields(fields, owner)?;
                self.intern(PlanRowExpressionNode::TaggedObject { tag, fields })?
            }
            ir::ExecutableExpressionKind::Source { .. } => {
                let source = self
                    .program
                    .executable
                    .sources
                    .iter()
                    .find(|source| source.expression == root)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "SOURCE executable expression {} has no ExecutableSourceId",
                            root.0
                        ))
                    })?;
                let value = self
                    .index
                    .resolve_executable_source(source.id)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "executable source {} (`{}`) has no unique typed SourceId",
                            source.id, source.binding_path
                        ))
                    })?;
                self.value_ref(value)?
            }
            ir::ExecutableExpressionKind::Call {
                callable_kind: _,
                name,
                arguments,
                contexts,
                ..
            } => {
                if !contexts.is_empty() {
                    return Err(PlanError::new(format!(
                        "call-local host context at expression {} cannot be lowered into a row value",
                        root.0
                    )));
                }
                self.lower_call(&name, arguments, owner)?
            }
            ir::ExecutableExpressionKind::Materialize { materialization } => {
                let materialization = self
                    .program
                    .materializations
                    .get(materialization)
                    .cloned()
                    .ok_or_else(|| PlanError::new("executable materialization is missing"))?;
                let source_expression = self
                    .program
                    .executable
                    .expressions
                    .get(materialization.source.as_usize())
                    .filter(|expression| expression.id == materialization.source)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "contextual {:?} owner {} references missing source expression {}",
                            materialization.operation,
                            materialization.owner.as_usize(),
                            materialization.source
                        ))
                    })?;
                let source_is_stored_authority = match &source_expression.kind {
                    ir::ExecutableExpressionKind::List { .. } => true,
                    ir::ExecutableExpressionKind::Call { name, .. } if name == "List/range" => {
                        materialization
                            .source_list_id
                            .and_then(|source_list| {
                                self.program
                                    .lists
                                    .iter()
                                    .find(|list| list.id == source_list)
                            })
                            .is_some_and(|list| {
                                matches!(list.initializer, ir::ListInitializer::Range { .. })
                            })
                    }
                    _ => false,
                };
                let source = if source_is_stored_authority {
                    let list_id = materialization.source_list_id.ok_or_else(|| {
                        PlanError::new(format!(
                            "contextual {:?} owner {} has an object-list authority without keyed storage",
                            materialization.operation,
                            materialization.owner.as_usize()
                        ))
                    })?;
                    let list_id = plan_list_id(list_id);
                    if !self.inputs.contains(&ValueRef::List(list_id)) {
                        self.inputs.push(ValueRef::List(list_id));
                    }
                    self.intern(PlanRowExpressionNode::AuthorityListRef { list_id })?
                } else {
                    self.lower_scoped(materialization.source, owner)
                        .map_err(|error| {
                            PlanError::new(format!(
                                "contextual {:?} owner {} source failed: {error}",
                                materialization.operation,
                                materialization.owner.as_usize()
                            ))
                        })?
                };
                let materialization_owner = PlanStaticOwnerId(materialization.owner.as_usize());
                self.active_materialization_owners
                    .push(materialization_owner);
                let body_result = self
                    .lower_scoped(materialization.body, Some(materialization_owner))
                    .and_then(|body| {
                        let captures = self.materialization_captures(&materialization)?;
                        Ok((body, captures))
                    });
                self.active_materialization_owners.pop();
                let (body, captures) = body_result.map_err(|error| {
                    PlanError::new(format!(
                        "contextual {:?} owner {} body failed: {error}",
                        materialization.operation,
                        materialization.owner.as_usize()
                    ))
                })?;
                if matches!(
                    materialization.operation,
                    ir::ContextualOperationKind::SortBy | ir::ContextualOperationKind::ThenBy
                ) {
                    if !captures.is_empty() {
                        return Err(PlanError::new(format!(
                            "contextual order owner {} cannot own detached state captures",
                            materialization.owner
                        )));
                    }
                    let mut ordered_source = source;
                    for inherited in &materialization.inherited_order {
                        if !matches!(
                            inherited.operation,
                            ir::ContextualOperationKind::SortBy
                                | ir::ContextualOperationKind::ThenBy
                        ) {
                            return Err(PlanError::new(format!(
                                "contextual order owner {} inherited non-order operation {:?}",
                                materialization.owner, inherited.operation
                            )));
                        }
                        self.active_materialization_owners
                            .push(materialization_owner);
                        let inherited_body =
                            self.lower_scoped(inherited.body, Some(materialization_owner));
                        self.active_materialization_owners.pop();
                        let inherited_body = inherited_body.map_err(|error| {
                            PlanError::new(format!(
                                "contextual order owner {} inherited key failed: {error}",
                                materialization.owner.as_usize()
                            ))
                        })?;
                        let inherited_direction = self
                            .lower_scoped(inherited.direction, owner)
                            .map_err(|error| {
                                PlanError::new(format!(
                                    "contextual order owner {} inherited direction failed: {error}",
                                    materialization.owner.as_usize()
                                ))
                            })?;
                        ordered_source = self.intern(PlanRowExpressionNode::ContextualOrder {
                            owner: materialization_owner,
                            operation: plan_order_operation(inherited.operation)?,
                            source: ordered_source,
                            row_local: PlanLocalId(materialization.row_local.0 as usize),
                            key: inherited_body,
                            direction: inherited_direction,
                        })?;
                    }
                    let direction = materialization.direction.ok_or_else(|| {
                        PlanError::new(format!(
                            "contextual {:?} owner {} has no erased direction expression",
                            materialization.operation,
                            materialization.owner.as_usize()
                        ))
                    })?;
                    let direction = self.lower_scoped(direction, owner).map_err(|error| {
                        PlanError::new(format!(
                            "contextual {:?} owner {} direction failed: {error}",
                            materialization.operation,
                            materialization.owner.as_usize()
                        ))
                    })?;
                    self.intern(PlanRowExpressionNode::ContextualOrder {
                        owner: PlanStaticOwnerId(materialization.owner.as_usize()),
                        operation: plan_order_operation(materialization.operation)?,
                        source: ordered_source,
                        row_local: PlanLocalId(materialization.row_local.0 as usize),
                        key: body,
                        direction,
                    })?
                } else {
                    self.intern(PlanRowExpressionNode::ContextualCollection {
                        owner: PlanStaticOwnerId(materialization.owner.as_usize()),
                        operation: plan_contextual_operation(materialization.operation)?,
                        source,
                        row_local: PlanLocalId(materialization.row_local.0 as usize),
                        body,
                        captures,
                        indexed_access: None,
                    })?
                }
            }
            ir::ExecutableExpressionKind::Draining { input } => self.lower_scoped(input, owner)?,
            ir::ExecutableExpressionKind::Hold { .. } => {
                let state = self
                    .program
                    .executable
                    .states
                    .iter()
                    .find(|state| state.expression == root)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "HOLD executable expression {} has no ExecutableStateId; registered states are {:?}",
                            root.0,
                            self.program
                                .executable
                                .states
                                .iter()
                                .map(|state| (state.id, state.expression, state.binding_path.as_str()))
                                .collect::<Vec<_>>()
                        ))
                    })?;
                let value = self
                    .index
                    .resolve_executable_state(state.id)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "executable state {} (`{}`) has no unique typed StateId",
                            state.id, state.binding_path
                        ))
                    })?;
                self.value_ref(value)?
            }
            ir::ExecutableExpressionKind::Latest { branches } => {
                let [branch] = branches.as_slice() else {
                    return Err(PlanError::new(format!(
                        "temporal LATEST expression {} reached pure executable value lowering while updating {:?}; registered state expressions {:?}",
                        root.0,
                        self.active_state_update,
                        self.program
                            .executable
                            .states
                            .iter()
                            .map(|state| (state.id, state.expression))
                            .collect::<Vec<_>>()
                    )));
                };
                self.lower_scoped(*branch, owner)?
            }
            ir::ExecutableExpressionKind::When { input, arms } => {
                let input = self.lower_scoped(input, owner)?;
                let arms = arms
                    .into_iter()
                    .map(|arm| {
                        Ok(PlanRowSelectArm {
                            pattern: executable_select_pattern(&arm.pattern)?,
                            value: self.lower_scoped(arm.output, owner)?,
                        })
                    })
                    .collect::<Result<Vec<_>, PlanError>>()?;
                self.intern(PlanRowExpressionNode::Select { input, arms })?
            }
            ir::ExecutableExpressionKind::Then { input, output } => {
                self.lower_scoped(output.unwrap_or(input), owner)?
            }
            ir::ExecutableExpressionKind::Infix { left, op, right } => {
                let op = PlanInfixOp::from_symbol(&op).ok_or_else(|| {
                    PlanError::new(format!(
                        "checked executable expression uses unsupported infix operator `{op}`"
                    ))
                })?;
                let left = self.lower_scoped(left, owner)?;
                let right = self.lower_scoped(right, owner)?;
                self.intern(PlanRowExpressionNode::NumberInfix { op, left, right })?
            }
            ir::ExecutableExpressionKind::MatchArm { output, .. } => self.lower_scoped(
                output.ok_or_else(|| PlanError::new("match arm has no output"))?,
                owner,
            )?,
            ir::ExecutableExpressionKind::Object(fields) => {
                let fields = self.lower_fields(fields, owner)?;
                self.intern(PlanRowExpressionNode::Object { fields })?
            }
            ir::ExecutableExpressionKind::Block { bindings, result } => {
                let bindings = bindings
                    .into_iter()
                    .map(|binding| (binding.id, binding.value))
                    .collect();
                self.lexical_bindings.push(bindings);
                let value = self.lower_scoped(result, owner);
                self.lexical_bindings.pop();
                value?
            }
            ir::ExecutableExpressionKind::List { items, .. } => {
                let items = items
                    .into_iter()
                    .map(|item| self.lower_scoped(item, owner))
                    .collect::<Result<Vec<_>, _>>()?;
                self.intern(PlanRowExpressionNode::ListLiteral { items })?
            }
            ir::ExecutableExpressionKind::MapEntry { .. } => {
                return Err(PlanError::new(format!(
                    "MAP entry executable expression {} escaped its MAP literal",
                    root.0
                )));
            }
            ir::ExecutableExpressionKind::Map { entries } => {
                let mut lowered = Vec::with_capacity(entries.len());
                for entry in entries {
                    let kind = self
                        .program
                        .executable
                        .expressions
                        .get(entry.as_usize())
                        .filter(|expression| expression.id == entry)
                        .map(|expression| expression.kind.clone())
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "MAP executable expression {} references missing entry {}",
                                root.0, entry.0
                            ))
                        })?;
                    let ir::ExecutableExpressionKind::MapEntry { key, value } = kind else {
                        return Err(PlanError::new(format!(
                            "MAP executable expression {} contains non-entry expression {}",
                            root.0, entry.0
                        )));
                    };
                    lowered.push(PlanRowMapEntry {
                        key: self.lower_scoped(key, owner)?,
                        value: self.lower_scoped(value, owner)?,
                    });
                }
                self.intern(PlanRowExpressionNode::MapLiteral {
                    authority: boon_plan::PlanCollectionAuthorityId(root.as_usize()),
                    entries: lowered,
                })?
            }
            ir::ExecutableExpressionKind::Set { items } => {
                let items = items
                    .into_iter()
                    .map(|item| self.lower_scoped(item, owner))
                    .collect::<Result<Vec<_>, _>>()?;
                self.intern(PlanRowExpressionNode::SetLiteral {
                    authority: boon_plan::PlanCollectionAuthorityId(root.as_usize()),
                    items,
                })?
            }
            ir::ExecutableExpressionKind::Bytes { .. } => {
                self.bytes_constant(executable_static_bytes(self.program, root).ok_or_else(
                    || PlanError::new("dynamic BYTES literal is not a closed scalar"),
                )?)?
            }
            ir::ExecutableExpressionKind::Delimiter => {
                let parents = self
                    .program
                    .executable
                    .expressions
                    .iter()
                    .filter(|expression| {
                        ir::executable_expression_children(&expression.kind).contains(&root)
                    })
                    .map(|expression| {
                        (
                            expression.id.0,
                            expression.checked_expr_id.0,
                            expression.kind.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                return Err(PlanError::new(format!(
                    "pipeline delimiter executable expression {} (checked {}) survived expansion under parent(s) {:?}",
                    root.0, expression.checked_expr_id.0, parents
                )));
            }
            ir::ExecutableExpressionKind::Project { input, fields } => {
                let mut value = self.lower_scoped(input, owner)?;
                for field in fields {
                    value = self.project_field(value, field)?;
                }
                value
            }
            ir::ExecutableExpressionKind::MaterializationLocal {
                owner: local_owner,
                local,
                projection,
                constructor_projection,
            } => {
                let erased_owner = local_owner;
                let local_owner = PlanStaticOwnerId(local_owner.as_usize());
                let _materialization = self
                    .program
                    .materializations
                    .iter()
                    .find(|materialization| {
                        PlanStaticOwnerId(materialization.owner.as_usize()) == local_owner
                            && materialization.row_local == local
                    })
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "contextual owner {} local {} has no exact materialization",
                            local_owner.0, local.0
                        ))
                    })?;
                let local_def = self
                    .program
                    .scope_index
                    .locals
                    .iter()
                    .find(|candidate| candidate.owner == erased_owner && candidate.local == local)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "contextual owner {} local {} has no erased row binding",
                            local_owner.0, local.0
                        ))
                    })?;
                let local_is_active = self.active_materialization_owners.contains(&local_owner);
                let detached_state = (self.state_initializer.is_some()
                    || self.active_state_update.is_some())
                    && !local_is_active;
                let local_row_owner = local_def.row;
                let local_item_type = local_def.item_type.clone();
                if detached_state && local_row_owner.is_none() {
                    return self.detached_state_local(
                        erased_owner,
                        local,
                        &projection,
                        &constructor_projection,
                    );
                }
                let Some(row) = local_row_owner else {
                    return self.intern(PlanRowExpressionNode::Local {
                        owner: local_owner,
                        local: PlanLocalId(local.0 as usize),
                        projection,
                    });
                };
                let list_id = plan_list_id(row.list);
                let event_row = self.event_row.and_then(|(source, event_list)| {
                    self.event_row_reaches_list(event_list, list_id)
                        .then_some((source, event_list))
                });
                if detached_state && event_row.is_none() {
                    return self.detached_state_local(
                        erased_owner,
                        local,
                        &projection,
                        &constructor_projection,
                    );
                }
                let contextual_row = self.intern(PlanRowExpressionNode::LocalRow {
                    owner: local_owner,
                    local: PlanLocalId(local.0 as usize),
                })?;
                let event_row_expression = event_row
                    .map(|(source, event_list)| {
                        self.intern(PlanRowExpressionNode::EventRow {
                            source,
                            list_id: event_list,
                        })
                    })
                    .transpose()?;
                let local_row = event_row_expression.unwrap_or(contextual_row);
                if projection.is_empty()
                    && matches!(local_item_type, boon_typecheck::Type::Number)
                    && self
                        .program
                        .lists
                        .iter()
                        .find(|list| plan_list_id(list.id) == list_id)
                        .is_some_and(|list| {
                            matches!(list.initializer, ir::ListInitializer::Range { .. })
                        })
                {
                    let fields = self
                        .program
                        .scope_index
                        .fields
                        .iter()
                        .filter(|field| {
                            field.row.map(|row| plan_list_id(row.list)) == Some(list_id)
                                && field.name == "value"
                                && field.role.is_authority()
                        })
                        .map(|field| plan_field_id(field.id))
                        .collect::<Vec<_>>();
                    let [field] = fields.as_slice() else {
                        return Err(PlanError::new(format!(
                            "range list {} scalar row resolves to {} authority `value` fields",
                            list_id.0,
                            fields.len()
                        )));
                    };
                    let list_input = ValueRef::List(list_id);
                    if !self.inputs.contains(&list_input) {
                        self.inputs.push(list_input);
                    }
                    self.intern(PlanRowExpressionNode::ListRowField {
                        row: local_row,
                        list_id,
                        field: *field,
                    })?
                } else if projection.is_empty() {
                    local_row
                } else if let Some((target, consumed)) =
                    self.materialization_local_member(erased_owner, local, &projection)
                {
                    let mut projection_offset = consumed;
                    let mut value = match target {
                        ir::ErasedLocalMemberTarget::Field(field) => {
                            let mut row = local_row;
                            let (field_list, field) = if let Some((_, event_list)) = event_row
                                && event_list != list_id
                            {
                                let event_field =
                                    self.event_list_field(event_list, &projection[0])?;
                                let event_field_is_resource_only = self
                                    .program
                                    .scope_index
                                    .fields
                                    .iter()
                                    .find(|candidate| plan_field_id(candidate.id) == event_field)
                                    .is_some_and(|candidate| candidate.resource_only);
                                let target_field_is_resource_only = self
                                    .program
                                    .scope_index
                                    .fields
                                    .iter()
                                    .find(|candidate| candidate.id == field)
                                    .is_some_and(|candidate| candidate.resource_only);
                                if local_is_active
                                    && event_field_is_resource_only
                                    && !target_field_is_resource_only
                                {
                                    row = contextual_row;
                                    (list_id, plan_field_id(field))
                                } else {
                                    (event_list, event_field)
                                }
                            } else {
                                (list_id, plan_field_id(field))
                            };
                            let list_input = ValueRef::List(field_list);
                            if !self.inputs.contains(&list_input) {
                                self.inputs.push(list_input);
                            }
                            self.intern(PlanRowExpressionNode::ListRowField {
                                row,
                                list_id: field_list,
                                field,
                            })?
                        }
                        ir::ErasedLocalMemberTarget::Source(source) => {
                            if let Some(payload_name) = projection.get(consumed) {
                                let source_port = self
                                    .program
                                    .sources
                                    .iter()
                                    .find(|candidate| candidate.id == source)
                                    .ok_or_else(|| {
                                        PlanError::new(format!(
                                            "materialization local references missing source {}",
                                            source.0
                                        ))
                                    })?;
                                let payload_field = source_port
                                    .payload_schema
                                    .fields
                                    .iter()
                                    .find(|field| field.name() == payload_name)
                                    .ok_or_else(|| {
                                        PlanError::new(format!(
                                            "source `{}` has no payload field `{payload_name}`",
                                            source_port.path
                                        ))
                                    })?;
                                projection_offset += 1;
                                self.value_ref(ValueRef::SourcePayload {
                                    source_id: plan_source_id(source),
                                    field: source_payload_field_from_ir(payload_field),
                                })?
                            } else {
                                self.value_ref(ValueRef::Source(plan_source_id(source)))?
                            }
                        }
                        ir::ErasedLocalMemberTarget::State(state) => {
                            let state_cell = self
                                .program
                                .state_cells
                                .get(state.as_usize())
                                .filter(|candidate| candidate.id == state)
                                .ok_or_else(|| {
                                    PlanError::new(format!(
                                        "materialization local references missing state {}",
                                        state.0
                                    ))
                                })?;
                            let state_field = plan_indexed_state_field(self.program, state_cell)?
                                .ok_or_else(|| {
                                    PlanError::new(format!(
                                        "materialization local member `{}` references non-indexed state `{}`",
                                        projection.join("."),
                                        state_cell.path
                                    ))
                                })?;
                            let state_input = ValueRef::State(plan_state_id(state));
                            if !self.inputs.contains(&state_input) {
                                self.inputs.push(state_input);
                            }
                            let list_input = ValueRef::List(list_id);
                            if !self.inputs.contains(&list_input) {
                                self.inputs.push(list_input);
                            }
                            self.intern(PlanRowExpressionNode::ListRowField {
                                row: local_row,
                                list_id,
                                field: state_field,
                            })?
                        }
                    };
                    for nested in projection.iter().skip(projection_offset) {
                        value = self.project_field(value, nested.clone())?;
                    }
                    value
                } else {
                    if !local_is_active && event_row.is_some() {
                        return Err(PlanError::new(format!(
                            "event row for contextual owner {} local {} projection `{}` has no exact typed row field",
                            local_owner.0,
                            local.0,
                            projection.join(".")
                        )));
                    }
                    self.intern(PlanRowExpressionNode::Local {
                        owner: local_owner,
                        local: PlanLocalId(local.0 as usize),
                        projection,
                    })?
                }
            }
            ir::ExecutableExpressionKind::FunctionParameter {
                parameter,
                projection,
            } => {
                return Err(PlanError::new(format!(
                    "executable producer parameter {}:{} projection `{}` has no exact distributed expression binding",
                    parameter.function.0,
                    parameter.ordinal,
                    projection.join(".")
                )));
            }
        };
        self.memo.insert(key, value);
        Ok(value)
    }

    fn lower_transient_collection(
        &mut self,
        region: &ir::TransientCollection,
        owner: Option<PlanStaticOwnerId>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        if region.snapshot_copy_budget != 0
            || region.authority_flow.first() != Some(&region.constructor)
            || region.authority_flow.last() != Some(&region.result.expression())
        {
            return Err(PlanError::new(format!(
                "verified transient collection {} has invalid proof flow or snapshot budget",
                region.constructor.0
            )));
        }
        let kind = match region.kind {
            ir::TransientCollectionKind::List => PlanTransientCollectionKind::List,
            ir::TransientCollectionKind::Map => PlanTransientCollectionKind::Map,
            ir::TransientCollectionKind::Set => PlanTransientCollectionKind::Set,
        };
        let list_items = region
            .list_items
            .iter()
            .copied()
            .map(|item| self.lower_scoped(item, owner))
            .collect::<Result<Vec<_>, PlanError>>()?;
        let map_entries = region
            .map_entries
            .iter()
            .map(|entry| {
                Ok(PlanTransientMapEntry {
                    key: self.lower_scoped(entry.key, owner)?,
                    value: self.lower_scoped(entry.value, owner)?,
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let set_items = region
            .set_items
            .iter()
            .copied()
            .map(|item| self.lower_scoped(item, owner))
            .collect::<Result<Vec<_>, PlanError>>()?;
        let steps = region
            .steps
            .iter()
            .map(|step| {
                Ok(match step {
                    ir::TransientCollectionStep::ListAppend { item, .. } => {
                        PlanTransientCollectionStep::ListAppend {
                            item: self.lower_scoped(*item, owner)?,
                        }
                    }
                    ir::TransientCollectionStep::MapUpsert { key, value, .. } => {
                        PlanTransientCollectionStep::MapUpsert {
                            key: self.lower_scoped(*key, owner)?,
                            value: self.lower_scoped(*value, owner)?,
                        }
                    }
                    ir::TransientCollectionStep::MapRemove { key, .. } => {
                        PlanTransientCollectionStep::MapRemove {
                            key: self.lower_scoped(*key, owner)?,
                        }
                    }
                    ir::TransientCollectionStep::SetAdd { item, .. } => {
                        PlanTransientCollectionStep::SetAdd {
                            item: self.lower_scoped(*item, owner)?,
                        }
                    }
                    ir::TransientCollectionStep::SetRemove { item, .. } => {
                        PlanTransientCollectionStep::SetRemove {
                            item: self.lower_scoped(*item, owner)?,
                        }
                    }
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let result = match region.result {
            ir::TransientCollectionResult::ListGet { position, .. } => {
                PlanTransientCollectionResult::ListGet {
                    position: self.lower_scoped(position, owner)?,
                }
            }
            ir::TransientCollectionResult::ListLength { .. } => {
                PlanTransientCollectionResult::ListLength
            }
            ir::TransientCollectionResult::ListIsNotEmpty { .. } => {
                PlanTransientCollectionResult::ListIsNotEmpty
            }
            ir::TransientCollectionResult::MapGet { key, .. } => {
                PlanTransientCollectionResult::MapGet {
                    key: self.lower_scoped(key, owner)?,
                }
            }
            ir::TransientCollectionResult::SetContains { item, .. } => {
                PlanTransientCollectionResult::SetContains {
                    item: self.lower_scoped(item, owner)?,
                }
            }
        };
        self.intern(PlanRowExpressionNode::TransientCollection {
            region: Box::new(PlanTransientCollection {
                kind,
                declared_capacity: region.declared_capacity,
                list_items,
                map_entries,
                set_items,
                steps,
                result,
                operation_work_budget: region.operation_work_budget,
                storage_growth_budget: region.storage_growth_budget,
                snapshot_copy_budget: region.snapshot_copy_budget,
            }),
        })
    }

    fn lower_local_read(
        &mut self,
        binding: ir::ExecutableLocalBindingId,
        declaration: boon_typecheck::DeclId,
        projection: &[String],
        inherited_owner: Option<PlanStaticOwnerId>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        let value = self
            .lexical_bindings
            .iter()
            .rev()
            .find_map(|bindings| bindings.get(&binding).copied())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "lexical binding {} for declaration {} has no active erased BLOCK binding",
                    binding.0, declaration.0
                ))
            })?;
        if !self.active_lexical_bindings.insert(binding) {
            return Err(PlanError::new(format!(
                "lexical binding {} for declaration {} forms an executable value cycle",
                binding.0, declaration.0
            )));
        }
        let lowered = self.lower_scoped(value, inherited_owner);
        self.active_lexical_bindings.remove(&binding);
        let mut lowered = lowered?;
        for field in projection {
            lowered = self.project_field(lowered, field.clone())?;
        }
        Ok(lowered)
    }

    fn lower_erased_read(
        &mut self,
        expression: ir::ExecutableExprId,
        inherited_owner: Option<PlanStaticOwnerId>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        let read_id = self.index.resolve_read(expression).ok_or_else(|| {
            PlanError::new(format!(
                "executable read {expression} has no exact erased read target"
            ))
        })?;
        let read = self
            .program
            .scope_index
            .reads
            .get(read_id.as_usize())
            .filter(|read| read.id == read_id && read.expression == expression)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "executable read {expression} references inconsistent erased read {read_id}"
                ))
            })?;
        match &read.target {
            ir::ErasedReadTarget::Binding {
                binding: storage_binding,
                projection,
            } => {
                let binding = self
                    .program
                    .scope_index
                    .bindings
                    .get(storage_binding.as_usize())
                    .filter(|binding| binding.id == *storage_binding)
                    .ok_or_else(|| {
                        PlanError::new(format!("erased read references missing {storage_binding}"))
                    })?;
                if !projection.is_empty() {
                    let exact_path = if binding.diagnostic_path.is_empty() {
                        projection.join(".")
                    } else {
                        format!("{}.{}", binding.diagnostic_path, projection.join("."))
                    };
                    if let Some(value) = self.index.resolve(&exact_path) {
                        return self.value_ref(value);
                    }
                }
                if matches!(
                    &binding.target,
                    ir::ErasedBindingTarget::Value {
                        field: None,
                        row: None
                    }
                ) {
                    if !self.active_storage_bindings.insert(*storage_binding) {
                        return Err(PlanError::new(format!(
                            "storage binding {storage_binding} `{}` forms an executable value cycle",
                            binding.diagnostic_path
                        )));
                    }
                    let lowered = self.lower_scoped(binding.producer, inherited_owner);
                    self.active_storage_bindings.remove(storage_binding);
                    let mut lowered = lowered?;
                    for field in projection {
                        lowered = self.project_field(lowered, field.clone())?;
                    }
                    return Ok(lowered);
                }
                let value = match binding.target {
                    ir::ErasedBindingTarget::Source { runtime, .. } => {
                        if !projection.is_empty() {
                            return Err(PlanError::new(format!(
                                "source binding {storage_binding} retained an unresolved projection"
                            )));
                        }
                        ValueRef::Source(plan_source_id(runtime))
                    }
                    ir::ErasedBindingTarget::State { runtime, .. } => {
                        if !projection.is_empty() {
                            return Err(PlanError::new(format!(
                                "state binding {storage_binding} retained an unresolved projection"
                            )));
                        }
                        ValueRef::State(plan_state_id(runtime))
                    }
                    ir::ErasedBindingTarget::Value { .. } => self
                        .index
                        .resolve_storage(*storage_binding)
                        .ok_or_else(|| {
                            PlanError::new(format!(
                                "storage binding {storage_binding} `{}` has no exact machine value for producer {} and target {:?}",
                                binding.diagnostic_path,
                                binding.producer,
                                binding.target,
                            ))
                        })?,
                };
                let mut value = self.value_ref(value)?;
                for field in projection {
                    value = self.project_field(value, field.clone())?;
                }
                Ok(value)
            }
            ir::ErasedReadTarget::SourcePayload {
                source,
                field,
                projection,
                ..
            } => {
                let mut value = self.value_ref(ValueRef::SourcePayload {
                    source_id: plan_source_id(*source),
                    field: source_payload_field_from_ir(field),
                })?;
                for field in projection {
                    value = self.project_field(value, field.clone())?;
                }
                Ok(value)
            }
            ir::ErasedReadTarget::StateProjection { state, fields, .. } => {
                self.value_ref(ValueRef::StateProjection {
                    state_id: plan_state_id(*state),
                    field_path: fields.clone(),
                })
            }
            ir::ErasedReadTarget::Expression {
                expression,
                projection,
            } => {
                let mut value = self.lower_scoped(*expression, inherited_owner)?;
                for field in projection {
                    value = self.project_field(value, field.clone())?;
                }
                Ok(value)
            }
            ir::ErasedReadTarget::Local {
                binding,
                declaration,
                value,
                projection,
            } => {
                let active = self
                    .lexical_bindings
                    .iter()
                    .rev()
                    .find_map(|bindings| bindings.get(binding).copied())
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "lexical binding {} for declaration {} has no active erased BLOCK binding",
                            binding.0, declaration.0
                        ))
                    })?;
                if active != *value {
                    return Err(PlanError::new(format!(
                        "lexical declaration {} active value {active} does not match erased value {value}",
                        declaration.0
                    )));
                }
                self.lower_local_read(*binding, *declaration, projection, inherited_owner)
            }
            ir::ErasedReadTarget::ExternalValue { reference } => {
                let reference = self
                    .program
                    .distributed_references
                    .value_references
                    .get(*reference)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "executable read {expression} references missing distributed value {reference}"
                        ))
                    })?;
                let checked = self
                    .program
                    .executable
                    .expressions
                    .get(read.expression.as_usize())
                    .filter(|expression| expression.id == read.expression)
                    .map(|expression| expression.checked_expr_id.0 as usize)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "distributed erased read {} has no executable expression",
                            read.expression
                        ))
                    })?;
                if reference.expr_id.as_usize() != checked {
                    return Err(PlanError::new(format!(
                        "distributed reference {reference:?} does not own executable read {}",
                        read.expression
                    )));
                }
                let value = self
                    .index
                    .resolve_distributed_expression(read.expression)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "distributed executable read {} has no typed machine value",
                            read.expression
                        ))
                    })?;
                self.value_ref(value)
            }
            ir::ErasedReadTarget::ElementState { .. }
            | ir::ErasedReadTarget::MaterializationLocal { .. }
            | ir::ErasedReadTarget::FunctionParameter { .. } => Err(PlanError::new(format!(
                "executable read {expression} reached an invalid indirect contextual target {:?}",
                read.target
            ))),
        }
    }

    fn materialization_local_member(
        &self,
        owner: ir::StaticOwnerId,
        local: ir::MaterializationLocalId,
        projection: &[String],
    ) -> Option<(ir::ErasedLocalMemberTarget, usize)> {
        self.program
            .scope_index
            .locals
            .iter()
            .find(|candidate| candidate.owner == owner && candidate.local == local)?
            .members
            .iter()
            .filter(|member| projection.starts_with(&member.path))
            .max_by_key(|member| member.path.len())
            .map(|member| (member.target, member.path.len()))
    }

    fn event_list_field(&self, list: ListId, name: &str) -> Result<FieldId, PlanError> {
        let list = self
            .program
            .lists
            .iter()
            .find(|candidate| plan_list_id(candidate.id) == list)
            .ok_or_else(|| {
                PlanError::new(format!("event row references missing ListId {}", list.0))
            })?;
        let candidates = self
            .program
            .scope_index
            .fields
            .iter()
            .filter(|field| field.row.map(|row| row.list) == Some(list.id) && field.name == name)
            .collect::<Vec<_>>();
        let authority = candidates
            .iter()
            .copied()
            .filter(|field| field.role.is_authority())
            .collect::<Vec<_>>();
        let preferred = if authority.is_empty() {
            let ownerless = candidates
                .iter()
                .copied()
                .filter(|field| {
                    erased_field_is_runtime_row_storage(self.program, field)
                        && field.static_owner.is_none()
                })
                .collect::<Vec<_>>();
            if ownerless.is_empty() {
                candidates
                    .iter()
                    .copied()
                    .filter(|field| erased_field_is_runtime_row_storage(self.program, field))
                    .collect::<Vec<_>>()
            } else {
                ownerless
            }
        } else {
            authority
        };
        let [field] = preferred.as_slice() else {
            return Err(PlanError::new(format!(
                "event row ListId {} member `{name}` resolves to {} exact fields among {:?}",
                list.id,
                preferred.len(),
                candidates
                    .iter()
                    .map(|field| (field.id, field.role, field.static_owner))
                    .collect::<Vec<_>>()
            )));
        };
        Ok(plan_field_id(field.id))
    }

    fn event_row_reaches_list(&self, source: ListId, target: ListId) -> bool {
        if source == target {
            return true;
        }
        let mut pending = vec![source];
        let mut visited = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            for materialization in &self.program.materializations {
                if !matches!(
                    materialization.operation,
                    ir::ContextualOperationKind::Map
                        | ir::ContextualOperationKind::Filter
                        | ir::ContextualOperationKind::Retain
                ) || materialization.source_list_id.map(plan_list_id) != Some(current)
                {
                    continue;
                }
                let Some(next) = materialization.target_list_id.map(plan_list_id) else {
                    continue;
                };
                if next == target {
                    return true;
                }
                pending.push(next);
            }
        }
        false
    }

    fn project_field(
        &mut self,
        value: PlanRowExpressionId,
        field: String,
    ) -> Result<PlanRowExpressionId, PlanError> {
        let Some(list_id) = self.direct_row_source(value)? else {
            return self.intern(PlanRowExpressionNode::ObjectField {
                object: value,
                field,
            });
        };
        let field_id =
            row_input_field_id_for_list_id(self.program, list_id, &field).ok_or_else(|| {
                let list = self
                    .program
                    .lists
                    .iter()
                    .find(|candidate| plan_list_id(candidate.id) == list_id);
                let fields = list
                    .map(|list| {
                        self.program
                            .scope_index
                            .fields
                            .iter()
                            .filter(|candidate| candidate.row.map(|row| row.list) == Some(list.id))
                            .map(|candidate| {
                                (
                                    candidate.name.as_str(),
                                    candidate.diagnostic_path.as_str(),
                                    candidate.role,
                                    candidate.static_owner,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                PlanError::new(format!(
                    "typed row expression {:?} from list {} (`{}`) has no compiler-owned field `{field}` among {fields:?}",
                    self.arena.node(value),
                    list_id.0,
                    list.map(|list| list.name.as_str()).unwrap_or("<missing>")
                ))
            })?;
        let input = ValueRef::List(list_id);
        if !self.inputs.contains(&input) {
            self.inputs.push(input);
        }
        self.intern(PlanRowExpressionNode::ListRowField {
            row: value,
            list_id,
            field: field_id,
        })
    }

    fn direct_row_source(
        &self,
        expression: PlanRowExpressionId,
    ) -> Result<Option<ListId>, PlanError> {
        if let Some(list) = self.stored_row_source(expression)? {
            return Ok(Some(list));
        }
        match self.arena.node(expression)? {
            PlanRowExpressionNode::LocalRow { owner, local } => {
                let lists = self
                    .program
                    .scope_index
                    .locals
                    .iter()
                    .filter(|candidate| {
                        PlanStaticOwnerId(candidate.owner.as_usize()) == *owner
                            && PlanLocalId(candidate.local.0 as usize) == *local
                    })
                    .filter_map(|candidate| candidate.row.map(|row| plan_list_id(row.list)))
                    .collect::<BTreeSet<_>>();
                match lists.into_iter().collect::<Vec<_>>().as_slice() {
                    [list] => Ok(Some(*list)),
                    [] => Ok(None),
                    lists => Err(PlanError::new(format!(
                        "contextual local {}:{} has multiple erased row owners {lists:?}",
                        owner.0, local.0
                    ))),
                }
            }
            PlanRowExpressionNode::EventRow { list_id, .. } => Ok(Some(*list_id)),
            PlanRowExpressionNode::ObjectField { object, field }
                if field == "value"
                    && matches!(
                        self.arena.node(*object)?,
                        PlanRowExpressionNode::ContextualCollection {
                            operation: PlanContextualOperationKind::Find,
                            ..
                        }
                    ) =>
            {
                let PlanRowExpressionNode::ContextualCollection { source, .. } =
                    self.arena.node(*object)?
                else {
                    unreachable!("guard proves contextual collection")
                };
                self.unique_row_source(*source)
            }
            PlanRowExpressionNode::ObjectField { object, field }
                if field == "value"
                    && matches!(
                        self.arena.node(*object)?,
                        PlanRowExpressionNode::BuiltinCall {
                            function: PlanRowBuiltin::ListGet,
                            input: Some(_),
                            ..
                        }
                    ) =>
            {
                let PlanRowExpressionNode::BuiltinCall {
                    input: Some(source),
                    ..
                } = self.arena.node(*object)?
                else {
                    unreachable!("guard proves List/get input")
                };
                self.unique_row_source(*source)
            }
            PlanRowExpressionNode::Select { arms, .. } => {
                let mut sources = BTreeSet::new();
                for arm in arms {
                    if let Some(source) = self.direct_row_source(arm.value)? {
                        sources.insert(source);
                    }
                }
                match sources.into_iter().collect::<Vec<_>>().as_slice() {
                    [list] => Ok(Some(*list)),
                    [] => Ok(None),
                    lists => Err(PlanError::new(format!(
                        "row-valued selection has multiple list owners {lists:?}"
                    ))),
                }
            }
            PlanRowExpressionNode::FlushBoundary { input } => self.direct_row_source(*input),
            _ => Ok(None),
        }
    }

    fn stored_row_source(
        &self,
        expression: PlanRowExpressionId,
    ) -> Result<Option<ListId>, PlanError> {
        fn root<'a>(
            arena: &'a PlanRowExpressionArena,
            expression: PlanRowExpressionId,
            projection: &mut Vec<String>,
        ) -> Result<Option<&'a ValueRef>, PlanError> {
            Ok(match arena.node(expression)? {
                PlanRowExpressionNode::Field { input } => Some(input),
                PlanRowExpressionNode::ObjectField { object, field } => {
                    let Some(input) = root(arena, *object, projection)? else {
                        return Ok(None);
                    };
                    projection.push(field.clone());
                    Some(input)
                }
                _ => None,
            })
        }

        let mut projection = Vec::new();
        let Some(value) = root(self.arena, expression, &mut projection)? else {
            return Ok(None);
        };
        self.index.row_source(value, &projection)
    }

    fn unique_row_source(
        &self,
        expression: PlanRowExpressionId,
    ) -> Result<Option<ListId>, PlanError> {
        let mut sources = BTreeSet::new();
        collect_row_result_sources(self.arena, expression, &[], &mut sources)?;
        match sources.into_iter().collect::<Vec<_>>().as_slice() {
            [list] => Ok(Some(*list)),
            [] => Ok(None),
            lists => Err(PlanError::new(format!(
                "row-valued expression has multiple list owners {lists:?}"
            ))),
        }
    }

    fn intern(&mut self, node: PlanRowExpressionNode) -> Result<PlanRowExpressionId, PlanError> {
        self.arena.intern(node)
    }

    fn value_ref(&mut self, value: ValueRef) -> Result<PlanRowExpressionId, PlanError> {
        if !self.inputs.contains(&value) {
            self.inputs.push(value.clone());
        }
        self.intern(match value {
            ValueRef::List(list_id) => PlanRowExpressionNode::ListRef { list_id },
            input => PlanRowExpressionNode::Field { input },
        })
    }

    fn constant(&mut self, value: PlanConstantValue) -> Result<PlanRowExpressionId, PlanError> {
        row_constant_expression(self.arena, self.constants, self.inputs, value)
    }

    fn bytes_constant(&mut self, bytes: Vec<u8>) -> Result<PlanRowExpressionId, PlanError> {
        row_bytes_constant_expression(self.arena, self.constants, self.inputs, bytes)
    }

    fn lower_fields(
        &mut self,
        fields: Vec<ir::ExecutableRecordField>,
        owner: Option<PlanStaticOwnerId>,
    ) -> Result<Vec<PlanRowObjectField>, PlanError> {
        fields
            .into_iter()
            .filter(|field| {
                !self.program.scope_index.fields.iter().any(|stored| {
                    stored.resource_only
                        && stored.producer == Some(field.value)
                        && stored.declaration == field.declaration
                        && stored.name == field.name
                        && stored
                            .static_owner
                            .map(|stored_owner| PlanStaticOwnerId(stored_owner.as_usize()))
                            == owner
                }) && !self.program.scope_index.bindings.iter().any(|binding| {
                    matches!(binding.target, ir::ErasedBindingTarget::Source { .. })
                        && binding.producer == field.value
                })
            })
            .map(|field| {
                Ok(PlanRowObjectField {
                    name: field.name,
                    value: self.lower_scoped(field.value, owner)?,
                    spread: field.spread,
                })
            })
            .collect()
    }

    fn lower_list_page(
        &mut self,
        input: Option<PlanRowExpressionId>,
        args: Vec<PlanRowCallArg>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        let mut names = BTreeSet::new();
        for argument in &args {
            if !matches!(argument.name.as_str(), "list" | "size" | "after") {
                return Err(PlanError::new(format!(
                    "List/page has unknown argument `{}`",
                    argument.name
                )));
            }
            if !names.insert(argument.name.as_str()) {
                return Err(PlanError::new(format!(
                    "List/page has duplicate argument `{}`",
                    argument.name
                )));
            }
        }
        if input.is_some() && names.contains("list") {
            return Err(PlanError::new("piped List/page cannot also declare `list`"));
        }
        let input = input
            .or_else(|| row_call_arg_value(&args, &["list"]))
            .ok_or_else(|| PlanError::new("List/page requires an input list"))?;
        let size = required_list_terminal_argument(&args, "size", "List/page")?;
        validate_literal_page_size(self.arena, self.constants, size)?;
        let after = required_list_terminal_argument(&args, "after", "List/page")?;
        let bounded_view = input;
        let (view, view_limit) = split_terminal_take(self.arena, input)?;
        let indexes = self.list_indexes.as_deref_mut().ok_or_else(|| {
            PlanError::new(
                "List/page is not valid in this executable context because no typed access owner is available",
            )
        })?;
        let accesses = plan_typed_list_access(
            self.program,
            self.index,
            self.arena,
            self.constants,
            view,
            size,
            indexes,
        )?;
        if let Some(accesses) = accesses {
            return build_directional_access_expression(self.arena, accesses, |access| {
                PlanRowExpressionNode::ListPage {
                    page: Box::new(PlanListPage {
                        access,
                        view_limit,
                        after,
                        view_fingerprint: [0; 32],
                    }),
                }
            });
        }

        let (bounded_view, _) = lower_exhaustive_candidate_view(
            self.program,
            self.index,
            self.arena,
            self.constants,
            bounded_view,
            size,
            indexes,
        )?;
        let bounded_view = lower_bounded_row_access(
            self.program,
            self.index,
            self.arena,
            self.constants,
            bounded_view,
            indexes,
        )?;
        if !bounded_list_page_view(self.arena, self.constants, bounded_view)? {
            return Err(PlanError::new(
                "typed List/page has no compiler-proven bounded source-order or keyed access path",
            ));
        }
        self.arena.intern(PlanRowExpressionNode::BoundedListPage {
            page: Box::new(PlanBoundedListPage {
                view: bounded_view,
                size,
                after,
                max_items: MAX_BOUNDED_LIST_PAGE_ITEMS,
                view_fingerprint: [0; 32],
            }),
        })
    }

    fn lower_call(
        &mut self,
        function: &str,
        arguments: Vec<ir::ExecutableCallArgument>,
        owner: Option<PlanStaticOwnerId>,
    ) -> Result<PlanRowExpressionId, PlanError> {
        if let Some(intrinsic) = session_info_intrinsic(function) {
            return self.intern(PlanRowExpressionNode::Intrinsic { intrinsic });
        }
        let mut input = None;
        let mut args = Vec::new();
        for argument in arguments {
            let value = if row_builtin_arg_expects_symbol(function, &argument.name) {
                match &self.program.executable.expressions[argument.value.as_usize()].kind {
                    ir::ExecutableExpressionKind::Tag(value)
                    | ir::ExecutableExpressionKind::Text(value) => {
                        self.constant(PlanConstantValue::Text {
                            value: value.clone(),
                        })?
                    }
                    _ => self.lower_scoped(argument.value, owner).map_err(|error| {
                        PlanError::new(format!(
                            "call `{function}` argument `{}` failed: {error}",
                            argument.name
                        ))
                    })?,
                }
            } else {
                self.lower_scoped(argument.value, owner).map_err(|error| {
                    PlanError::new(format!(
                        "call `{function}` argument `{}` failed: {error}",
                        argument.name
                    ))
                })?
            };
            if argument.from_pipe {
                if input.replace(value).is_some() {
                    return Err(PlanError::new(format!(
                        "executable call `{function}` has multiple pipe inputs"
                    )));
                }
            } else {
                args.push(PlanRowCallArg {
                    name: argument.name,
                    value,
                });
            }
        }
        if function == "List/page" {
            return self.lower_list_page(input, args);
        }
        if let Some(field) = function.strip_prefix("Field/") {
            let input = input
                .or_else(|| {
                    args.iter()
                        .find(|argument| argument.name == "input")
                        .map(|argument| argument.value)
                })
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "typed field projection `{function}` has no exact input"
                    ))
                })?;
            return self.project_field(input, field.to_owned());
        }
        if function == "Stream/pulses"
            && let Some(ValueRef::Pulse(pulse)) = self.event_trigger.as_ref()
        {
            return self.value_ref(ValueRef::Pulse(*pulse));
        }
        if function == "Stream/skip" && matches!(self.event_trigger, Some(ValueRef::Pulse(_))) {
            return input.ok_or_else(|| {
                PlanError::new("pulse-owned Stream/skip has no exact stream input")
            });
        }
        plan_builtin_expression(self.arena, function, input, args)
    }
}

fn plan_contextual_operation(
    value: ir::ContextualOperationKind,
) -> Result<PlanContextualOperationKind, PlanError> {
    Ok(match value {
        ir::ContextualOperationKind::Map => PlanContextualOperationKind::Map,
        ir::ContextualOperationKind::Filter => PlanContextualOperationKind::Filter,
        ir::ContextualOperationKind::Retain => PlanContextualOperationKind::Retain,
        ir::ContextualOperationKind::Remove => PlanContextualOperationKind::Remove,
        ir::ContextualOperationKind::Every => PlanContextualOperationKind::Every,
        ir::ContextualOperationKind::Any => PlanContextualOperationKind::Any,
        ir::ContextualOperationKind::Find => PlanContextualOperationKind::Find,
        ir::ContextualOperationKind::SortBy | ir::ContextualOperationKind::ThenBy => {
            return Err(PlanError::new(
                "typed ordering cannot lower as a predicate collection",
            ));
        }
    })
}

fn plan_order_operation(
    value: ir::ContextualOperationKind,
) -> Result<PlanOrderOperationKind, PlanError> {
    match value {
        ir::ContextualOperationKind::SortBy => Ok(PlanOrderOperationKind::SortBy),
        ir::ContextualOperationKind::ThenBy => Ok(PlanOrderOperationKind::ThenBy),
        _ => Err(PlanError::new(
            "non-order contextual operation cannot lower as typed ordering",
        )),
    }
}

fn executable_select_pattern(
    pattern: &boon_typecheck::CheckedMatchPattern,
) -> Result<PlanRowSelectPattern, PlanError> {
    use boon_typecheck::CheckedMatchPattern;

    Ok(match pattern {
        CheckedMatchPattern::Wildcard | CheckedMatchPattern::Binding { .. } => {
            PlanRowSelectPattern::Wildcard
        }
        CheckedMatchPattern::Number { value } => PlanRowSelectPattern::Number {
            value: value.clone(),
        },
        CheckedMatchPattern::Text { value } => PlanRowSelectPattern::Text {
            value: value.clone(),
        },
        CheckedMatchPattern::Bits { value } => PlanRowSelectPattern::Bits {
            value: value.clone(),
        },
        CheckedMatchPattern::Tag { name, .. } => PlanRowSelectPattern::Tag { name: name.clone() },
    })
}

fn executable_static_bytes(program: &ErasedProgram, root: ir::ExecutableExprId) -> Option<Vec<u8>> {
    match &program.executable.expressions.get(root.as_usize())?.kind {
        ir::ExecutableExpressionKind::BytesByte(value) => Some(vec![*value]),
        ir::ExecutableExpressionKind::Bytes { items, .. } => {
            let mut bytes = Vec::new();
            for item in items {
                bytes.extend(executable_static_bytes(program, *item)?);
            }
            Some(bytes)
        }
        _ => None,
    }
}

fn plan_builtin_expression(
    arena: &mut PlanRowExpressionArena,
    function: &str,
    mut input: Option<PlanRowExpressionId>,
    mut args: Vec<PlanRowCallArg>,
) -> Result<PlanRowExpressionId, PlanError> {
    let runtime_builtin = PlanRowBuiltin::from_function_name(function);
    if input.is_none()
        && let Some(receiver) = runtime_builtin.and_then(PlanRowBuiltin::receiver_parameter)
        && let Some(index) = args
            .iter()
            .position(|argument| argument.name == receiver.name)
    {
        input = Some(args.remove(index).value);
    }
    if let Some(runtime_builtin) = runtime_builtin {
        runtime_builtin.validate_call(input, &args)?;
    }
    let fallback_input = input;
    let named = |names: &[&str]| row_call_arg_value(&args, names);
    let expression = match function {
        "Text/trim" => input
            .or_else(|| named(&["input", "text"]))
            .map(|input| PlanRowExpressionNode::TextTrim { input }),
        "Text/is_empty" => input
            .or_else(|| named(&["input", "text"]))
            .map(|input| PlanRowExpressionNode::TextIsEmpty { input }),
        "Text/starts_with" => input
            .or_else(|| named(&["input", "text"]))
            .zip(named(&["prefix"]))
            .map(|(input, prefix)| PlanRowExpressionNode::TextStartsWith { input, prefix }),
        "Text/length" => input
            .or_else(|| named(&["input", "text"]))
            .map(|input| PlanRowExpressionNode::TextLength { input }),
        "Text/to_number" => input.or_else(|| named(&["input", "text"])).map(|input| {
            PlanRowExpressionNode::TextToNumber {
                input,
                radix: named(&["radix"]),
            }
        }),
        "Text/concat" => input
            .or_else(|| named(&["input", "text", "left"]))
            .zip(named(&["with", "right"]))
            .map(|(left, right)| {
                let mut parts = vec![left];
                if let Some(separator) = named(&["separator"]) {
                    parts.push(separator);
                }
                parts.push(right);
                PlanRowExpressionNode::TextConcat { parts }
            }),
        "Text/slice" => input
            .or_else(|| named(&["input", "text"]))
            .zip(named(&["from"]))
            .zip(named(&["count"]))
            .map(|((input, from), count)| PlanRowExpressionNode::TextSlice { input, from, count }),
        "Text/to_bytes" => input.or_else(|| named(&["input", "text"])).map(|input| {
            PlanRowExpressionNode::TextToBytes {
                input,
                encoding: named(&["encoding"]),
            }
        }),
        "Bytes/to_text" => input.or_else(|| named(&["input", "bytes"])).map(|input| {
            PlanRowExpressionNode::BytesToText {
                input,
                encoding: named(&["encoding"]),
            }
        }),
        "Bytes/to_hex" => input
            .or_else(|| named(&["input", "bytes"]))
            .map(|input| PlanRowExpressionNode::BytesToHex { input }),
        "Bytes/to_base64" => input
            .or_else(|| named(&["input", "bytes"]))
            .map(|input| PlanRowExpressionNode::BytesToBase64 { input }),
        "Bytes/from_hex" => input
            .or_else(|| named(&["input", "text"]))
            .map(|input| PlanRowExpressionNode::BytesFromHex { input }),
        "Bytes/from_base64" => input
            .or_else(|| named(&["input", "text"]))
            .map(|input| PlanRowExpressionNode::BytesFromBase64 { input }),
        "Bytes/is_empty" => input
            .or_else(|| named(&["input"]))
            .map(|input| PlanRowExpressionNode::BytesIsEmpty { input }),
        "Bytes/length" => input
            .or_else(|| named(&["input"]))
            .map(|input| PlanRowExpressionNode::BytesLength { input }),
        "Bytes/get" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["position"]))
            .map(|(input, position)| PlanRowExpressionNode::BytesGet { input, position }),
        "Bytes/read_unsigned" | "Bytes/read_signed" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["from"]))
            .zip(named(&["byte_count"]))
            .zip(named(&["endian"]))
            .map(|(((input, from), byte_count), endian)| {
                if function == "Bytes/read_signed" {
                    PlanRowExpressionNode::BytesReadSigned {
                        input,
                        from,
                        byte_count,
                        endian,
                    }
                } else {
                    PlanRowExpressionNode::BytesReadUnsigned {
                        input,
                        from,
                        byte_count,
                        endian,
                    }
                }
            }),
        "Bytes/slice" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["from"]))
            .zip(named(&["count"]))
            .map(|((input, from), count)| PlanRowExpressionNode::BytesSlice { input, from, count }),
        "Bytes/take" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["count"]))
            .map(|(input, count)| PlanRowExpressionNode::BytesTake { input, count }),
        "Bytes/drop" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["count"]))
            .map(|(input, count)| PlanRowExpressionNode::BytesDrop { input, count }),
        "Bytes/zeros" => named(&["count"]).map(|count| PlanRowExpressionNode::BytesZeros { count }),
        "Bytes/set" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["position"]))
            .zip(named(&["value"]))
            .map(
                |((input, position), value)| PlanRowExpressionNode::BytesSet {
                    input,
                    position,
                    value,
                },
            ),
        "Bytes/write_unsigned" | "Bytes/write_signed" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["from"]))
            .zip(named(&["byte_count"]))
            .zip(named(&["endian"]))
            .zip(named(&["value"]))
            .map(|((((input, from), byte_count), endian), value)| {
                if function == "Bytes/write_signed" {
                    PlanRowExpressionNode::BytesWriteSigned {
                        input,
                        from,
                        byte_count,
                        endian,
                        value,
                    }
                } else {
                    PlanRowExpressionNode::BytesWriteUnsigned {
                        input,
                        from,
                        byte_count,
                        endian,
                        value,
                    }
                }
            }),
        "Bytes/find" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["needle"]))
            .map(|(input, needle)| PlanRowExpressionNode::BytesFind { input, needle }),
        "Bytes/starts_with" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["prefix"]))
            .map(|(input, prefix)| PlanRowExpressionNode::BytesStartsWith { input, prefix }),
        "Bytes/ends_with" => input
            .or_else(|| named(&["input"]))
            .zip(named(&["suffix"]))
            .map(|(input, suffix)| PlanRowExpressionNode::BytesEndsWith { input, suffix }),
        "Bytes/concat" => input
            .or_else(|| named(&["left", "input"]))
            .zip(named(&["right", "with"]))
            .map(|(left, right)| PlanRowExpressionNode::BytesConcat { left, right }),
        "Bytes/equal" => input
            .or_else(|| named(&["left", "input"]))
            .zip(named(&["right", "with"]))
            .map(|(left, right)| PlanRowExpressionNode::BytesEqual { left, right }),
        "List/range" => named(&["from"])
            .zip(named(&["to"]))
            .map(|(from, to)| PlanRowExpressionNode::ListRange { from, to }),
        "List/sum" => input
            .or_else(|| named(&["input", "list"]))
            .map(|input| PlanRowExpressionNode::ListSum { input }),
        "Dependency/catch_cycle" => input
            .or_else(|| named(&["value"]))
            .zip(named(&["on_cycle"]))
            .map(|(input, on_cycle)| PlanRowExpressionNode::CatchCycle { input, on_cycle }),
        "List/get" | "List/latest" | "List/count" | "List/length" | "List/is_not_empty"
        | "List/take" => input.or_else(|| named(&["input", "list"])).map(|input| {
            PlanRowExpressionNode::BuiltinCall {
                function: runtime_builtin.expect("typed list terminal builtin"),
                input: Some(input),
                args: args
                    .iter()
                    .filter(|argument| !matches!(argument.name.as_str(), "input" | "list"))
                    .cloned()
                    .collect(),
            }
        }),
        "Text/all_chars_in" => {
            input
                .or_else(|| named(&["input"]))
                .map(|input| PlanRowExpressionNode::BuiltinCall {
                    function: runtime_builtin.expect("typed text predicate builtin"),
                    input: Some(input),
                    args: args
                        .iter()
                        .filter(|argument| argument.name != "input")
                        .cloned()
                        .collect(),
                })
        }
        _ => None,
    };
    if let Some(expression) = expression {
        return arena.intern(expression);
    }
    let function = runtime_builtin.ok_or_else(|| {
        PlanError::new(format!(
            "call `{function}` has no typed PlanExecutor row operation"
        ))
    })?;
    arena.intern(PlanRowExpressionNode::BuiltinCall {
        function,
        input: fallback_input,
        args,
    })
}

fn row_expression_value_type(
    program: &ErasedProgram,
    index: &ValueIndex,
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    expression: PlanRowExpressionId,
) -> Result<Option<PlanValueType>, PlanError> {
    Ok(match arena.node(expression)? {
        PlanRowExpressionNode::Absent => None,
        PlanRowExpressionNode::Flush { payload }
        | PlanRowExpressionNode::FlushBoundary { input: payload } => {
            row_expression_value_type(program, index, arena, constants, *payload)?
        }
        PlanRowExpressionNode::CatchCycle { input, on_cycle } => {
            let input_type = row_expression_value_type(program, index, arena, constants, *input)?;
            let cycle_type =
                row_expression_value_type(program, index, arena, constants, *on_cycle)?;
            match (input_type, cycle_type) {
                (Some(input_type), Some(cycle_type)) if input_type == cycle_type => {
                    Some(input_type)
                }
                (Some(_), Some(_)) => Some(PlanValueType::Data),
                (input_type, cycle_type) => input_type.or(cycle_type),
            }
        }
        PlanRowExpressionNode::Intrinsic { .. } => Some(PlanValueType::Tag),
        PlanRowExpressionNode::EffectResult
        | PlanRowExpressionNode::TransientEffectResult { .. } => Some(PlanValueType::Data),
        PlanRowExpressionNode::Field { input } => {
            plan_value_type_for_value_ref(program, index, input)
        }
        PlanRowExpressionNode::Constant { constant_id } => constants
            .iter()
            .find(|constant| constant.id == *constant_id)
            .and_then(|constant| match &constant.value {
                PlanConstantValue::Text { .. } => Some(PlanValueType::Text),
                PlanConstantValue::Number { .. } => Some(PlanValueType::Number),
                PlanConstantValue::Bytes { byte_len, .. } => Some(PlanValueType::Bytes {
                    fixed_len: Some(*byte_len),
                }),
                PlanConstantValue::Tag { .. } => Some(PlanValueType::Tag),
                PlanConstantValue::Bits { value } => Some(PlanValueType::Bits {
                    width: value.width(),
                }),
                PlanConstantValue::Data { .. } => Some(PlanValueType::Data),
            }),
        PlanRowExpressionNode::TextTrim { .. }
        | PlanRowExpressionNode::TextSlice { .. }
        | PlanRowExpressionNode::TextConcat { .. }
        | PlanRowExpressionNode::BytesToText { .. }
        | PlanRowExpressionNode::BytesToHex { .. }
        | PlanRowExpressionNode::BytesToBase64 { .. } => Some(PlanValueType::Text),
        PlanRowExpressionNode::TextToBytes { .. }
        | PlanRowExpressionNode::BytesSlice { .. }
        | PlanRowExpressionNode::BytesTake { .. }
        | PlanRowExpressionNode::BytesDrop { .. }
        | PlanRowExpressionNode::BytesZeros { .. }
        | PlanRowExpressionNode::BytesSet { .. }
        | PlanRowExpressionNode::BytesWriteUnsigned { .. }
        | PlanRowExpressionNode::BytesWriteSigned { .. }
        | PlanRowExpressionNode::BytesConcat { .. }
        | PlanRowExpressionNode::BytesFromHex { .. }
        | PlanRowExpressionNode::BytesFromBase64 { .. } => {
            Some(PlanValueType::Bytes { fixed_len: None })
        }
        PlanRowExpressionNode::NumberInfix { op, .. } if op.is_comparison() => {
            Some(PlanValueType::Tag)
        }
        PlanRowExpressionNode::BytesLength { .. }
        | PlanRowExpressionNode::BytesReadUnsigned { .. }
        | PlanRowExpressionNode::BytesReadSigned { .. }
        | PlanRowExpressionNode::TextLength { .. }
        | PlanRowExpressionNode::NumberInfix { .. }
        | PlanRowExpressionNode::ListSum { .. } => Some(PlanValueType::Number),
        PlanRowExpressionNode::BytesIsEmpty { .. }
        | PlanRowExpressionNode::BytesStartsWith { .. }
        | PlanRowExpressionNode::BytesEndsWith { .. }
        | PlanRowExpressionNode::BytesEqual { .. }
        | PlanRowExpressionNode::BytesGet { .. }
        | PlanRowExpressionNode::BytesFind { .. }
        | PlanRowExpressionNode::TextIsEmpty { .. }
        | PlanRowExpressionNode::TextStartsWith { .. }
        | PlanRowExpressionNode::TextToNumber { .. } => Some(PlanValueType::Tag),
        PlanRowExpressionNode::TransientCollection { region } => match &region.result {
            PlanTransientCollectionResult::ListLength => Some(PlanValueType::Number),
            PlanTransientCollectionResult::ListGet { .. }
            | PlanTransientCollectionResult::ListIsNotEmpty
            | PlanTransientCollectionResult::MapGet { .. }
            | PlanTransientCollectionResult::SetContains { .. } => Some(PlanValueType::Tag),
        },
        PlanRowExpressionNode::BuiltinCall {
            function,
            input,
            args,
        } => {
            bits_row_builtin_value_type(program, index, arena, constants, *function, *input, args)?
                .or_else(|| function.fixed_result_type())
        }
        PlanRowExpressionNode::Select { arms, .. } => {
            let mut arm_types = Vec::new();
            for arm in arms {
                if let Some(value_type) =
                    row_expression_value_type(program, index, arena, constants, arm.value)?
                {
                    arm_types.push(value_type);
                }
            }
            let Some(first) = arm_types.first().copied() else {
                return Ok(None);
            };
            arm_types
                .into_iter()
                .all(|arm_type| arm_type == first)
                .then_some(first)
        }
        PlanRowExpressionNode::ListGetField { field, .. }
        | PlanRowExpressionNode::ListRowField { field, .. } => {
            index.field_value_type(*field).copied()
        }
        PlanRowExpressionNode::ListRef { .. }
        | PlanRowExpressionNode::AuthorityListRef { .. }
        | PlanRowExpressionNode::ListRange { .. }
        | PlanRowExpressionNode::ListLiteral { .. }
        | PlanRowExpressionNode::MapLiteral { .. }
        | PlanRowExpressionNode::SetLiteral { .. }
        | PlanRowExpressionNode::ContextualCollection { .. }
        | PlanRowExpressionNode::ContextualOrder { .. }
        | PlanRowExpressionNode::ListAccess { .. }
        | PlanRowExpressionNode::ListPage { .. }
        | PlanRowExpressionNode::BoundedListPage { .. }
        | PlanRowExpressionNode::Local { .. }
        | PlanRowExpressionNode::LocalRow { .. }
        | PlanRowExpressionNode::EventRow { .. }
        | PlanRowExpressionNode::Object { .. }
        | PlanRowExpressionNode::TaggedObject { .. }
        | PlanRowExpressionNode::ObjectField { .. } => None,
    })
}

fn bits_row_builtin_value_type(
    program: &ErasedProgram,
    index: &ValueIndex,
    arena: &PlanRowExpressionArena,
    constants: &[PlanConstant],
    function: PlanRowBuiltin,
    input: Option<PlanRowExpressionId>,
    args: &[PlanRowCallArg],
) -> Result<Option<PlanValueType>, PlanError> {
    let input_type = match input {
        Some(input) => row_expression_value_type(program, index, arena, constants, input)?,
        None => None,
    };
    let input_width = match input_type {
        Some(PlanValueType::Bits { width }) => Some(width),
        _ => None,
    };
    let arg = |name: &str| {
        args.iter()
            .find(|argument| argument.name == name)
            .map(|argument| argument.value)
    };
    let arg_bits_width = |name: &str| -> Result<Option<u32>, PlanError> {
        let Some(expression) = arg(name) else {
            return Ok(None);
        };
        Ok(
            match row_expression_value_type(program, index, arena, constants, expression)? {
                Some(PlanValueType::Bits { width }) => Some(width),
                _ => None,
            },
        )
    };
    let static_width = |name: &str| {
        let expression = arg(name)?;
        let PlanRowExpressionNode::Constant { constant_id } = arena.node(expression).ok()? else {
            return None;
        };
        constants
            .iter()
            .find(|constant| constant.id == *constant_id)
            .and_then(|constant| match &constant.value {
                PlanConstantValue::Number { value } => value
                    .to_u64_exact()
                    .ok()
                    .and_then(|value| u32::try_from(value).ok()),
                _ => None,
            })
    };

    Ok(match function {
        PlanRowBuiltin::BitsSet
        | PlanRowBuiltin::BitsSetSlice
        | PlanRowBuiltin::BitsAnd
        | PlanRowBuiltin::BitsOr
        | PlanRowBuiltin::BitsXor
        | PlanRowBuiltin::BitsNot
        | PlanRowBuiltin::BitsShiftLeft
        | PlanRowBuiltin::BitsShiftRight
        | PlanRowBuiltin::BitsShiftRightArithmetic
        | PlanRowBuiltin::BitsRotateLeft
        | PlanRowBuiltin::BitsRotateRight
        | PlanRowBuiltin::BitsAddOrWrap
        | PlanRowBuiltin::BitsSubtractOrWrap => input_type,
        PlanRowBuiltin::BitsSlice => {
            static_width("count").map(|width| PlanValueType::Bits { width })
        }
        PlanRowBuiltin::BitsConcat => input_width
            .zip(arg_bits_width("with")?)
            .and_then(|(left, right)| left.checked_add(right))
            .map(|width| PlanValueType::Bits { width }),
        PlanRowBuiltin::BitsZeroExtend
        | PlanRowBuiltin::BitsSignExtend
        | PlanRowBuiltin::BitsTruncate
        | PlanRowBuiltin::BytesToBits => {
            static_width("width").map(|width| PlanValueType::Bits { width })
        }
        PlanRowBuiltin::BitsAddWidening => input_width
            .and_then(|width| width.checked_add(1))
            .map(|width| PlanValueType::Bits { width }),
        PlanRowBuiltin::BitsToBytes => input_width.map(|width| PlanValueType::Bytes {
            fixed_len: Some(u64::from(width / 8)),
        }),
        _ => None,
    })
}

fn session_info_intrinsic(function: &str) -> Option<PlanIntrinsic> {
    match function {
        "SessionInfo/status" => Some(PlanIntrinsic::SessionInfoStatus),
        "SessionInfo/principal" => Some(PlanIntrinsic::SessionInfoPrincipal),
        _ => None,
    }
}

fn row_call_arg_value(args: &[PlanRowCallArg], names: &[&str]) -> Option<PlanRowExpressionId> {
    args.iter()
        .find(|arg| names.contains(&arg.name.as_str()))
        .map(|arg| arg.value)
}

fn row_builtin_arg_expects_symbol(function: &str, arg_name: &str) -> bool {
    matches!(
        (function, arg_name),
        (_, "encoding")
            | (
                "Bytes/read_unsigned"
                    | "Bytes/read_signed"
                    | "Bytes/write_unsigned"
                    | "Bytes/write_signed",
                "endian"
            )
    )
}

fn row_constant_expression(
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    value: PlanConstantValue,
) -> Result<PlanRowExpressionId, PlanError> {
    let constant_id = push_plan_constant(constants, value);
    if !inputs.contains(&ValueRef::Constant(constant_id)) {
        inputs.push(ValueRef::Constant(constant_id));
    }
    arena.intern(PlanRowExpressionNode::Constant { constant_id })
}

fn row_bytes_constant_expression(
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    bytes: Vec<u8>,
) -> Result<PlanRowExpressionId, PlanError> {
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    row_constant_expression(
        arena,
        constants,
        inputs,
        PlanConstantValue::Bytes {
            byte_len: bytes.len() as u64,
            sha256: format!("{:x}", hasher.finalize()),
            inline_bytes: (bytes.len() <= INLINE_BYTE_CONSTANT_LIMIT).then_some(bytes),
        },
    )
}

fn row_expression_for_value(
    program: &ErasedProgram,
    derived: &boon_ir::DerivedValue,
    index: &ValueIndex,
    _distributed: &DistributedMachineContext,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    list_indexes: &mut Vec<PlanListIndex>,
) -> Result<Option<PlanDerivedExpression>, PlanError> {
    if !matches!(
        derived.kind,
        DerivedValueKind::Pure | DerivedValueKind::ListView | DerivedValueKind::Aggregate
    ) {
        return Ok(None);
    }
    if let Some(state) = derived.state_backing {
        let state = plan_state_id(state);
        if !program
            .state_cells
            .iter()
            .any(|candidate| plan_state_id(candidate.id) == state)
        {
            return Err(PlanError::new(format!(
                "derived value `{}` references missing exact state backing {}",
                derived.path, state.0
            )));
        }
        let input = ValueRef::State(state);
        if !inputs.contains(&input) {
            inputs.push(input.clone());
        }
        let expression = arena.intern(PlanRowExpressionNode::Field { input })?;
        return Ok(Some(PlanDerivedExpression::RowExpression { expression }));
    }
    let root = program
        .executable
        .expressions
        .get(derived.producer.as_usize())
        .filter(|expression| expression.id == derived.producer)
        .map(|_| derived.producer)
        .ok_or_else(|| {
            PlanError::new(format!(
                "derived value `{}` has no executable root",
                derived.path
            ))
        })?;
    let expression = ExecutableRowLowerer::new(program, index, arena, constants, inputs)
        .with_list_indexes(list_indexes)
        .lower(root)
        .map_err(|error| {
            PlanError::new(format!(
                "derived value `{}` failed executable lowering: {error}",
                derived.path
            ))
        })?;
    Ok(Some(PlanDerivedExpression::RowExpression { expression }))
}

fn executable_expression_reaches(
    program: &ErasedProgram,
    root: ir::ExecutableExprId,
    target: ir::ExecutableExprId,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        if expression_id == target {
            return true;
        }
        let Some(expression) = program.executable.expressions.get(expression_id.as_usize()) else {
            continue;
        };
        pending.extend(ir::executable_expression_children(&expression.kind));
    }
    false
}

fn exact_host_effect_expression(
    program: &ErasedProgram,
    root: ir::ExecutableExprId,
) -> Result<Option<ir::ExecutableExprId>, PlanError> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    let mut effects = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = program
            .executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| {
                PlanError::new(format!("missing executable expression {expression_id}"))
            })?;
        if matches!(
            &expression.kind,
            ir::ExecutableExpressionKind::Call { name, .. }
                if boon_typecheck::is_typed_host_effect(name)
        ) {
            effects.insert(expression_id);
        } else {
            pending.extend(ir::executable_expression_children(&expression.kind));
        }
    }
    match effects.into_iter().collect::<Vec<_>>().as_slice() {
        [] => Ok(None),
        [effect] => Ok(Some(*effect)),
        effects => Err(PlanError::new(format!(
            "state update expression {root} reaches multiple host effects {effects:?}"
        ))),
    }
}

struct HostEffectPlanParts {
    operation: String,
    gate: PlanRowExpressionId,
    intent_expressions: Vec<(String, PlanRowExpressionId)>,
}

fn exact_host_effect_plan_parts(
    program: &ErasedProgram,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    trigger: &ValueRef,
    active_state: Option<ir::ExecutableStateId>,
    output: ir::ExecutableExprId,
    effect: ir::ExecutableExprId,
) -> Result<HostEffectPlanParts, PlanError> {
    let expression = program
        .executable
        .expressions
        .get(effect.as_usize())
        .filter(|expression| expression.id == effect)
        .ok_or_else(|| PlanError::new(format!("missing exact host effect expression {effect}")))?;
    let ir::ExecutableExpressionKind::Call {
        name, arguments, ..
    } = &expression.kind
    else {
        return Err(PlanError::new(format!(
            "exact host effect expression {effect} is not a call"
        )));
    };
    let contract = builtin_effect_contract(name)?.ok_or_else(|| {
        PlanError::new(format!(
            "host effect `{name}` has no centralized effect contract"
        ))
    })?;
    let Some(EffectSchemaPlan {
        intent_type: DataTypePlan::Record {
            fields,
            open: false,
        },
        intent_defaults,
        ..
    }) = contract.schema
    else {
        return Err(PlanError::new(format!(
            "host effect `{name}` has no closed typed intent schema"
        )));
    };
    let mut lowerer = ExecutableRowLowerer::new(program, index, arena, constants, inputs)
        .with_event_trigger(trigger);
    if let Some(active_state) = active_state {
        lowerer = lowerer.with_state_update(active_state);
    }
    let gate = lowerer.lower_reachability_gate(output, effect)?;
    let mut intent_expressions = Vec::with_capacity(fields.len());
    for field in fields {
        let value = if let Some(argument) = arguments
            .iter()
            .find(|argument| argument.name == field.name)
        {
            lowerer.lower(argument.value).map_err(|error| {
                PlanError::new(format!(
                    "host effect `{name}` intent field `{}` failed exact lowering: {error}",
                    field.name
                ))
            })?
        } else {
            let default = intent_defaults
                .iter()
                .find(|default| default.field_name == field.name)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "host effect `{name}` intent field `{}` has neither an executable argument nor a typed default",
                        field.name
                    ))
                })?;
            lowerer.constant(effect_intent_default_constant(&default.value))?
        };
        intent_expressions.push((field.name, value));
    }
    Ok(HostEffectPlanParts {
        operation: name.clone(),
        gate,
        intent_expressions,
    })
}

#[allow(clippy::too_many_arguments)]
fn exact_host_effect_result_transform(
    program: &ErasedProgram,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    inputs: &mut Vec<ValueRef>,
    trigger: &ValueRef,
    active_state: Option<ir::ExecutableStateId>,
    output: ir::ExecutableExprId,
    effect: ir::ExecutableExprId,
) -> Result<Option<PlanRowExpressionId>, PlanError> {
    if output == effect {
        return Ok(None);
    }
    let result = arena.intern(PlanRowExpressionNode::EffectResult)?;
    let mut lowerer = ExecutableRowLowerer::new(program, index, arena, constants, inputs)
        .with_bindings(BTreeMap::from([(effect, result)]))
        .with_event_trigger(trigger);
    if let Some(active_state) = active_state {
        lowerer = lowerer.with_state_update(active_state);
    }
    let transform = lowerer
        .lower(output)
        .map_err(|error| {
            PlanError::new(format!(
                "host effect result continuation from expression {effect} to output {output} failed exact lowering: {error}"
            ))
        })?;
    Ok((transform != result).then_some(transform))
}

fn push_plan_constant(
    constants: &mut Vec<PlanConstant>,
    value: PlanConstantValue,
) -> PlanConstantId {
    if let Some(existing) = constants
        .iter()
        .find(|constant| constant.value == value)
        .map(|constant| constant.id)
    {
        return existing;
    }
    let id = PlanConstantId(constants.len());
    constants.push(PlanConstant { id, value });
    id
}

fn invocation_result_sources_in_row(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    invocation_sources: &BTreeSet<SourceId>,
) -> Result<BTreeSet<SourceId>, PlanError> {
    let mut sources = BTreeSet::new();
    arena.visit_value_refs(expression, &mut |value| {
        if let ValueRef::SourcePayload { source_id, .. } = value
            && invocation_sources.contains(source_id)
        {
            sources.insert(*source_id);
        }
    })?;
    Ok(sources)
}

fn validate_invocation_result_continuation(
    arena: &PlanRowExpressionArena,
    expression: PlanRowExpressionId,
    result_source: SourceId,
    diagnostic: &str,
) -> Result<(), PlanError> {
    let mut stale_source = None;
    arena.visit_value_refs(expression, &mut |value| match value {
        ValueRef::Source(source) if *source != result_source => stale_source = Some(*source),
        ValueRef::SourcePayload { source_id, .. } if *source_id != result_source => {
            stale_source = Some(*source_id)
        }
        _ => {}
    })?;
    if let Some(source) = stale_source {
        return Err(PlanError::new(format!(
            "{diagnostic} reads transient source {} after an asynchronous distributed invocation; capture the required value in current state before the call",
            source.0
        )));
    }
    Ok(())
}

fn single_invocation_result_source(
    sources: BTreeSet<SourceId>,
    diagnostic: &str,
) -> Result<Option<SourceId>, PlanError> {
    if sources.len() > 1 {
        return Err(PlanError::new(format!(
            "{diagnostic} joins multiple asynchronous distributed invocation results without an explicit temporal join"
        )));
    }
    Ok(sources.into_iter().next())
}

fn retarget_derived_invocation_results(
    arena: &PlanRowExpressionArena,
    expression: &mut PlanDerivedExpression,
    invocation_sources: &BTreeSet<SourceId>,
    diagnostic: &str,
) -> Result<Vec<ValueRef>, PlanError> {
    match expression {
        PlanDerivedExpression::SourceEventTransform { default, arms, .. } => {
            if !invocation_result_sources_in_row(arena, *default, invocation_sources)?.is_empty() {
                return Err(PlanError::new(format!(
                    "{diagnostic} uses an asynchronous distributed invocation result as an event default"
                )));
            }
            let mut triggers = Vec::new();
            for arm in arms {
                let source = single_invocation_result_source(
                    invocation_result_sources_in_row(arena, arm.value, invocation_sources)?,
                    diagnostic,
                )?;
                let Some(source) = source else {
                    continue;
                };
                validate_invocation_result_continuation(arena, arm.value, source, diagnostic)?;
                arm.trigger = ValueRef::Source(source);
                triggers.push(arm.trigger.clone());
            }
            Ok(triggers)
        }
        PlanDerivedExpression::RowExpression { expression }
        | PlanDerivedExpression::MaterializedRowField { expression, .. } => {
            if invocation_result_sources_in_row(arena, *expression, invocation_sources)?.is_empty()
            {
                Ok(Vec::new())
            } else {
                Err(PlanError::new(format!(
                    "{diagnostic} reads an asynchronous distributed invocation result outside an event-owned continuation"
                )))
            }
        }
        PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
            source_id, state, ..
        } => {
            let reads_result = invocation_sources.contains(source_id)
                || matches!(state, ValueRef::SourcePayload { source_id, .. } if invocation_sources.contains(source_id));
            if reads_result {
                Err(PlanError::new(format!(
                    "{diagnostic} uses an invocation result in a specialized synchronous source transform"
                )))
            } else {
                Ok(Vec::new())
            }
        }
        PlanDerivedExpression::BoolNot { input } => {
            let reads_result = matches!(input, ValueRef::SourcePayload { source_id, .. } if invocation_sources.contains(source_id));
            if reads_result {
                Err(PlanError::new(format!(
                    "{diagnostic} reads an invocation result outside an event-owned continuation"
                )))
            } else {
                Ok(Vec::new())
            }
        }
        PlanDerivedExpression::NumberCompareConst { left, .. } => {
            let reads_result = matches!(left, ValueRef::SourcePayload { source_id, .. } if invocation_sources.contains(source_id));
            if reads_result {
                Err(PlanError::new(format!(
                    "{diagnostic} reads an invocation result outside an event-owned continuation"
                )))
            } else {
                Ok(Vec::new())
            }
        }
        PlanDerivedExpression::ValueCompare { left, right, .. } => {
            let reads_result = [left, right].into_iter().any(|value| {
                matches!(value, ValueRef::SourcePayload { source_id, .. } if invocation_sources.contains(source_id))
            });
            if reads_result {
                Err(PlanError::new(format!(
                    "{diagnostic} reads an invocation result outside an event-owned continuation"
                )))
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn effect_invocation_result_source(
    arena: &PlanRowExpressionArena,
    effect: &EffectInvocationPlan,
    invocation_sources: &BTreeSet<SourceId>,
    diagnostic: &str,
) -> Result<Option<SourceId>, PlanError> {
    let mut sources = invocation_result_sources_in_row(arena, effect.gate, invocation_sources)?;
    for field in &effect.intent_fields {
        sources.extend(invocation_result_sources_in_row(
            arena,
            field.expression,
            invocation_sources,
        )?);
    }
    let source = single_invocation_result_source(sources, diagnostic)?;
    if let Some(source) = source {
        validate_invocation_result_continuation(arena, effect.gate, source, diagnostic)?;
        for field in &effect.intent_fields {
            validate_invocation_result_continuation(arena, field.expression, source, diagnostic)?;
        }
    }
    Ok(source)
}

fn retarget_invocation_result_operations(
    program: &ErasedProgram,
    arena: &PlanRowExpressionArena,
    regions: &mut [OperationRegion],
    invocation_sources: &BTreeSet<SourceId>,
) -> Result<(), PlanError> {
    if invocation_sources.is_empty() {
        return Ok(());
    }
    for op in regions.iter_mut().flat_map(|region| &mut region.ops) {
        let output_label = match op.output {
            Some(ValueRef::Field(field)) => program
                .scope_index
                .fields
                .get(field.0)
                .map(|field| field.diagnostic_path.as_str()),
            Some(ValueRef::State(state)) => program
                .state_cells
                .get(state.0)
                .map(|state| state.path.as_str()),
            _ => None,
        };
        let diagnostic = output_label.map_or_else(
            || format!("plan operation {} output {:?}", op.id.0, op.output),
            |label| format!("plan operation {} `{label}`", op.id.0),
        );
        let mut superseded_triggers = Vec::new();
        let mut result_triggers = match &mut op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(expression),
                ..
            } => retarget_derived_invocation_results(
                arena,
                expression,
                invocation_sources,
                &diagnostic,
            )?,
            PlanOpKind::StateUpdate {
                trigger,
                value,
                effect,
            } => {
                let mut sources = value
                    .map(|value| invocation_result_sources_in_row(arena, value, invocation_sources))
                    .transpose()?
                    .unwrap_or_default();
                if let Some(effect) = effect
                    && let Some(source) = effect_invocation_result_source(
                        arena,
                        effect,
                        invocation_sources,
                        &diagnostic,
                    )?
                {
                    sources.insert(source);
                }
                let source = single_invocation_result_source(sources, &diagnostic)?;
                if let Some(source) = source {
                    if let Some(value) = value {
                        validate_invocation_result_continuation(
                            arena,
                            *value,
                            source,
                            &diagnostic,
                        )?;
                    }
                    superseded_triggers.push(trigger.clone());
                    *trigger = ValueRef::Source(source);
                    vec![trigger.clone()]
                } else {
                    Vec::new()
                }
            }
            PlanOpKind::EffectUpdate {
                trigger,
                result_transform,
                effect,
            } => {
                let mut sources = result_transform
                    .map(|transform| {
                        invocation_result_sources_in_row(arena, transform, invocation_sources)
                    })
                    .transpose()?
                    .unwrap_or_default();
                if let Some(source) =
                    effect_invocation_result_source(arena, effect, invocation_sources, &diagnostic)?
                {
                    sources.insert(source);
                }
                let source = single_invocation_result_source(sources, &diagnostic)?;
                if let Some(source) = source {
                    if let Some(result_transform) = result_transform {
                        validate_invocation_result_continuation(
                            arena,
                            *result_transform,
                            source,
                            &diagnostic,
                        )?;
                    }
                    superseded_triggers.push(trigger.clone());
                    *trigger = ValueRef::Source(source);
                    vec![trigger.clone()]
                } else {
                    Vec::new()
                }
            }
            PlanOpKind::ListMutation { mutation } => {
                let (trigger, expressions): (&mut ValueRef, Vec<PlanRowExpressionId>) =
                    match mutation {
                        PlanListMutation::Append(append) => {
                            (&mut append.trigger, vec![append.gate, append.item])
                        }
                        PlanListMutation::Remove(remove) => {
                            (&mut remove.trigger, vec![remove.gate, remove.predicate])
                        }
                    };
                let mut sources = BTreeSet::new();
                for expression in &expressions {
                    sources.extend(invocation_result_sources_in_row(
                        arena,
                        *expression,
                        invocation_sources,
                    )?);
                }
                let source = single_invocation_result_source(sources, &diagnostic)?;
                if let Some(source) = source {
                    for expression in expressions {
                        validate_invocation_result_continuation(
                            arena,
                            expression,
                            source,
                            &diagnostic,
                        )?;
                    }
                    superseded_triggers.push(trigger.clone());
                    *trigger = ValueRef::Source(source);
                    vec![trigger.clone()]
                } else {
                    Vec::new()
                }
            }
            PlanOpKind::SourceRoute
            | PlanOpKind::DerivedValue {
                expression: None, ..
            }
            | PlanOpKind::ListProjection { .. }
            | PlanOpKind::DependencyEdge => Vec::new(),
        };
        if !result_triggers.is_empty() {
            op.inputs
                .retain(|input| !superseded_triggers.contains(input));
            op.inputs.append(&mut result_triggers);
            op.inputs = unique_value_refs(std::mem::take(&mut op.inputs));
        }
    }
    Ok(())
}

fn op(
    next_op: &mut usize,
    kind: PlanOpKind,
    inputs: Vec<ValueRef>,
    output: Option<ValueRef>,
    indexed: bool,
    unresolved_executable_ref_count: usize,
) -> PlanOp {
    let id = PlanOpId(*next_op);
    *next_op += 1;
    PlanOp {
        id,
        kind,
        inputs,
        output,
        indexed,
        unresolved_executable_ref_count,
    }
}

fn region(id: usize, kind: RegionKind, ops: Vec<PlanOp>) -> OperationRegion {
    OperationRegion {
        id: PlanRegionId(id),
        kind,
        ops,
    }
}

fn unresolved_region_op_count(
    regions: &[OperationRegion],
    kind: RegionKind,
) -> Result<usize, PlanError> {
    let region = regions
        .iter()
        .find(|region| region.kind == kind)
        .ok_or_else(|| PlanError::new(format!("machine plan has no {kind:?} region")))?;
    Ok(region
        .ops
        .iter()
        .filter(|op| op.unresolved_executable_ref_count > 0)
        .count())
}

fn resolve_path(
    index: &ValueIndex,
    path: &str,
    refs: &mut Vec<ValueRef>,
    unresolved: &mut BTreeSet<String>,
) -> usize {
    if let Some(value_ref) = index.resolve(path) {
        refs.push(value_ref);
        0
    } else {
        unresolved.insert(path.to_owned());
        1
    }
}

fn effect_intent_default_constant(value: &EffectIntentDefaultValuePlan) -> PlanConstantValue {
    match value {
        EffectIntentDefaultValuePlan::Tag { name } => PlanConstantValue::Tag { name: name.clone() },
        EffectIntentDefaultValuePlan::Number { value } => PlanConstantValue::Number {
            value: value.clone(),
        },
        EffectIntentDefaultValuePlan::Text { value } => PlanConstantValue::Text {
            value: value.clone(),
        },
    }
}

fn unique_value_refs(value_refs: Vec<ValueRef>) -> Vec<ValueRef> {
    value_refs
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn delta_routes(
    program: &ErasedProgram,
    scalar_fields: &ScalarFieldCatalog,
) -> Result<Vec<DeltaRoute>, PlanError> {
    let mut outputs = BTreeSet::new();
    for state in &program.state_cells {
        outputs.insert(ValueRef::State(plan_state_id(state.id)));
    }
    for list in &program.lists {
        outputs.insert(ValueRef::List(plan_list_id(list.id)));
    }
    for derived in &program.derived_values {
        outputs.insert(derived_output_ref(derived, scalar_fields)?);
    }
    Ok(outputs
        .into_iter()
        .enumerate()
        .map(|(id, output)| DeltaRoute {
            id: PlanDeltaId(id),
            output,
        })
        .collect())
}

fn derived_output_ref(
    derived: &boon_ir::DerivedValue,
    scalar_fields: &ScalarFieldCatalog,
) -> Result<ValueRef, PlanError> {
    if let Some(list) = derived.materialized_list_id {
        return Ok(ValueRef::List(plan_list_id(list)));
    }
    Ok(ValueRef::Field(scalar_fields.require(
        derived.id,
        &format!("derived value `{}`", derived.path),
    )?))
}

#[derive(Clone, Debug)]
struct ScalarFieldCatalog {
    by_ir: BTreeMap<ir::FieldId, FieldId>,
    plan_ids: BTreeSet<FieldId>,
}

impl ScalarFieldCatalog {
    fn new(program: &ErasedProgram) -> Result<Self, PlanError> {
        let mut by_ir = BTreeMap::new();
        let mut plan_ids = BTreeSet::new();
        for field in &program.scope_index.fields {
            if field.resource_only {
                continue;
            }
            let plan_id = plan_field_id(field.id);
            if by_ir.insert(field.id, plan_id).is_some() || !plan_ids.insert(plan_id) {
                return Err(PlanError::new(format!(
                    "scalar field catalog contains duplicate FieldId {}",
                    field.id
                )));
            }
        }
        Ok(Self { by_ir, plan_ids })
    }

    fn require(&self, field: ir::FieldId, context: &str) -> Result<FieldId, PlanError> {
        self.by_ir.get(&field).copied().ok_or_else(|| {
            PlanError::new(format!(
                "{context} references missing or resource-only FieldId {field}"
            ))
        })
    }

    fn contains_plan_id(&self, field: FieldId) -> bool {
        self.plan_ids.contains(&field)
    }
}

pub(super) struct ValueIndex {
    by_path: BTreeMap<String, ValueRef>,
    by_storage: BTreeMap<ir::ErasedBindingId, ValueRef>,
    row_source_by_value_projection: BTreeMap<(ValueRef, Vec<String>), BTreeSet<ListId>>,
    read_by_expression: BTreeMap<ir::ExecutableExprId, ir::ErasedReadId>,
    distributed_by_expression: BTreeMap<ir::ExecutableExprId, ValueRef>,
    source_by_executable: BTreeMap<ir::ExecutableSourceId, ValueRef>,
    state_by_executable: BTreeMap<ir::ExecutableStateId, ValueRef>,
    state_value_types: BTreeMap<String, PlanValueType>,
    state_data_types: BTreeMap<StateId, DataTypePlan>,
    field_value_types: BTreeMap<FieldId, PlanValueType>,
    field_data_types: BTreeMap<FieldId, DataTypePlan>,
}

pub(super) fn lower_document_runtime_expression(
    program: &ErasedProgram,
    index: &ValueIndex,
    arena: &mut PlanRowExpressionArena,
    constants: &mut Vec<PlanConstant>,
    root: ir::ExecutableExprId,
) -> Result<PlanRowExpressionId, PlanError> {
    let mut inputs = Vec::new();
    ExecutableRowLowerer::new(program, index, arena, constants, &mut inputs)
        .lower(root)
        .map_err(|error| {
            PlanError::new(format!(
                "document runtime expression {} failed executable lowering: {error}",
                root.0
            ))
        })
}

impl ValueIndex {
    fn new(
        program: &ErasedProgram,
        distributed_by_expression: &BTreeMap<ir::ExecutableExprId, ValueRef>,
        distributed_by_path: &BTreeMap<String, ValueRef>,
        scalar_fields: &ScalarFieldCatalog,
    ) -> Result<Self, PlanError> {
        let mut by_path = BTreeMap::new();
        let mut by_storage = BTreeMap::new();
        let mut state_value_types = BTreeMap::new();
        let mut state_data_types = BTreeMap::new();
        let mut field_value_types = BTreeMap::new();
        let mut field_data_types = BTreeMap::new();
        let authority_field_ids = list_authority_field_ids(program);
        for source in &program.sources {
            by_path.insert(
                source.path.clone(),
                ValueRef::Source(plan_source_id(source.id)),
            );
            by_path.insert(
                source.binding_path.clone(),
                ValueRef::Source(plan_source_id(source.id)),
            );
        }
        let source_by_executable = program
            .sources
            .iter()
            .filter_map(|source| {
                source
                    .executable_source_id
                    .map(|executable| (executable, ValueRef::Source(plan_source_id(source.id))))
            })
            .collect();
        for state in &program.state_cells {
            by_path.insert(state.path.clone(), ValueRef::State(plan_state_id(state.id)));
            state_value_types.insert(
                state.path.clone(),
                migration_storage_default(program, state).map_or_else(
                    || state_executable_value_type(program, state),
                    |default| default.value_type,
                ),
            );
        }
        let state_by_executable = program
            .state_cells
            .iter()
            .filter_map(|state| {
                state
                    .executable_state_id
                    .map(|executable| (executable, ValueRef::State(plan_state_id(state.id))))
            })
            .collect();
        for arm in &program.state_update_arms {
            let Ok(Some(effect)) = exact_host_effect_expression(program, arm.output_expression_id)
            else {
                continue;
            };
            let Some(expression) = program
                .executable
                .expressions
                .get(effect.as_usize())
                .filter(|expression| expression.id == effect)
            else {
                continue;
            };
            let ir::ExecutableExpressionKind::Call { name, .. } = &expression.kind else {
                continue;
            };
            let Ok(Some(contract)) = builtin_effect_contract(name) else {
                continue;
            };
            let Some(schema) = contract.schema else {
                continue;
            };
            state_data_types.insert(plan_state_id(arm.state), schema.result_type);
        }
        for list in &program.lists {
            by_path.insert(list.name.clone(), ValueRef::List(plan_list_id(list.id)));
            if let Some((_, local_name)) = list.name.rsplit_once('.') {
                by_path
                    .entry(local_name.to_owned())
                    .or_insert(ValueRef::List(plan_list_id(list.id)));
            }
            if let ListInitializer::RecordLiteral { rows } = &list.initializer {
                for row in rows {
                    for field in &row.fields {
                        if let Some(field_id) = storage_input_field_id(
                            program,
                            &list.name,
                            &field.name,
                            &authority_field_ids,
                        ) {
                            by_path
                                .entry(format!("{}.{}", list.name, field.name))
                                .or_insert(ValueRef::Field(field_id));
                            if let Some((_, local_name)) = list.name.rsplit_once('.') {
                                by_path
                                    .entry(format!("{local_name}.{}", field.name))
                                    .or_insert(ValueRef::Field(field_id));
                            }
                        }
                    }
                }
            }
        }
        for derived in &program.derived_values {
            let output_ref = derived_output_ref(derived, scalar_fields)?;
            if let ValueRef::Field(field_id) = &output_ref
                && let Some(value_type) = derived_value_output_type(program, derived)
            {
                insert_field_value_type_if_absent(&mut field_value_types, *field_id, value_type);
            }
            by_path.insert(derived.path.clone(), output_ref);
        }
        for binding in &program.scope_index.bindings {
            let value = match binding.target {
                ir::ErasedBindingTarget::Value { row: Some(row), .. } => {
                    Some(ValueRef::List(plan_list_id(row.list)))
                }
                ir::ErasedBindingTarget::Value {
                    field: Some(field), ..
                } => Some(ValueRef::Field(scalar_fields.require(
                    field,
                    &format!("erased storage binding {}", binding.id),
                )?)),
                ir::ErasedBindingTarget::Value { .. } => None,
                ir::ErasedBindingTarget::Source { runtime, .. } => {
                    Some(ValueRef::Source(plan_source_id(runtime)))
                }
                ir::ErasedBindingTarget::State { runtime, .. } => {
                    Some(ValueRef::State(plan_state_id(runtime)))
                }
            };
            if let Some(value) = value {
                by_storage.insert(binding.id, value);
            }
        }
        let mut row_source_by_value_projection =
            BTreeMap::<(ValueRef, Vec<String>), BTreeSet<ListId>>::new();
        for binding in &program.scope_index.bindings {
            let Some(value) = by_storage.get(&binding.id).cloned() else {
                continue;
            };
            for row_value in program
                .scope_index
                .row_values
                .iter()
                .filter(|row_value| row_value.expression == binding.producer)
            {
                let key = (value.clone(), row_value.projection.clone());
                row_source_by_value_projection
                    .entry(key)
                    .or_default()
                    .insert(plan_list_id(row_value.row.list));
            }
        }
        for field in &program.scope_index.fields {
            if !scalar_fields.contains_plan_id(plan_field_id(field.id)) {
                continue;
            }
            if field.role != ir::ErasedFieldRole::Capture {
                let replace_existing_field = by_path
                    .get(&field.diagnostic_path)
                    .and_then(|value| match value {
                        ValueRef::Field(existing) => program
                            .scope_index
                            .fields
                            .get(existing.0)
                            .filter(|candidate| plan_field_id(candidate.id) == *existing),
                        _ => None,
                    })
                    .is_some_and(|existing| {
                        let role_priority = |role| match role {
                            ir::ErasedFieldRole::Value => 0,
                            ir::ErasedFieldRole::ValueAuthority => 1,
                            ir::ErasedFieldRole::ListAuthority => 2,
                            ir::ErasedFieldRole::Capture => 3,
                        };
                        (
                            role_priority(field.role),
                            erased_row_field_depth(program, field),
                            field.id,
                        ) < (
                            role_priority(existing.role),
                            erased_row_field_depth(program, existing),
                            existing.id,
                        )
                    });
                if replace_existing_field || !by_path.contains_key(&field.diagnostic_path) {
                    by_path.insert(
                        field.diagnostic_path.clone(),
                        ValueRef::Field(plan_field_id(field.id)),
                    );
                }
            }
            if let Some(value_type) = plan_value_type_from_typecheck_type(&field.flow_type.ty)
                .filter(|value_type| plan_value_type_is_concrete(*value_type))
            {
                // Checked field types own storage and index encoding. Derived-expression
                // inference is only a fallback for fields whose checked type stayed open.
                field_value_types.insert(plan_field_id(field.id), value_type);
            }
            if let Some(data_type) = data_type_plan_from_typecheck_type(&field.flow_type.ty) {
                field_data_types.insert(plan_field_id(field.id), data_type);
            }
        }
        for (path, value_ref) in distributed_by_path {
            by_path.insert(path.clone(), value_ref.clone());
        }
        Ok(Self {
            by_path,
            by_storage,
            row_source_by_value_projection,
            read_by_expression: program
                .scope_index
                .reads
                .iter()
                .map(|read| (read.expression, read.id))
                .collect(),
            distributed_by_expression: distributed_by_expression.clone(),
            source_by_executable,
            state_by_executable,
            state_value_types,
            state_data_types,
            field_value_types,
            field_data_types,
        })
    }

    pub(super) fn resolve(&self, path: &str) -> Option<ValueRef> {
        self.by_path
            .get(path)
            .cloned()
            .or_else(|| self.resolve_state_projection(path))
    }

    fn resolve_storage(&self, binding: ir::ErasedBindingId) -> Option<ValueRef> {
        self.by_storage.get(&binding).cloned()
    }

    fn row_source(
        &self,
        value: &ValueRef,
        projection: &[String],
    ) -> Result<Option<ListId>, PlanError> {
        let lists = self
            .row_source_by_value_projection
            .get(&(value.clone(), projection.to_vec()))
            .cloned()
            .unwrap_or_default();
        match lists.into_iter().collect::<Vec<_>>().as_slice() {
            [] => Ok(None),
            [list] => Ok(Some(*list)),
            lists => Err(PlanError::new(format!(
                "stored row value {value:?} projection `{}` has multiple exact row owners {lists:?}",
                projection.join(".")
            ))),
        }
    }

    fn resolve_read(&self, expression: ir::ExecutableExprId) -> Option<ir::ErasedReadId> {
        self.read_by_expression.get(&expression).copied()
    }

    fn resolve_distributed_expression(&self, expression: ir::ExecutableExprId) -> Option<ValueRef> {
        self.distributed_by_expression.get(&expression).cloned()
    }

    fn resolve_executable_source(&self, source: ir::ExecutableSourceId) -> Option<ValueRef> {
        self.source_by_executable.get(&source).cloned()
    }

    fn resolve_executable_state(&self, state: ir::ExecutableStateId) -> Option<ValueRef> {
        self.state_by_executable.get(&state).cloned()
    }

    fn resolve_state_projection(&self, path: &str) -> Option<ValueRef> {
        self.by_path
            .iter()
            .filter_map(|(state_path, value_ref)| {
                let ValueRef::State(state_id) = value_ref else {
                    return None;
                };
                state_path_suffixes(state_path).find_map(|candidate| {
                    let suffix = path.strip_prefix(candidate)?.strip_prefix('.')?;
                    let field_path = suffix
                        .split('.')
                        .filter(|field| !field.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    (!field_path.is_empty()).then_some({
                        (
                            candidate.len(),
                            ValueRef::StateProjection {
                                state_id: *state_id,
                                field_path,
                            },
                        )
                    })
                })
            })
            .max_by_key(|(matched_len, _)| *matched_len)
            .map(|(_, value_ref)| value_ref)
    }

    fn state_value_type(&self, path: &str) -> Option<&PlanValueType> {
        self.state_value_types.get(path)
    }

    fn field_value_type(&self, field_id: FieldId) -> Option<&PlanValueType> {
        self.field_value_types.get(&field_id)
    }

    fn field_data_type(&self, field_id: FieldId) -> Option<&DataTypePlan> {
        self.field_data_types.get(&field_id)
    }

    fn state_projection_data_type(
        &self,
        state_id: StateId,
        field_path: &[String],
    ) -> Option<DataTypePlan> {
        project_data_type(self.state_data_types.get(&state_id)?, field_path)
    }
}

fn state_path_suffixes(path: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(path), |candidate| {
        candidate.split_once('.').map(|(_, suffix)| suffix)
    })
}

fn project_data_type(data_type: &DataTypePlan, field_path: &[String]) -> Option<DataTypePlan> {
    let Some((field, rest)) = field_path.split_first() else {
        return Some(data_type.clone());
    };
    let projected = match data_type {
        DataTypePlan::Record { fields, .. } => fields
            .iter()
            .find(|candidate| candidate.name == *field)
            .map(|candidate| candidate.data_type.clone()),
        DataTypePlan::Variant { variants } => {
            let mut projected = variants.iter().filter_map(|variant| {
                variant
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == *field)
                    .map(|candidate| candidate.data_type.clone())
            });
            let first = projected.next()?;
            projected
                .all(|candidate| candidate == first)
                .then_some(first)
        }
        _ => None,
    }?;
    project_data_type(&projected, rest)
}

fn insert_field_value_type_if_absent(
    field_value_types: &mut BTreeMap<FieldId, PlanValueType>,
    field_id: FieldId,
    value_type: PlanValueType,
) {
    if !plan_value_type_is_concrete(value_type) {
        return;
    }
    field_value_types.entry(field_id).or_insert(value_type);
}

#[cfg(test)]
mod scalar_field_catalog_tests {
    use super::*;

    const RESOURCE_ROWS: &str = r#"
store: [
    inputs: LIST { [label: TEXT { one }] }
    rows:
        inputs
        |> List/map(item, new: new_row(input: item))
    forwarded:
        rows
        |> List/map(item, new: [
            controls: item.controls
            label: item.label
            selected: item.selected
        ])
]

FUNCTION new_row(input) {
    [
        controls: [select: SOURCE]
        label: input.label
        selected:
            True |> HOLD selected {
                controls.select |> THEN { False }
            }
    ]
}

FUNCTION render_row(row) {
    Element/button(
        element: [events: [press: row.controls.select]]
        style: []
        label: row.label
    )
}

document: Document/new(
    root: Element/stripe(
        element: []
        direction: Column
        style: []
        items: store.forwarded |> List/map(item, new: render_row(row: item))
    )
)
"#;

    fn compiled_resource_rows() -> crate::CompiledMachinePlanFromSource {
        crate::compile_source_text_to_machine_plan(
            "scalar-field-catalog.bn",
            RESOURCE_ROWS,
            TargetProfile::SoftwareDefault,
        )
        .expect("generic resource rows compile")
    }

    fn resource_field(compiled: &crate::CompiledMachinePlanFromSource) -> FieldId {
        compiled
            .ir
            .scope_index
            .fields
            .iter()
            .find(|field| field.resource_only && field.static_owner.is_some())
            .map(|field| plan_field_id(field.id))
            .expect("producer-owned resource-only structural field")
    }

    fn assert_resource_ref_rejected(
        compiled: &crate::CompiledMachinePlanFromSource,
        plan: &MachinePlan,
        location: &str,
    ) {
        let error = validate_resource_only_fields_excluded(&compiled.ir, plan)
            .expect_err("resource-only scalar reference must fail");
        assert!(
            error
                .to_string()
                .contains("entered a final MachinePlan ValueRef"),
            "{location}: {error}"
        );
    }

    fn assert_resource_field_surface_rejected(
        compiled: &crate::CompiledMachinePlanFromSource,
        plan: &MachinePlan,
        expected: &str,
    ) {
        let error = validate_resource_only_fields_excluded(&compiled.ir, plan)
            .expect_err("resource-only typed field surface must fail");
        assert!(error.to_string().contains(expected), "{error}");
    }

    #[test]
    fn value_index_excludes_resource_only_paths() {
        let compiled = compiled_resource_rows();
        let resource = compiled
            .ir
            .scope_index
            .fields
            .iter()
            .find(|field| field.resource_only && field.static_owner.is_some())
            .expect("producer-owned resource-only structural field");
        let scalar = compiled
            .ir
            .scope_index
            .fields
            .iter()
            .find(|field| {
                !field.resource_only
                    && field.row == resource.row
                    && field.role.is_value()
                    && field.name == "label"
            })
            .expect("ordinary scalar sibling");
        let catalog = ScalarFieldCatalog::new(&compiled.ir).unwrap();
        let index =
            ValueIndex::new(&compiled.ir, &BTreeMap::new(), &BTreeMap::new(), &catalog).unwrap();

        assert_ne!(
            index.resolve(&resource.diagnostic_path),
            Some(ValueRef::Field(plan_field_id(resource.id))),
            "structural source metadata cannot resolve as a scalar field"
        );
        assert_eq!(
            index.resolve(&scalar.diagnostic_path),
            Some(ValueRef::Field(plan_field_id(scalar.id))),
            "ordinary row values remain scalar-indexed"
        );
    }

    #[test]
    fn producer_ownership_and_source_routes_separate_resources_from_scalars() {
        let compiled = compiled_resource_rows();
        let resource = compiled
            .ir
            .scope_index
            .fields
            .iter()
            .find(|field| field.resource_only && field.static_owner.is_some())
            .expect("producer-owned resource-only structural field");
        let owner = resource.static_owner.expect("row function owner");
        let ownership = producer_function_ownership_seed(&compiled.ir, owner).unwrap();
        assert!(
            !ownership.fields.contains(&plan_field_id(resource.id)),
            "resource-only metadata entered producer scalar ownership"
        );
        assert!(
            compiled
                .plan
                .source_routes
                .iter()
                .any(
                    |route| route.row_projections.iter().any(|projection| projection
                        .path
                        .iter()
                        .map(String::as_str)
                        .eq(["controls", "select"]))
                ),
            "scalar filtering removed the exact nested source row projection"
        );
    }

    #[test]
    fn final_plan_validator_rejects_resource_refs_in_independent_plan_surfaces() {
        let compiled = compiled_resource_rows();
        let resource = ValueRef::Field(resource_field(&compiled));

        let mut output_plan = compiled.plan.clone();
        output_plan.outputs[0].value = OutputValueRef::RuntimeValue {
            value: resource.clone(),
            list_fields: Vec::new(),
        };
        assert_resource_ref_rejected(&compiled, &output_plan, "output");

        let mut op_plan = compiled.plan.clone();
        op_plan.regions[0].ops[0].inputs.push(resource.clone());
        assert_resource_ref_rejected(&compiled, &op_plan, "operation input");

        let mut derived_plan = compiled.plan.clone();
        let derived = derived_plan
            .regions
            .iter_mut()
            .flat_map(|region| &mut region.ops)
            .find_map(|op| match &mut op.kind {
                PlanOpKind::DerivedValue { expression, .. } => Some(expression),
                _ => None,
            })
            .expect("derived operation");
        *derived = Some(PlanDerivedExpression::BoolNot {
            input: resource.clone(),
        });
        assert_resource_ref_rejected(&compiled, &derived_plan, "derived operand");

        let mut delta_plan = compiled.plan.clone();
        delta_plan.delta_plan.deltas[0].output = resource.clone();
        assert_resource_ref_rejected(&compiled, &delta_plan, "delta");

        let mut arena_plan = compiled.plan.clone();
        let mut nodes = arena_plan.row_expressions.clone().into_nodes();
        nodes.push(PlanRowExpressionNode::Field { input: resource });
        arena_plan.row_expressions = PlanRowExpressionArena::from_nodes(nodes).unwrap();
        assert_resource_ref_rejected(&compiled, &arena_plan, "unreachable arena node");

        let resource_field = resource_field(&compiled);
        let mut materialization_plan = compiled.plan.clone();
        let materialization = materialization_plan
            .regions
            .iter_mut()
            .flat_map(|region| &mut region.ops)
            .find_map(|op| match &mut op.kind {
                PlanOpKind::DerivedValue {
                    materialization: Some(materialization),
                    ..
                } => Some(materialization),
                _ => None,
            })
            .expect("resource-row fixture materialization");
        materialization
            .fields
            .insert("forged_resource".to_owned(), resource_field);
        let error = validate_resource_only_fields_excluded(&compiled.ir, &materialization_plan)
            .expect_err("resource-only materialization field must fail");
        assert!(
            error.to_string().contains("scalar materialization field"),
            "{error}"
        );

        let mut authority_plan = compiled.plan.clone();
        let materialization = authority_plan
            .regions
            .iter_mut()
            .flat_map(|region| &mut region.ops)
            .find_map(|op| match &mut op.kind {
                PlanOpKind::DerivedValue {
                    materialization: Some(materialization),
                    ..
                } => Some(materialization),
                _ => None,
            })
            .expect("resource-row fixture materialization");
        materialization
            .value_list_authorities
            .push(PlanValueListAuthority {
                owner: PlanStaticOwnerId::ROOT,
                list_id: materialization.target_list,
                fields: BTreeMap::from([("forged_resource".to_owned(), resource_field)]),
            });
        let error = validate_resource_only_fields_excluded(&compiled.ir, &authority_plan)
            .expect_err("resource-only value-list authority field must fail");
        assert!(
            error.to_string().contains("value-list authority field"),
            "{error}"
        );

        let mut append_plan = compiled.plan.clone();
        let row_expression = append_plan
            .row_expressions
            .iter()
            .next()
            .map(|(id, _)| id)
            .expect("resource-row fixture row expression");
        let list_id = append_plan
            .storage_layout
            .list_slots
            .first()
            .map(|slot| slot.list_id)
            .expect("resource-row fixture list");
        append_plan.regions[0].ops[0].kind = PlanOpKind::ListMutation {
            mutation: PlanListMutation::Append(PlanListAppend {
                site: 0,
                ordinal: 0,
                owner: PlanOwner::root(),
                trigger: ValueRef::List(list_id),
                gate: row_expression,
                item: row_expression,
                fields: vec![PlanListAppendField {
                    name: "forged_resource".to_owned(),
                    field_id: resource_field,
                }],
                row_field_copies: Vec::new(),
            }),
        };
        let error = validate_resource_only_fields_excluded(&compiled.ir, &append_plan)
            .expect_err("resource-only append field must fail");
        assert!(error.to_string().contains("list append field"), "{error}");

        let mut demand_plan = compiled.plan.clone();
        demand_plan.demand.root_derived_outputs = RootOutputDemand::Selected(vec![resource_field]);
        assert_resource_field_surface_rejected(
            &compiled,
            &demand_plan,
            "selected root-output scalar demand",
        );

        let mut persistence_plan = compiled.plan.clone();
        persistence_plan
            .persistence
            .memory
            .first_mut()
            .expect("resource-row fixture scalar memory")
            .leaves
            .first_mut()
            .expect("resource-row fixture scalar memory leaf")
            .runtime_field_id = Some(resource_field);
        assert_resource_field_surface_rejected(
            &compiled,
            &persistence_plan,
            "persistent scalar memory metadata",
        );

        let mut initial_row_plan = compiled.plan.clone();
        initial_row_plan
            .storage_layout
            .list_slots
            .iter_mut()
            .flat_map(|slot| &mut slot.initial_rows)
            .flat_map(|row| &mut row.fields)
            .next()
            .expect("resource-row fixture initial list field")
            .field_id = Some(resource_field);
        assert_resource_field_surface_rejected(
            &compiled,
            &initial_row_plan,
            "scalar initial-list row field",
        );

        let mut list_get_plan = compiled.plan.clone();
        let mut nodes = list_get_plan.row_expressions.clone().into_nodes();
        let index = PlanRowExpressionId(0);
        let list_id = list_get_plan
            .storage_layout
            .list_slots
            .first()
            .expect("resource-row fixture list")
            .list_id;
        nodes.push(PlanRowExpressionNode::ListGetField {
            list_id,
            index,
            field: resource_field,
        });
        list_get_plan.row_expressions = PlanRowExpressionArena::from_nodes(nodes).unwrap();
        assert_resource_field_surface_rejected(
            &compiled,
            &list_get_plan,
            "scalar ListGetField read",
        );

        let mut document_plan = compiled.plan.clone();
        document_plan
            .document
            .as_mut()
            .expect("resource-row fixture document")
            .expressions
            .first_mut()
            .expect("resource-row fixture document expression")
            .op = DocumentExprOp::Read {
            read: DocumentRead::Field {
                field: resource_field,
            },
        };
        assert_resource_field_surface_rejected(
            &compiled,
            &document_plan,
            "retained document scalar field read",
        );
    }
}

#[cfg(test)]
mod session_info_intrinsic_tests {
    use super::*;

    const SOURCE: &str = r#"
outputs: [
    status: SessionInfo/status()
    principal: SessionInfo/principal()
]
"#;

    #[test]
    fn compiler_lowers_session_info_as_typed_plan_intrinsics() {
        let compiled = crate::compile_source_text_to_machine_plan_for_role(
            "session-info.bn",
            SOURCE,
            TargetProfile::SoftwareDefault,
            ProgramRole::Session,
        )
        .unwrap();

        assert!(compiled.plan.constants.is_empty());
        assert!(compiled.plan.effects.is_empty());
        let intrinsic_ops = compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .filter_map(|op| match &op.kind {
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::RowExpression { expression }),
                    ..
                } => match compiled.plan.row_expressions.get(*expression) {
                    Some(PlanRowExpressionNode::Intrinsic { intrinsic }) => {
                        Some((*intrinsic, op.inputs.as_slice()))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            intrinsic_ops,
            vec![
                (PlanIntrinsic::SessionInfoStatus, &[][..]),
                (PlanIntrinsic::SessionInfoPrincipal, &[][..]),
            ]
        );

        let status_type = compiled
            .plan
            .output_root("status")
            .map(|output| &output.contract)
            .unwrap();
        let OutputContractKind::HostValue {
            data_type: DataTypePlan::Variant { variants },
        } = status_type
        else {
            panic!("status output must be a variant host value");
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.tag.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["Connecting", "Current", "Stale", "Failed"])
        );
        let failed = variants
            .iter()
            .find(|variant| variant.tag == "Failed")
            .unwrap();
        assert!(matches!(
            failed.fields.as_slice(),
            [DataTypeFieldPlan {
                name,
                data_type: DataTypePlan::Text,
            }] if name == "code"
        ));
        let principal_type = compiled
            .plan
            .output_root("principal")
            .map(|output| &output.contract)
            .unwrap();
        let OutputContractKind::HostValue {
            data_type: DataTypePlan::Variant { variants },
        } = principal_type
        else {
            panic!("principal output must be a variant host value");
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.tag.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["Anonymous", "Authenticated"])
        );
        assert!(variants.iter().all(|variant| !variant.open));
        let authenticated = variants
            .iter()
            .find(|variant| variant.tag == "Authenticated")
            .unwrap();
        assert!(
            authenticated
                .fields
                .iter()
                .any(|field| { field.name == "subject" && field.data_type == DataTypePlan::Text })
        );
        assert!(authenticated.fields.iter().any(|field| {
            field.name == "roles"
                && field.data_type
                    == (DataTypePlan::List {
                        item: Box::new(DataTypePlan::Text),
                    })
        }));

        let verification = verify_plan(&compiled.plan).unwrap();
        assert_eq!(verification.status, "pass", "{:#?}", verification.checks);
    }
}
