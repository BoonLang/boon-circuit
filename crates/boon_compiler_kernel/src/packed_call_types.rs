use crate::{
    KernelTypeParameterId, PackedCallTypeSubstitution, PackedCallableTypeParameter, PackedFlow,
    TypeTermArena, TypeTermHead, TypeTermId, TypeVariableId, VariantTerm,
};
use boon_contract::SymbolId;

/// Reusable, allocation-free-after-growth storage for packed callable
/// matching and instantiation.
///
/// Every map is generation stamped.  A call therefore resets only rows it
/// actually reaches; it neither allocates a tree map nor clears a
/// component-sized array merely to derive a small generic environment.
#[derive(Debug, Default)]
pub(crate) struct PackedCallTypeScratch {
    parameter_generation: u32,
    parameter_generations: Vec<u32>,
    parameter_ordinals: Vec<u32>,
    parameter_sources: Vec<TypeVariableId>,
    traversal: Vec<TypeTermId>,
    children: Vec<TypeTermId>,
    matched: Vec<Option<TypeTermId>>,
    result_matched: Vec<Option<TypeTermId>>,
    substitution_by_parameter: Vec<Option<TypeTermId>>,
    term_generation: u32,
    term_generations: Vec<u32>,
    remapped_terms: Vec<TypeTermId>,
    postorder: Vec<(TypeTermId, bool)>,
    remapped_children: Vec<TypeTermId>,
}

/// Small sealing-only traversal state for checking retained callable
/// parameter order.
///
/// Unlike the occurrence matcher this deliberately does not index arrays by
/// `TypeVariableId`: generic arities are tiny, while project-global variable
/// IDs can be large. Linear duplicate checks therefore avoid zeroing a pair
/// of project-sized maps during release sealing.
#[derive(Debug, Default)]
pub(crate) struct PackedCallableParameterTraversalScratch {
    traversal: Vec<TypeTermId>,
    children: Vec<TypeTermId>,
}

impl PackedCallTypeScratch {
    fn next_parameter_generation(&mut self) {
        self.parameter_generation = self.parameter_generation.wrapping_add(1);
        if self.parameter_generation == 0 {
            self.parameter_generations.fill(0);
            self.parameter_generation = 1;
        }
        self.parameter_sources.clear();
        self.traversal.clear();
        self.children.clear();
    }

    fn ensure_parameter_slot(&mut self, variable: TypeVariableId) {
        let required = variable.0 as usize + 1;
        if self.parameter_generations.len() < required {
            self.parameter_generations.resize(required, 0);
            self.parameter_ordinals.resize(required, 0);
        }
    }

    fn insert_parameter(&mut self, variable: TypeVariableId) {
        self.ensure_parameter_slot(variable);
        let index = variable.0 as usize;
        if self.parameter_generations[index] == self.parameter_generation {
            return;
        }
        self.parameter_generations[index] = self.parameter_generation;
        self.parameter_ordinals[index] = u32::try_from(self.parameter_sources.len())
            .expect("packed callable parameter count exceeds u32");
        self.parameter_sources.push(variable);
    }

    fn parameter_ordinal(&self, variable: TypeVariableId) -> Option<usize> {
        let index = variable.0 as usize;
        (self.parameter_generations.get(index).copied() == Some(self.parameter_generation))
            .then(|| self.parameter_ordinals[index] as usize)
    }

    fn collect_parameters(&mut self, arena: &TypeTermArena, root: TypeTermId) {
        self.traversal.push(root);
        while let Some(term) = self.traversal.pop() {
            if let TypeTermHead::Variable(variable) = arena.term_head(term) {
                self.insert_parameter(variable);
                continue;
            }
            self.children.clear();
            arena.append_term_children(term, &mut self.children);
            self.traversal.extend(self.children.iter().rev().copied());
        }
    }

    fn prepare_parameter_roots(
        &mut self,
        arena: &TypeTermArena,
        roots: impl IntoIterator<Item = TypeTermId>,
    ) {
        self.next_parameter_generation();
        for root in roots {
            self.collect_parameters(arena, root);
        }
        self.matched.resize(self.parameter_sources.len(), None);
        self.matched.fill(None);
        self.result_matched
            .resize(self.parameter_sources.len(), None);
        self.result_matched.fill(None);
    }

    fn prepare_parameters(
        &mut self,
        arena: &TypeTermArena,
        formals: &[PackedFlow],
        result: PackedFlow,
    ) {
        self.prepare_parameter_roots(
            arena,
            formals
                .iter()
                .map(|formal| formal.term)
                .chain(std::iter::once(result.term)),
        );
    }

    fn next_term_generation(&mut self, term_count: usize) {
        self.term_generation = self.term_generation.wrapping_add(1);
        if self.term_generation == 0 {
            self.term_generations.fill(0);
            self.term_generation = 1;
        }
        self.term_generations.resize(term_count, 0);
        self.remapped_terms.resize(term_count, TypeTermId(0));
        self.postorder.clear();
        self.children.clear();
        self.remapped_children.clear();
    }

    fn remapped(&self, term: TypeTermId) -> Option<TypeTermId> {
        let index = term.0 as usize;
        (self.term_generations.get(index).copied() == Some(self.term_generation))
            .then(|| self.remapped_terms[index])
    }

    fn set_remapped(&mut self, term: TypeTermId, remapped: TypeTermId) {
        let index = term.0 as usize;
        self.term_generations[index] = self.term_generation;
        self.remapped_terms[index] = remapped;
    }
}

/// Retain the exact generic-parameter order used by packed call matching.
///
/// Parameter IDs are traversal ordinals, not definition alpha ordinals.  A
/// later semantic consumer therefore needs this source-variable sequence to
/// interpret a [`KernelTypeParameterId`] without rediscovering the callable's
/// recursive type structure or depending on canonical object-field order.
pub(crate) fn collect_packed_callable_type_parameter_sources(
    arena: &TypeTermArena,
    formal_terms: impl IntoIterator<Item = TypeTermId>,
    result: TypeTermId,
    scratch: &mut PackedCallableParameterTraversalScratch,
    output: &mut Vec<TypeVariableId>,
) {
    output.clear();
    visit_packed_callable_type_parameter_sources(
        arena,
        formal_terms,
        result,
        scratch,
        |variable| {
            if !output.contains(&variable) {
                output.push(variable);
            }
            true
        },
    );
}

/// Check that a retained parameter column exactly matches packed call
/// matching's first-occurrence traversal, without allocating storage indexed
/// by the project-wide variable namespace.
pub(crate) fn packed_callable_type_parameter_sources_match(
    arena: &TypeTermArena,
    formal_terms: impl IntoIterator<Item = TypeTermId>,
    result: TypeTermId,
    expected: &[PackedCallableTypeParameter],
    scratch: &mut PackedCallableParameterTraversalScratch,
) -> bool {
    let mut matched = 0usize;
    visit_packed_callable_type_parameter_sources(arena, formal_terms, result, scratch, |variable| {
        if expected[..matched]
            .iter()
            .any(|parameter| parameter.source == variable)
        {
            return true;
        }
        if expected.get(matched).map(|parameter| parameter.source) != Some(variable) {
            return false;
        }
        matched += 1;
        true
    }) && matched == expected.len()
}

fn visit_packed_callable_type_parameter_sources(
    arena: &TypeTermArena,
    formal_terms: impl IntoIterator<Item = TypeTermId>,
    result: TypeTermId,
    scratch: &mut PackedCallableParameterTraversalScratch,
    mut visit: impl FnMut(TypeVariableId) -> bool,
) -> bool {
    scratch.traversal.clear();
    scratch.children.clear();
    for root in formal_terms.into_iter().chain(std::iter::once(result)) {
        scratch.traversal.push(root);
        while let Some(term) = scratch.traversal.pop() {
            if let TypeTermHead::Variable(variable) = arena.term_head(term) {
                if !visit(variable) {
                    return false;
                }
                continue;
            }
            scratch.children.clear();
            arena.append_term_children(term, &mut scratch.children);
            scratch
                .traversal
                .extend(scratch.children.iter().rev().copied());
        }
    }
    true
}

/// Derive one occurrence-local generic environment directly from packed term
/// IDs. `output` is caller-owned scratch or a flat destination column; the
/// function performs no recursive `Type` construction and no per-call map
/// allocation.
pub(crate) fn derive_packed_call_type_substitutions(
    arena: &TypeTermArena,
    target_formals: &[PackedFlow],
    target_result: PackedFlow,
    actuals: &[(u32, TypeTermId)],
    actual_result: Option<TypeTermId>,
    scratch: &mut PackedCallTypeScratch,
    output: &mut Vec<PackedCallTypeSubstitution>,
) {
    scratch.prepare_parameters(arena, target_formals, target_result);
    for &(ordinal, actual) in actuals {
        let Some(pattern) = target_formals.get(ordinal as usize) else {
            continue;
        };
        match_call_type_pattern(
            arena,
            pattern.term,
            actual,
            &scratch.parameter_generations,
            &scratch.parameter_ordinals,
            scratch.parameter_generation,
            &mut scratch.matched,
        );
    }
    if let Some(actual_result) = actual_result {
        match_call_type_pattern(
            arena,
            target_result.term,
            actual_result,
            &scratch.parameter_generations,
            &scratch.parameter_ordinals,
            scratch.parameter_generation,
            &mut scratch.result_matched,
        );
        for (matched, result) in scratch.matched.iter_mut().zip(&scratch.result_matched) {
            if result.is_some() {
                *matched = *result;
            }
        }
    }
    output.clear();
    output.extend(
        scratch
            .matched
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(ordinal, term)| {
                term.map(|term| PackedCallTypeSubstitution {
                    variable: KernelTypeParameterId(
                        u32::try_from(ordinal)
                            .expect("packed callable parameter count exceeds u32"),
                    ),
                    term,
                })
            }),
    );
}

/// Instantiate one callable pattern in the same packed arena.
///
/// The post-order walk is generation stamped and copy-on-change. Closed roots
/// return their original ID without interning; variable-bearing roots rebuild
/// only ancestors of a replaced parameter.
pub(crate) fn instantiate_packed_call_type(
    arena: &mut TypeTermArena,
    pattern: TypeTermId,
    substitutions: &[PackedCallTypeSubstitution],
    scratch: &mut PackedCallTypeScratch,
) -> TypeTermId {
    scratch
        .substitution_by_parameter
        .resize(scratch.parameter_sources.len(), None);
    scratch.substitution_by_parameter.fill(None);
    for substitution in substitutions {
        if let Some(slot) = scratch
            .substitution_by_parameter
            .get_mut(substitution.variable.0 as usize)
        {
            *slot = Some(substitution.term);
        }
    }
    scratch.next_term_generation(arena.len());
    scratch.postorder.push((pattern, false));
    while let Some((term, expanded)) = scratch.postorder.pop() {
        if scratch.remapped(term).is_some() {
            continue;
        }
        if let TypeTermHead::Variable(variable) = arena.term_head(term) {
            let remapped = scratch
                .parameter_ordinal(variable)
                .and_then(|ordinal| scratch.substitution_by_parameter[ordinal])
                .unwrap_or(term);
            scratch.set_remapped(term, remapped);
            continue;
        }
        scratch.children.clear();
        arena.append_term_children(term, &mut scratch.children);
        if scratch.children.is_empty() {
            scratch.set_remapped(term, term);
            continue;
        }
        if !expanded {
            scratch.postorder.push((term, true));
            for index in (0..scratch.children.len()).rev() {
                let child = scratch.children[index];
                if scratch.remapped(child).is_none() {
                    scratch.postorder.push((child, false));
                }
            }
            continue;
        }
        scratch.remapped_children.clear();
        for index in 0..scratch.children.len() {
            let child = scratch.children[index];
            let remapped = scratch
                .remapped(child)
                .expect("packed child is post-ordered");
            scratch.remapped_children.push(remapped);
        }
        let remapped = arena.rebuild_term_from_children(term, &scratch.remapped_children);
        scratch.set_remapped(term, remapped);
    }
    scratch
        .remapped(pattern)
        .expect("packed call instantiation visits its root")
}

fn parameter_ordinal(
    variable: TypeVariableId,
    generations: &[u32],
    ordinals: &[u32],
    generation: u32,
) -> Option<usize> {
    let index = variable.0 as usize;
    (generations.get(index).copied() == Some(generation)).then(|| ordinals[index] as usize)
}

fn match_call_type_pattern(
    arena: &TypeTermArena,
    pattern: TypeTermId,
    actual: TypeTermId,
    parameter_generations: &[u32],
    parameter_ordinals: &[u32],
    parameter_generation: u32,
    substitutions: &mut [Option<TypeTermId>],
) {
    match (arena.term_head(pattern), arena.term_head(actual)) {
        (
            TypeTermHead::Variable(_),
            TypeTermHead::Unknown | TypeTermHead::UnresolvedShape(_) | TypeTermHead::Absent,
        ) => {}
        (TypeTermHead::Variable(variable), _) => {
            let Some(ordinal) = parameter_ordinal(
                variable,
                parameter_generations,
                parameter_ordinals,
                parameter_generation,
            ) else {
                return;
            };
            if substitutions[ordinal]
                .is_none_or(|term| packed_call_type_is_placeholder(arena, term))
            {
                substitutions[ordinal] = Some(actual);
            }
        }
        (TypeTermHead::List(pattern), TypeTermHead::List(actual))
        | (TypeTermHead::Set(pattern), TypeTermHead::Set(actual)) => match_call_type_pattern(
            arena,
            pattern,
            actual,
            parameter_generations,
            parameter_ordinals,
            parameter_generation,
            substitutions,
        ),
        (
            TypeTermHead::Map {
                key: pattern_key,
                value: pattern_value,
            },
            TypeTermHead::Map {
                key: actual_key,
                value: actual_value,
            },
        ) => {
            match_call_type_pattern(
                arena,
                pattern_key,
                actual_key,
                parameter_generations,
                parameter_ordinals,
                parameter_generation,
                substitutions,
            );
            match_call_type_pattern(
                arena,
                pattern_value,
                actual_value,
                parameter_generations,
                parameter_ordinals,
                parameter_generation,
                substitutions,
            );
        }
        (
            TypeTermHead::Function {
                args: pattern_args,
                result: pattern_result,
                ..
            },
            TypeTermHead::Function {
                args: actual_args,
                result: actual_result,
                ..
            },
        ) if pattern_args.len() == actual_args.len() => {
            for (&pattern, &actual) in arena
                .term_ids(pattern_args)
                .iter()
                .zip(arena.term_ids(actual_args))
            {
                match_call_type_pattern(
                    arena,
                    pattern,
                    actual,
                    parameter_generations,
                    parameter_ordinals,
                    parameter_generation,
                    substitutions,
                );
            }
            match_call_type_pattern(
                arena,
                pattern_result,
                actual_result,
                parameter_generations,
                parameter_ordinals,
                parameter_generation,
                substitutions,
            );
        }
        (
            TypeTermHead::Object { shape: pattern, .. },
            TypeTermHead::Object { shape: actual, .. },
        ) => {
            for field in arena.object_fields_for_shape(pattern).canonical_iter() {
                if let Some(actual) = arena.lookup_object_field(actual, field.name) {
                    match_call_type_pattern(
                        arena,
                        field.ty,
                        actual,
                        parameter_generations,
                        parameter_ordinals,
                        parameter_generation,
                        substitutions,
                    );
                }
            }
        }
        (TypeTermHead::VariantSet(pattern), TypeTermHead::VariantSet(actual)) => {
            for pattern in arena.variant_terms(pattern) {
                let VariantTerm::Tagged {
                    tag: pattern_tag,
                    fields: pattern_fields,
                } = *pattern
                else {
                    continue;
                };
                let Some(VariantTerm::Tagged {
                    fields: actual_fields,
                    ..
                }) = arena
                    .variant_terms(actual)
                    .iter()
                    .find(|variant| variant.tag() == pattern_tag)
                else {
                    continue;
                };
                let (
                    TypeTermHead::Object {
                        shape: pattern_shape,
                        ..
                    },
                    TypeTermHead::Object {
                        shape: actual_shape,
                        ..
                    },
                ) = (
                    arena.term_head(pattern_fields),
                    arena.term_head(*actual_fields),
                )
                else {
                    continue;
                };
                for field in arena
                    .object_fields_for_shape(pattern_shape)
                    .canonical_iter()
                {
                    if let Some(actual) = arena.lookup_object_field(actual_shape, field.name) {
                        match_call_type_pattern(
                            arena,
                            field.ty,
                            actual,
                            parameter_generations,
                            parameter_ordinals,
                            parameter_generation,
                            substitutions,
                        );
                    }
                }
            }
        }
        (TypeTermHead::Union(pattern), TypeTermHead::Union(actual)) => {
            for &actual in arena.term_ids(actual) {
                if let Some(&pattern) = arena
                    .term_ids(pattern)
                    .iter()
                    .find(|pattern| packed_call_type_pattern_accepts(arena, **pattern, actual))
                {
                    match_call_type_pattern(
                        arena,
                        pattern,
                        actual,
                        parameter_generations,
                        parameter_ordinals,
                        parameter_generation,
                        substitutions,
                    );
                }
            }
        }
        (TypeTermHead::Union(pattern), _) => {
            if let Some(&pattern) = arena
                .term_ids(pattern)
                .iter()
                .find(|pattern| packed_call_type_pattern_accepts(arena, **pattern, actual))
            {
                match_call_type_pattern(
                    arena,
                    pattern,
                    actual,
                    parameter_generations,
                    parameter_ordinals,
                    parameter_generation,
                    substitutions,
                );
            }
        }
        (_, TypeTermHead::Union(actual)) => {
            for &actual in arena
                .term_ids(actual)
                .iter()
                .filter(|actual| packed_call_type_pattern_accepts(arena, pattern, **actual))
            {
                match_call_type_pattern(
                    arena,
                    pattern,
                    actual,
                    parameter_generations,
                    parameter_ordinals,
                    parameter_generation,
                    substitutions,
                );
            }
        }
        _ => {}
    }
}

fn packed_call_type_pattern_accepts(
    arena: &TypeTermArena,
    pattern: TypeTermId,
    actual: TypeTermId,
) -> bool {
    match (arena.term_head(pattern), arena.term_head(actual)) {
        (
            TypeTermHead::Variable(_),
            TypeTermHead::Unknown | TypeTermHead::UnresolvedShape(_) | TypeTermHead::Absent,
        ) => false,
        (TypeTermHead::Variable(_), _) => true,
        (TypeTermHead::List(pattern), TypeTermHead::List(actual))
        | (TypeTermHead::Set(pattern), TypeTermHead::Set(actual)) => {
            packed_call_type_pattern_accepts(arena, pattern, actual)
        }
        (
            TypeTermHead::Object { shape: pattern, .. },
            TypeTermHead::Object { shape: actual, .. },
        ) => arena
            .object_fields_for_shape(pattern)
            .canonical_iter()
            .all(|field| {
                arena
                    .lookup_object_field(actual, field.name)
                    .is_some_and(|actual| packed_call_type_pattern_accepts(arena, field.ty, actual))
            }),
        (TypeTermHead::Union(pattern), _) => arena
            .term_ids(pattern)
            .iter()
            .any(|pattern| packed_call_type_pattern_accepts(arena, *pattern, actual)),
        (_, TypeTermHead::Union(actual)) => arena
            .term_ids(actual)
            .iter()
            .all(|actual| packed_call_type_pattern_accepts(arena, pattern, *actual)),
        _ => pattern == actual || packed_resolved_type_is_assignable_to(arena, actual, pattern),
    }
}

pub(crate) fn packed_call_type_is_placeholder(arena: &TypeTermArena, term: TypeTermId) -> bool {
    match arena.term_head(term) {
        TypeTermHead::Unknown
        | TypeTermHead::Variable(_)
        | TypeTermHead::UnresolvedShape(_)
        | TypeTermHead::OpenObjectPlaceholder => true,
        TypeTermHead::Object { shape, open } => {
            open && arena.object_fields_for_shape(shape).is_empty()
        }
        _ => false,
    }
}

pub(crate) fn packed_type_is_assignable_to(
    arena: &TypeTermArena,
    actual: TypeTermId,
    expected: TypeTermId,
) -> bool {
    if actual == expected {
        return true;
    }
    if packed_call_type_is_placeholder(arena, actual)
        || packed_call_type_is_placeholder(arena, expected)
    {
        return true;
    }
    match (arena.term_head(actual), arena.term_head(expected)) {
        (TypeTermHead::RenderContract, _) | (_, TypeTermHead::RenderContract) => true,
        (TypeTermHead::Union(actual), _) if actual.len() == 0 => false,
        (_, TypeTermHead::Union(expected)) if expected.len() == 0 => false,
        (TypeTermHead::Union(actual), TypeTermHead::Union(expected)) => {
            arena.term_ids(actual).iter().all(|actual| {
                arena
                    .term_ids(expected)
                    .iter()
                    .any(|expected| packed_type_is_assignable_to(arena, *actual, *expected))
            })
        }
        (TypeTermHead::Union(actual), _) => arena
            .term_ids(actual)
            .iter()
            .all(|actual| packed_type_is_assignable_to(arena, *actual, expected)),
        (_, TypeTermHead::Union(expected)) => arena
            .term_ids(expected)
            .iter()
            .any(|expected| packed_type_is_assignable_to(arena, actual, *expected)),
        (TypeTermHead::Text, TypeTermHead::Text)
        | (TypeTermHead::Number, TypeTermHead::Number)
        | (TypeTermHead::Absent, TypeTermHead::Absent) => true,
        (TypeTermHead::Bytes(actual), TypeTermHead::Bytes(expected)) => match (actual, expected) {
            (_, crate::BytesTerm::Dynamic) => true,
            (crate::BytesTerm::Fixed(actual), crate::BytesTerm::Fixed(expected)) => {
                actual == expected
            }
            (crate::BytesTerm::Dynamic, crate::BytesTerm::Fixed(_)) => false,
        },
        (TypeTermHead::Bits(actual), TypeTermHead::Bits(expected)) => actual == expected,
        (TypeTermHead::List(actual), TypeTermHead::List(expected))
        | (TypeTermHead::Set(actual), TypeTermHead::Set(expected)) => {
            packed_type_is_assignable_to(arena, actual, expected)
        }
        (
            TypeTermHead::Map {
                key: actual_key,
                value: actual_value,
            },
            TypeTermHead::Map {
                key: expected_key,
                value: expected_value,
            },
        ) => {
            packed_type_is_assignable_to(arena, actual_key, expected_key)
                && packed_type_is_assignable_to(arena, actual_value, expected_value)
        }
        (
            TypeTermHead::Object {
                shape: actual,
                open: actual_open,
            },
            TypeTermHead::Object {
                shape: expected, ..
            },
        ) => arena
            .object_fields_for_shape(expected)
            .canonical_iter()
            .all(|field| {
                arena
                    .lookup_object_field(actual, field.name)
                    .is_some_and(|actual| packed_type_is_assignable_to(arena, actual, field.ty))
                    || actual_open
            }),
        (
            TypeTermHead::VariantSet(actual),
            TypeTermHead::Object {
                shape: expected, ..
            },
        ) => arena
            .variant_terms(actual)
            .iter()
            .all(|variant| match *variant {
                VariantTerm::Tag(_) => arena.object_fields_for_shape(expected).is_empty(),
                VariantTerm::Tagged { fields, .. } => {
                    let TypeTermHead::Object { shape: actual, .. } = arena.term_head(fields) else {
                        return false;
                    };
                    packed_object_is_assignable_to(arena, actual, expected, false)
                }
            }),
        (TypeTermHead::VariantSet(actual), TypeTermHead::VariantSet(expected)) => {
            arena.variant_terms(actual).iter().all(|actual| {
                arena
                    .variant_terms(expected)
                    .iter()
                    .any(|expected| packed_variant_is_assignable_to(arena, *actual, *expected))
            })
        }
        (
            TypeTermHead::Function {
                args: actual_args,
                result_mode: actual_mode,
                result: actual_result,
            },
            TypeTermHead::Function {
                args: expected_args,
                result_mode: expected_mode,
                result: expected_result,
            },
        ) => {
            actual_args.len() == expected_args.len()
                && actual_mode == expected_mode
                && arena
                    .term_ids(expected_args)
                    .iter()
                    .zip(arena.term_ids(actual_args))
                    .all(|(expected, actual)| {
                        packed_type_is_assignable_to(arena, *expected, *actual)
                    })
                && packed_type_is_assignable_to(arena, actual_result, expected_result)
        }
        _ => false,
    }
}

fn packed_object_is_assignable_to(
    arena: &TypeTermArena,
    actual: u32,
    expected: u32,
    actual_open: bool,
) -> bool {
    arena
        .object_fields_for_shape(expected)
        .canonical_iter()
        .all(|field| {
            arena
                .lookup_object_field(actual, field.name)
                .is_some_and(|actual| packed_type_is_assignable_to(arena, actual, field.ty))
                || actual_open
        })
}

fn packed_variant_is_assignable_to(
    arena: &TypeTermArena,
    actual: VariantTerm,
    expected: VariantTerm,
) -> bool {
    match (actual, expected) {
        (VariantTerm::Tag(actual), VariantTerm::Tag(expected)) => actual == expected,
        (
            VariantTerm::Tagged {
                tag: actual_tag,
                fields: actual_fields,
            },
            VariantTerm::Tagged {
                tag: expected_tag,
                fields: expected_fields,
            },
        ) => {
            let (
                TypeTermHead::Object {
                    shape: actual,
                    open: actual_open,
                },
                TypeTermHead::Object {
                    shape: expected, ..
                },
            ) = (
                arena.term_head(actual_fields),
                arena.term_head(expected_fields),
            )
            else {
                return false;
            };
            actual_tag == expected_tag
                && packed_object_is_assignable_to(arena, actual, expected, actual_open)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callable_parameter_collection_preserves_authored_object_order() {
        let mut arena = TypeTermArena::new();
        let z = arena.intern_name("z");
        let a = arena.intern_name("a");
        let z_variable = TypeVariableId(9);
        let a_variable = TypeVariableId(7);
        let result_variable = TypeVariableId(11);
        let z_term = arena.variable(z_variable);
        let a_term = arena.variable(a_variable);
        let result = arena.variable(result_variable);
        let formal = arena.object([(z, z_term), (a, a_term)], false);

        let mut scratch = PackedCallableParameterTraversalScratch::default();
        let mut parameters = Vec::new();
        collect_packed_callable_type_parameter_sources(
            &arena,
            [formal],
            result,
            &mut scratch,
            &mut parameters,
        );

        assert_eq!(parameters, [z_variable, a_variable, result_variable]);
    }

    #[test]
    fn callable_parameter_collection_follows_packed_generic_union_order() {
        let mut arena = TypeTermArena::new();
        let field = arena.intern_name("value");
        let list_variable = TypeVariableId(31);
        let object_variable = TypeVariableId(37);
        let list_term = arena.variable(list_variable);
        let object_term = arena.variable(object_variable);
        let list = arena.list(list_term);
        let object = arena.object([(field, object_term)], false);
        // Packed unions canonicalize by type kind, so Object precedes List
        // even when candidates arrive in the opposite order. The retained
        // target-parameter sequence must use this traversal; rebuilding a
        // rich Type and Debug-sorting it used to swap T and U.
        let formal = arena.union([list, object]);

        let mut scratch = PackedCallableParameterTraversalScratch::default();
        let mut parameters = Vec::new();
        collect_packed_callable_type_parameter_sources(
            &arena,
            [formal],
            arena.number(),
            &mut scratch,
            &mut parameters,
        );

        assert_eq!(parameters, [object_variable, list_variable]);
    }
}

fn packed_resolved_type_is_assignable_to(
    arena: &TypeTermArena,
    actual: TypeTermId,
    expected: TypeTermId,
) -> bool {
    if actual == expected {
        return true;
    }
    match (arena.term_head(actual), arena.term_head(expected)) {
        (
            TypeTermHead::Unknown | TypeTermHead::Variable(_) | TypeTermHead::UnresolvedShape(_),
            _,
        )
        | (
            _,
            TypeTermHead::Unknown | TypeTermHead::Variable(_) | TypeTermHead::UnresolvedShape(_),
        ) => false,
        (TypeTermHead::Union(actual), _) if actual.len() == 0 => false,
        (_, TypeTermHead::Union(expected)) if expected.len() == 0 => false,
        (TypeTermHead::Union(actual), TypeTermHead::Union(expected)) => {
            arena.term_ids(actual).iter().all(|actual| {
                arena.term_ids(expected).iter().any(|expected| {
                    packed_resolved_type_is_assignable_to(arena, *actual, *expected)
                })
            })
        }
        (TypeTermHead::Union(actual), _) => arena
            .term_ids(actual)
            .iter()
            .all(|actual| packed_resolved_type_is_assignable_to(arena, *actual, expected)),
        (_, TypeTermHead::Union(expected)) => arena
            .term_ids(expected)
            .iter()
            .any(|expected| packed_resolved_type_is_assignable_to(arena, actual, *expected)),
        (TypeTermHead::Text, TypeTermHead::Text)
        | (TypeTermHead::Number, TypeTermHead::Number)
        | (TypeTermHead::Absent, TypeTermHead::Absent)
        | (TypeTermHead::RenderContract, TypeTermHead::RenderContract) => true,
        (TypeTermHead::Bytes(actual), TypeTermHead::Bytes(expected)) => match (actual, expected) {
            (_, crate::BytesTerm::Dynamic) => true,
            (crate::BytesTerm::Fixed(actual), crate::BytesTerm::Fixed(expected)) => {
                actual == expected
            }
            (crate::BytesTerm::Dynamic, crate::BytesTerm::Fixed(_)) => false,
        },
        (TypeTermHead::Bits(actual), TypeTermHead::Bits(expected)) => actual == expected,
        (TypeTermHead::List(actual), TypeTermHead::List(expected)) => {
            packed_resolved_type_is_assignable_to(arena, actual, expected)
        }
        (
            TypeTermHead::Object { shape: actual, .. },
            TypeTermHead::Object {
                shape: expected, ..
            },
        ) => arena
            .object_fields_for_shape(expected)
            .canonical_iter()
            .all(|field| {
                arena
                    .lookup_object_field(actual, field.name)
                    .is_some_and(|actual| {
                        packed_resolved_type_is_assignable_to(arena, actual, field.ty)
                    })
            }),
        (TypeTermHead::VariantSet(actual), TypeTermHead::VariantSet(expected)) => {
            arena.variant_terms(actual).iter().all(|actual| {
                arena.variant_terms(expected).iter().any(|expected| {
                    packed_resolved_variant_is_assignable_to(arena, *actual, *expected)
                })
            })
        }
        (
            TypeTermHead::Function {
                args: actual_args,
                result_mode: actual_mode,
                result: actual_result,
            },
            TypeTermHead::Function {
                args: expected_args,
                result_mode: expected_mode,
                result: expected_result,
            },
        ) => {
            actual_args.len() == expected_args.len()
                && actual_mode == expected_mode
                && arena
                    .term_ids(expected_args)
                    .iter()
                    .zip(arena.term_ids(actual_args))
                    .all(|(expected, actual)| {
                        packed_resolved_type_is_assignable_to(arena, *expected, *actual)
                    })
                && packed_resolved_type_is_assignable_to(arena, actual_result, expected_result)
        }
        // RenderContract is an API boundary.  The kernel intentionally does
        // not encode any library-specific tag such as `NoElement` here.
        (_, TypeTermHead::RenderContract) => packed_renderable_type(arena, actual),
        _ => false,
    }
}

fn packed_resolved_variant_is_assignable_to(
    arena: &TypeTermArena,
    actual: VariantTerm,
    expected: VariantTerm,
) -> bool {
    match (actual, expected) {
        (VariantTerm::Tag(actual), VariantTerm::Tag(expected)) => actual == expected,
        (
            VariantTerm::Tagged {
                tag: actual_tag,
                fields: actual_fields,
            },
            VariantTerm::Tagged {
                tag: expected_tag,
                fields: expected_fields,
            },
        ) => {
            actual_tag == expected_tag
                && packed_resolved_type_is_assignable_to(arena, actual_fields, expected_fields)
        }
        _ => false,
    }
}

fn packed_renderable_type(arena: &TypeTermArena, term: TypeTermId) -> bool {
    const KINDS: &[&str] = &[
        "Block",
        "Button",
        "Checkbox",
        "Document",
        "EmbeddedMedia",
        "EmbeddedProgram",
        "Label",
        "Link",
        "MapViewport",
        "Paragraph",
        "Row",
        "Scene",
        "Stack",
        "Text",
        "TextInput",
    ];
    match arena.term_head(term) {
        TypeTermHead::RenderContract => true,
        TypeTermHead::Union(members) => {
            !arena.term_ids(members).is_empty()
                && arena
                    .term_ids(members)
                    .iter()
                    .all(|member| packed_renderable_type(arena, *member))
        }
        TypeTermHead::Object { shape, .. } => {
            let Some(kind) = arena.text_snapshot().lookup_symbol("kind") else {
                return false;
            };
            let Some(kind) = arena.lookup_object_field(shape, kind) else {
                return false;
            };
            let TypeTermHead::VariantSet(variants) = arena.term_head(kind) else {
                return false;
            };
            arena.variant_terms(variants).iter().all(|variant| {
                matches!(variant, VariantTerm::Tag(tag) if KINDS.contains(&arena.name(*tag)))
            })
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PackedTypeMismatchKind {
    Type,
    MissingField,
    IncompatibleField,
}

/// Determine the same lexically first nested mismatch as the rich checker,
/// retaining the path as interned symbols in caller-owned scratch.
pub(crate) fn packed_type_mismatch(
    arena: &TypeTermArena,
    actual: TypeTermId,
    expected: TypeTermId,
    path: &mut Vec<SymbolId>,
) -> PackedTypeMismatchKind {
    path.clear();
    if packed_missing_field_path(arena, actual, expected, path) {
        return PackedTypeMismatchKind::MissingField;
    }
    path.clear();
    if packed_incompatible_field_path(arena, actual, expected, path) {
        return PackedTypeMismatchKind::IncompatibleField;
    }
    path.clear();
    PackedTypeMismatchKind::Type
}

fn packed_missing_field_path(
    arena: &TypeTermArena,
    actual: TypeTermId,
    expected: TypeTermId,
    path: &mut Vec<SymbolId>,
) -> bool {
    let (
        TypeTermHead::Object {
            shape: actual,
            open: actual_open,
        },
        TypeTermHead::Object {
            shape: expected, ..
        },
    ) = (arena.term_head(actual), arena.term_head(expected))
    else {
        return false;
    };
    for field in arena.object_fields_for_shape(expected).canonical_iter() {
        path.push(field.name);
        let Some(actual) = arena.lookup_object_field(actual, field.name) else {
            if !actual_open {
                return true;
            }
            path.pop();
            continue;
        };
        if packed_missing_field_path(arena, actual, field.ty, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn packed_incompatible_field_path(
    arena: &TypeTermArena,
    actual: TypeTermId,
    expected: TypeTermId,
    path: &mut Vec<SymbolId>,
) -> bool {
    let (
        TypeTermHead::Object { shape: actual, .. },
        TypeTermHead::Object {
            shape: expected, ..
        },
    ) = (arena.term_head(actual), arena.term_head(expected))
    else {
        return false;
    };
    for field in arena.object_fields_for_shape(expected).canonical_iter() {
        let Some(actual) = arena.lookup_object_field(actual, field.name) else {
            continue;
        };
        path.push(field.name);
        if packed_incompatible_field_path(arena, actual, field.ty, path)
            || !packed_type_is_assignable_to(arena, actual, field.ty)
        {
            return true;
        }
        path.pop();
    }
    false
}
