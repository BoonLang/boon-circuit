use crate::{
    DefinitionArtifact, KernelCallTarget, KernelDeclarationReference, KernelDefinitionFactsInput,
    KernelDiagnosticArtifact, KernelDiagnosticKind, KernelExpressionId, KernelExternalExpression,
    KernelExternalTarget, KernelLexicalBindingTarget, KernelOwnerBuildError, KernelOwnerId,
    KernelOwnerNodeKind, KernelOwnerProgramInput, KernelSolveError, KernelStatementChildReference,
    KernelStatementReference, KernelValueReference,
};
use boon_checked::{FlowType, ObjectShape, Type, TypeVar, Variant};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};

const KERNEL_DEFINITION_BASIS_DOMAIN_V5: &[u8] = b"boon.compiler-kernel.definition-basis.v5\0";
const KERNEL_PUBLIC_RESULT_DOMAIN_V1: &[u8] = b"boon.compiler-kernel.public-result.v1\0";
const KERNEL_EXPRESSION_SURFACE_DOMAIN_V1: &[u8] = b"boon.compiler-kernel.expression-surface.v1\0";
const KERNEL_DEFINITION_ARTIFACT_DOMAIN_V7: &[u8] =
    b"boon.compiler-kernel.definition-artifact.v7\0";
const KERNEL_DEPENDENCY_IMPORTS_DOMAIN_V1: &[u8] = b"boon.compiler-kernel.dependency-imports.v1\0";
const KERNEL_DEFINITION_CURRENTNESS_DOMAIN_V7: &[u8] =
    b"boon.compiler-kernel.definition-currentness.v7\0";

/// Exact definition-local origin of one dependency edge.
///
/// These rows are intentionally structural rather than diagnostic. They make
/// invalidation and retained-result currentness independent of source-tree
/// rediscovery and preserve multiple distinct uses of one provider.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDependencySource {
    ExpressionInput {
        expression: KernelExpressionId,
        input: u32,
    },
    StatementValue {
        statement: crate::KernelStatementId,
    },
    StatementChild {
        statement: crate::KernelStatementId,
        child: u32,
    },
    DeclarationValue {
        declaration: crate::KernelDeclarationId,
    },
    LexicalDeclaration {
        expression: KernelExpressionId,
    },
    LexicalValue {
        expression: KernelExpressionId,
    },
    CallTarget {
        expression: KernelExpressionId,
    },
    CallInput {
        expression: KernelExpressionId,
        input: u32,
    },
    SourceDeclaration {
        source: crate::KernelSourceId,
    },
    SourceStatement {
        source: crate::KernelSourceId,
    },
    SourcePathAnchor {
        source: crate::KernelSourceId,
    },
    StateBindingDeclaration {
        state: crate::KernelStateId,
    },
    StateDeclaration {
        state: crate::KernelStateId,
    },
    StateStatement {
        state: crate::KernelStateId,
    },
    StateInitial {
        state: crate::KernelStateId,
    },
    StatePathAnchor {
        state: crate::KernelStateId,
    },
    ListDeclaration {
        list: crate::KernelListId,
    },
    ListStatement {
        list: crate::KernelListId,
    },
    ListPathAnchor {
        list: crate::KernelListId,
    },
}

/// Exact authority imported from another dense definition.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDependencyTarget {
    Definition(KernelOwnerId),
    PublicDeclaration(KernelOwnerId),
    PublicStatement(KernelOwnerId),
    Expression {
        owner: KernelOwnerId,
        expression: KernelExpressionId,
    },
    Result(KernelOwnerId),
}

impl KernelDependencyTarget {
    pub const fn owner(self) -> KernelOwnerId {
        match self {
            Self::Definition(owner)
            | Self::PublicDeclaration(owner)
            | Self::PublicStatement(owner)
            | Self::Expression { owner, .. }
            | Self::Result(owner) => owner,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelDefinitionDependency {
    pub source: KernelDependencySource,
    pub target: KernelDependencyTarget,
}

/// Definition dependency rows plus a reverse-consumer CSR index.
///
/// `dependencies` preserves exact use sites. `consumers` is deduplicated by
/// definition so one provider mutation schedules each dependent definition at
/// most once.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDefinitionDependencyGraph {
    dependency_offsets: Box<[u32]>,
    dependencies: Box<[KernelDefinitionDependency]>,
    consumer_offsets: Box<[u32]>,
    consumers: Box<[KernelOwnerId]>,
}

impl KernelDefinitionDependencyGraph {
    pub fn definition_count(&self) -> usize {
        self.dependency_offsets.len().saturating_sub(1)
    }

    pub fn dependency_count(&self) -> usize {
        self.dependencies.len()
    }

    pub fn reverse_consumer_count(&self) -> usize {
        self.consumers.len()
    }

    pub fn dependencies(&self, definition: KernelOwnerId) -> Option<&[KernelDefinitionDependency]> {
        let index = definition.0 as usize;
        let start = *self.dependency_offsets.get(index)? as usize;
        let end = *self.dependency_offsets.get(index + 1)? as usize;
        self.dependencies.get(start..end)
    }

    pub fn consumers(&self, provider: KernelOwnerId) -> Option<&[KernelOwnerId]> {
        let index = provider.0 as usize;
        let start = *self.consumer_offsets.get(index)? as usize;
        let end = *self.consumer_offsets.get(index + 1)? as usize;
        self.consumers.get(start..end)
    }

    /// Sorted transitive reverse dependency cone, excluding `provider`.
    pub fn dependent_cone(&self, provider: KernelOwnerId) -> Option<Box<[KernelOwnerId]>> {
        self.consumers(provider)?;
        let mut seen = vec![false; self.definition_count()];
        seen[provider.0 as usize] = true;
        let mut pending = VecDeque::from([provider]);
        let mut cone = Vec::new();
        while let Some(current) = pending.pop_front() {
            for consumer in self
                .consumers(current)
                .expect("dependency graph consumer index is internally complete")
            {
                let index = consumer.0 as usize;
                if !seen[index] {
                    seen[index] = true;
                    cone.push(*consumer);
                    pending.push_back(*consumer);
                }
            }
        }
        cone.sort_unstable();
        Some(cone.into_boxed_slice())
    }
}

/// Separates a reusable semantic artifact from proof that this exact basis and
/// exact imported authority set produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelDefinitionCurrentnessReceipt {
    pub basis_fingerprint_v5: [u8; 32],
    pub public_result_fingerprint_v1: [u8; 32],
    pub artifact_fingerprint_v7: [u8; 32],
    pub dependency_fingerprint_v1: [u8; 32],
    pub fingerprint_v7: [u8; 32],
}

pub(crate) fn definition_basis_fingerprint(
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<[u8; 32], KernelOwnerBuildError> {
    definition_basis_fingerprint_with_buffer(input, facts, &mut Vec::new())
}

pub(crate) fn definition_basis_fingerprint_with_buffer(
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], KernelOwnerBuildError> {
    Ok(stable_fingerprint(
        KERNEL_DEFINITION_BASIS_DOMAIN_V5,
        &(input, facts),
        scratch,
    ))
}

pub(crate) fn build_snapshot_receipts(
    definitions: &mut [DefinitionArtifact],
    basis_fingerprints: &[[u8; 32]],
) -> Result<
    (
        KernelDefinitionDependencyGraph,
        Box<[KernelDefinitionCurrentnessReceipt]>,
    ),
    KernelSolveError,
> {
    if definitions.len() != basis_fingerprints.len() {
        return Err(KernelSolveError::new(format!(
            "kernel snapshot has {} definitions but {} basis fingerprints",
            definitions.len(),
            basis_fingerprints.len()
        )));
    }
    for definition in definitions.iter_mut() {
        alpha_normalize_definition(definition);
    }
    let dependency_graph = build_dependency_graph(definitions)?;
    let mut imported_expressions = vec![BTreeSet::new(); definitions.len()];
    for dependency in dependency_graph.dependencies.iter() {
        if let KernelDependencyTarget::Expression { owner, expression } = dependency.target {
            imported_expressions[owner.0 as usize].insert(expression);
        }
    }
    let mut public_result_fingerprints = Vec::with_capacity(definitions.len());
    let mut artifact_fingerprints = Vec::with_capacity(definitions.len());
    let mut expression_fingerprints = Vec::with_capacity(definitions.len());
    let mut hash_scratch = Vec::new();
    for definition in definitions.iter() {
        public_result_fingerprints.push(hash_normalized_flow_type(
            KERNEL_PUBLIC_RESULT_DOMAIN_V1,
            &definition.result,
            &mut hash_scratch,
        )?);
        artifact_fingerprints.push(stable_fingerprint(
            KERNEL_DEFINITION_ARTIFACT_DOMAIN_V7,
            definition,
            &mut hash_scratch,
        ));
        let definition_index = expression_fingerprints.len();
        let mut definition_expression_fingerprints = BTreeMap::new();
        for expression in definition
            .expressions
            .iter()
            .filter(|expression| imported_expressions[definition_index].contains(&expression.id))
        {
            definition_expression_fingerprints.insert(
                expression.id,
                hash_normalized_flow_type(
                    KERNEL_EXPRESSION_SURFACE_DOMAIN_V1,
                    &expression.flow_type,
                    &mut hash_scratch,
                )?,
            );
        }
        expression_fingerprints.push(definition_expression_fingerprints);
    }

    let mut receipts = Vec::with_capacity(definitions.len());
    for definition_index in 0..definitions.len() {
        let owner = KernelOwnerId(
            u32::try_from(definition_index)
                .expect("kernel definition count exceeds the dense u32 namespace"),
        );
        let dependencies = dependency_graph
            .dependencies(owner)
            .expect("kernel dependency graph contains every definition");
        let imported_authorities = dependencies
            .iter()
            .map(|dependency| {
                let target = dependency.target;
                let provider = target.owner().0 as usize;
                match target {
                    KernelDependencyTarget::Expression { expression, .. } => expression_fingerprints
                        [provider]
                        .get(&expression)
                        .copied()
                        .ok_or_else(|| {
                            KernelSolveError::new(format!(
                                "kernel dependency targets missing expression {} in definition {}",
                                expression.0, provider
                            ))
                        }),
                    KernelDependencyTarget::Definition(_)
                    | KernelDependencyTarget::PublicDeclaration(_)
                    | KernelDependencyTarget::PublicStatement(_)
                    | KernelDependencyTarget::Result(_) => {
                        Ok(public_result_fingerprints[provider])
                    }
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dependency_fingerprint_v1 = stable_fingerprint(
            KERNEL_DEPENDENCY_IMPORTS_DOMAIN_V1,
            &(dependencies, imported_authorities),
            &mut hash_scratch,
        );
        let basis_fingerprint_v5 = basis_fingerprints[definition_index];
        let public_result_fingerprint_v1 = public_result_fingerprints[definition_index];
        let artifact_fingerprint_v7 = artifact_fingerprints[definition_index];
        let fingerprint_v7 = stable_fingerprint(
            KERNEL_DEFINITION_CURRENTNESS_DOMAIN_V7,
            &(
                basis_fingerprint_v5,
                artifact_fingerprint_v7,
                dependency_fingerprint_v1,
            ),
            &mut hash_scratch,
        );
        receipts.push(KernelDefinitionCurrentnessReceipt {
            basis_fingerprint_v5,
            public_result_fingerprint_v1,
            artifact_fingerprint_v7,
            dependency_fingerprint_v1,
            fingerprint_v7,
        });
    }
    Ok((dependency_graph, receipts.into_boxed_slice()))
}

fn build_dependency_graph(
    definitions: &[DefinitionArtifact],
) -> Result<KernelDefinitionDependencyGraph, KernelSolveError> {
    let mut dependency_offsets = Vec::with_capacity(definitions.len() + 1);
    let mut dependencies = Vec::new();
    dependency_offsets.push(0);
    for (definition_index, definition) in definitions.iter().enumerate() {
        validate_definition_diagnostics(definitions, definition_index, definition)?;
        let mut local = definition_dependencies(definition);
        local.sort_unstable();
        local.dedup();
        for dependency in &local {
            validate_dependency_target(definitions, definition_index, dependency.target)?;
        }
        dependencies.extend(local);
        dependency_offsets.push(checked_u32(
            dependencies.len(),
            "kernel definition dependency count",
        )?);
    }

    let mut reverse = vec![BTreeSet::new(); definitions.len()];
    for (consumer_index, range) in dependency_offsets.windows(2).enumerate() {
        let consumer = KernelOwnerId(
            u32::try_from(consumer_index)
                .expect("kernel definition count exceeds the dense u32 namespace"),
        );
        for dependency in &dependencies[range[0] as usize..range[1] as usize] {
            let provider = dependency.target.owner();
            if provider != consumer {
                reverse[provider.0 as usize].insert(consumer);
            }
        }
    }
    let mut consumer_offsets = Vec::with_capacity(definitions.len() + 1);
    let mut consumers = Vec::new();
    consumer_offsets.push(0);
    for provider_consumers in reverse {
        consumers.extend(provider_consumers);
        consumer_offsets.push(checked_u32(
            consumers.len(),
            "kernel reverse dependency consumer count",
        )?);
    }
    Ok(KernelDefinitionDependencyGraph {
        dependency_offsets: dependency_offsets.into_boxed_slice(),
        dependencies: dependencies.into_boxed_slice(),
        consumer_offsets: consumer_offsets.into_boxed_slice(),
        consumers: consumers.into_boxed_slice(),
    })
}

fn validate_definition_diagnostics(
    definitions: &[DefinitionArtifact],
    owner_index: usize,
    definition: &DefinitionArtifact,
) -> Result<(), KernelSolveError> {
    let owner = KernelOwnerId(
        u32::try_from(owner_index)
            .expect("kernel definition count exceeds the dense u32 namespace"),
    );
    for diagnostic in &definition.diagnostics {
        if diagnostic.owner != owner {
            return Err(KernelSolveError::new(format!(
                "kernel definition {owner_index} contains diagnostic owned by definition {}",
                diagnostic.owner.0
            )));
        }
        match diagnostic.site {
            crate::KernelDiagnosticSite::Expression { expression } => {
                if definition
                    .expressions
                    .get(expression.0 as usize)
                    .is_none_or(|candidate| candidate.id != expression)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing expression {}",
                        expression.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallArgument { call, .. }
            | crate::KernelDiagnosticSite::CallPass { call, .. } => {
                if definition
                    .expressions
                    .get(call.0 as usize)
                    .is_none_or(|candidate| candidate.id != call)
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call expression {}",
                        call.0
                    )));
                }
            }
            crate::KernelDiagnosticSite::CallInput {
                call,
                target,
                formal_ordinal,
            } => {
                let Some(target_definition) = definitions.get(target.0 as usize) else {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing definition {}",
                        target.0
                    )));
                };
                if target_definition
                    .formals
                    .get(formal_ordinal as usize)
                    .is_none()
                {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic targets missing formal {formal_ordinal} in definition {}",
                        target.0
                    )));
                }
                let call_matches = definition.calls.iter().any(|candidate| {
                    candidate.expression == call
                        && matches!(
                            candidate.target,
                            KernelCallTarget::User {
                                target: candidate_target,
                                ..
                            } if candidate_target == target
                        )
                        && candidate.inputs.iter().any(|input| {
                            matches!(
                                input.role,
                                crate::KernelCallInputRole::Formal { ordinal }
                                    if ordinal == formal_ordinal
                            )
                        })
                });
                if !call_matches {
                    return Err(KernelSolveError::new(format!(
                        "kernel definition {owner_index} diagnostic references missing call input {} formal {formal_ordinal} targeting definition {}",
                        call.0, target.0
                    )));
                }
            }
        }
    }
    Ok(())
}

fn definition_dependencies(definition: &DefinitionArtifact) -> Vec<KernelDefinitionDependency> {
    let mut dependencies = Vec::new();
    for expression in &definition.expressions {
        for (input, edge) in expression.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::ExpressionInput {
                    expression: expression.id,
                    input: dense_index(input),
                },
                edge.value,
            );
        }
    }
    for statement in &definition.statements {
        if let Some(value) = statement.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::StatementValue {
                    statement: statement.id,
                },
                value,
            );
        }
        for (child, reference) in statement.children.iter().enumerate() {
            if let KernelStatementChildReference::Owner(owner) = reference {
                dependencies.push(KernelDefinitionDependency {
                    source: KernelDependencySource::StatementChild {
                        statement: statement.id,
                        child: dense_index(child),
                    },
                    target: KernelDependencyTarget::Definition(*owner),
                });
            }
        }
    }
    for declaration in &definition.declarations {
        if let Some(value) = declaration.value {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::DeclarationValue {
                    declaration: declaration.id,
                },
                value,
            );
        }
    }
    for binding in &definition.lexical_bindings {
        match binding.target {
            KernelLexicalBindingTarget::Declaration(reference) => push_declaration_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalDeclaration {
                    expression: binding.expression,
                },
                reference,
            ),
            KernelLexicalBindingTarget::Value { provider } => push_value_dependency(
                &mut dependencies,
                KernelDependencySource::LexicalValue {
                    expression: binding.expression,
                },
                provider,
            ),
            KernelLexicalBindingTarget::ContextFormal { .. }
            | KernelLexicalBindingTarget::RuntimeContext => {}
        }
    }
    for call in &definition.calls {
        if let KernelCallTarget::User { target, .. } = call.target {
            dependencies.push(KernelDefinitionDependency {
                source: KernelDependencySource::CallTarget {
                    expression: call.expression,
                },
                target: KernelDependencyTarget::Definition(target),
            });
        }
        for (input, argument) in call.inputs.iter().enumerate() {
            push_value_dependency(
                &mut dependencies,
                KernelDependencySource::CallInput {
                    expression: call.expression,
                    input: dense_index(input),
                },
                argument.value,
            );
        }
    }
    for source in &definition.sources {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourceDeclaration { source: source.id },
            source.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::SourceStatement { source: source.id },
            source.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::SourcePathAnchor { source: source.id },
            source.path.anchor,
        );
    }
    for state in &definition.states {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateBindingDeclaration { state: state.id },
            state.binding_declaration,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StateDeclaration { state: state.id },
            state.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::StateStatement { state: state.id },
            state.statement,
        );
        push_value_dependency(
            &mut dependencies,
            KernelDependencySource::StateInitial { state: state.id },
            state.initial,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::StatePathAnchor { state: state.id },
            state.path.anchor,
        );
    }
    for list in &definition.lists {
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListDeclaration { list: list.id },
            list.declaration,
        );
        push_statement_dependency(
            &mut dependencies,
            KernelDependencySource::ListStatement { list: list.id },
            list.statement,
        );
        push_declaration_dependency(
            &mut dependencies,
            KernelDependencySource::ListPathAnchor { list: list.id },
            list.path.anchor,
        );
    }
    dependencies
}

fn push_value_dependency(
    dependencies: &mut Vec<KernelDefinitionDependency>,
    source: KernelDependencySource,
    value: KernelValueReference,
) {
    let KernelValueReference::External(external) = value else {
        return;
    };
    dependencies.push(KernelDefinitionDependency {
        source,
        target: external_target(external),
    });
}

fn external_target(external: KernelExternalExpression) -> KernelDependencyTarget {
    match external.target {
        KernelExternalTarget::Expression(expression) => KernelDependencyTarget::Expression {
            owner: external.owner,
            expression,
        },
        KernelExternalTarget::Result => KernelDependencyTarget::Result(external.owner),
    }
}

fn push_declaration_dependency(
    dependencies: &mut Vec<KernelDefinitionDependency>,
    source: KernelDependencySource,
    reference: KernelDeclarationReference,
) {
    if let KernelDeclarationReference::OwnerPublic(owner) = reference {
        dependencies.push(KernelDefinitionDependency {
            source,
            target: KernelDependencyTarget::PublicDeclaration(owner),
        });
    }
}

fn push_statement_dependency(
    dependencies: &mut Vec<KernelDefinitionDependency>,
    source: KernelDependencySource,
    reference: KernelStatementReference,
) {
    if let KernelStatementReference::OwnerPublic(owner) = reference {
        dependencies.push(KernelDefinitionDependency {
            source,
            target: KernelDependencyTarget::PublicStatement(owner),
        });
    }
}

fn validate_dependency_target(
    definitions: &[DefinitionArtifact],
    consumer: usize,
    target: KernelDependencyTarget,
) -> Result<(), KernelSolveError> {
    let provider = target.owner().0 as usize;
    let Some(definition) = definitions.get(provider) else {
        return Err(KernelSolveError::new(format!(
            "kernel definition {consumer} depends on missing definition {provider}"
        )));
    };
    if let KernelDependencyTarget::Expression { expression, .. } = target
        && definition
            .expressions
            .get(expression.0 as usize)
            .is_none_or(|candidate| candidate.id != expression)
    {
        return Err(KernelSolveError::new(format!(
            "kernel definition {consumer} depends on missing expression {} in definition {provider}",
            expression.0
        )));
    }
    Ok(())
}

pub(crate) fn alpha_normalize_definition(normalized: &mut DefinitionArtifact) {
    let mut variables = BTreeMap::new();
    let mut next = 0;
    for formal in &mut normalized.formals {
        *formal = alpha_normalize_flow_type(formal, &mut variables, &mut next);
    }
    normalized.result = alpha_normalize_flow_type(&normalized.result, &mut variables, &mut next);
    for expression in &mut normalized.expressions {
        expression.flow_type =
            alpha_normalize_flow_type(&expression.flow_type, &mut variables, &mut next);
        match &mut expression.kind {
            KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty) => {
                *ty = alpha_normalize_type(ty, &mut variables, &mut next);
            }
            _ => {}
        }
    }
    for call in &mut normalized.calls {
        call.result = alpha_normalize_flow_type(&call.result, &mut variables, &mut next);
        for substitution in &mut call.type_substitutions {
            substitution.value =
                alpha_normalize_type(&substitution.value, &mut variables, &mut next);
        }
    }
    for source in &mut normalized.sources {
        source.payload_type = alpha_normalize_type(&source.payload_type, &mut variables, &mut next);
    }
    for state in &mut normalized.states {
        state.flow_type = alpha_normalize_flow_type(&state.flow_type, &mut variables, &mut next);
    }
    for list in &mut normalized.lists {
        list.item_type = alpha_normalize_type(&list.item_type, &mut variables, &mut next);
    }
    alpha_normalize_diagnostics(&mut normalized.diagnostics, &mut variables, &mut next);
}

pub(crate) fn alpha_normalize_public_flow(flow_type: &FlowType) -> FlowType {
    alpha_normalize_flow_type(flow_type, &mut BTreeMap::new(), &mut 0)
}

/// Normalize one public callable surface and its definition-local diagnostics
/// in one stable variable namespace. This is the diagnostics-only counterpart
/// of `alpha_normalize_definition`; it never materializes checked rows.
pub(crate) fn alpha_normalize_callable_interface_and_diagnostics(
    formals: &[FlowType],
    result: &FlowType,
    diagnostics: &[KernelDiagnosticArtifact],
    diagnostic_values: &[Type],
) -> (
    Box<[FlowType]>,
    FlowType,
    Box<[KernelDiagnosticArtifact]>,
    Box<[Type]>,
) {
    let mut variables = BTreeMap::new();
    let mut next = 0;
    let formals = formals
        .iter()
        .map(|formal| alpha_normalize_flow_type(formal, &mut variables, &mut next))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let result = alpha_normalize_flow_type(result, &mut variables, &mut next);
    let mut diagnostics = diagnostics.to_vec();
    alpha_normalize_diagnostics(&mut diagnostics, &mut variables, &mut next);
    let diagnostic_values = diagnostic_values
        .iter()
        .map(|ty| alpha_normalize_type(ty, &mut variables, &mut next))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    (
        formals,
        result,
        diagnostics.into_boxed_slice(),
        diagnostic_values,
    )
}

fn alpha_normalize_diagnostics(
    diagnostics: &mut [KernelDiagnosticArtifact],
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) {
    for diagnostic in diagnostics {
        match &mut diagnostic.kind {
            KernelDiagnosticKind::CallInputType {
                actual, expected, ..
            } => {
                *actual = alpha_normalize_type(actual, variables, next);
                *expected = alpha_normalize_type(expected, variables, next);
            }
            KernelDiagnosticKind::InvalidExpression { .. }
            | KernelDiagnosticKind::InvalidPattern
            | KernelDiagnosticKind::InvalidNumberLiteral { .. }
            | KernelDiagnosticKind::InvalidBitsLiteral { .. }
            | KernelDiagnosticKind::ByteLiteralOutsideBytes
            | KernelDiagnosticKind::DuplicateRecordField { .. }
            | KernelDiagnosticKind::MissingPassedContext
            | KernelDiagnosticKind::UnresolvedValue { .. }
            | KernelDiagnosticKind::CallableUsedAsValue { .. }
            | KernelDiagnosticKind::AmbiguousValue { .. }
            | KernelDiagnosticKind::UnresolvedCallable { .. }
            | KernelDiagnosticKind::AmbiguousCallable { .. }
            | KernelDiagnosticKind::PipeWithoutValueInput { .. }
            | KernelDiagnosticKind::UnexpectedCallEntry { .. }
            | KernelDiagnosticKind::MisorderedCallEntry { .. }
            | KernelDiagnosticKind::MissingCallEntry { .. }
            | KernelDiagnosticKind::BareOrdinaryInput { .. }
            | KernelDiagnosticKind::PassOnAuthoritativeCallable { .. }
            | KernelDiagnosticKind::MissingPassContext { .. } => {}
        }
    }
}

fn hash_normalized_flow_type(
    domain: &[u8],
    flow_type: &FlowType,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], KernelSolveError> {
    let normalized = alpha_normalize_public_flow(flow_type);
    Ok(stable_fingerprint(domain, &normalized, scratch))
}

fn alpha_normalize_flow_type(
    flow_type: &FlowType,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> FlowType {
    FlowType {
        mode: flow_type.mode,
        ty: alpha_normalize_type(&flow_type.ty, variables, next),
    }
}

fn alpha_normalize_type(
    ty: &Type,
    variables: &mut BTreeMap<TypeVar, TypeVar>,
    next: &mut u32,
) -> Type {
    match ty {
        Type::Var(variable) => Type::Var(*variables.entry(*variable).or_insert_with(|| {
            let normalized = TypeVar(*next);
            *next = next.saturating_add(1);
            normalized
        })),
        Type::Object(shape) => Type::object(ObjectShape {
            fields: shape
                .fields
                .iter()
                .map(|(name, ty)| (name.clone(), alpha_normalize_type(ty, variables, next)))
                .collect(),
            field_order: shape.field_order.clone(),
            open: shape.open,
        }),
        Type::List(item) => Type::List(Type::shared(alpha_normalize_type(item, variables, next))),
        Type::Set(item) => Type::Set(Type::shared(alpha_normalize_type(item, variables, next))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(alpha_normalize_type(key, variables, next)),
            value: Box::new(alpha_normalize_type(value, variables, next)),
        },
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|argument| alpha_normalize_type(argument, variables, next))
                .collect(),
            result: Box::new(alpha_normalize_flow_type(result, variables, next)),
        },
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => Variant::Tag(tag.clone()),
                    Variant::Tagged { tag, fields } => Variant::Tagged {
                        tag: tag.clone(),
                        fields: ObjectShape {
                            fields: fields
                                .fields
                                .iter()
                                .map(|(name, ty)| {
                                    (name.clone(), alpha_normalize_type(ty, variables, next))
                                })
                                .collect(),
                            field_order: fields.field_order.clone(),
                            open: fields.open,
                        }
                        .into(),
                    },
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| alpha_normalize_type(member, variables, next))
                .collect(),
        ),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown
        | Type::Bits { .. } => ty.clone(),
    }
}

fn checked_u32(value: usize, context: &str) -> Result<u32, KernelSolveError> {
    u32::try_from(value).map_err(|_| KernelSolveError::new(format!("{context} exceeds u32")))
}

fn dense_index(index: usize) -> u32 {
    u32::try_from(index).expect("kernel local artifact row count exceeds u32")
}

/// Direct deterministic structural hashing for hot kernel inventories.
///
/// Rust's derived `Hash` walk gives us the compact typed event stream; this
/// hasher fixes integer encoding to big endian and widens pointer-sized values
/// to 64 bits so fingerprints are process- and target-width-independent. A
/// schema/domain revision is required when a hashed DTO or enum order changes.
struct StableSha256Hasher<'a>(&'a mut Vec<u8>);

impl<'a> StableSha256Hasher<'a> {
    fn new(domain: &[u8], bytes: &'a mut Vec<u8>) -> Self {
        bytes.clear();
        bytes.extend_from_slice(&(domain.len() as u64).to_be_bytes());
        bytes.extend_from_slice(domain);
        Self(bytes)
    }

    fn finalize(self) -> [u8; 32] {
        Sha256::digest(self.0.as_slice()).into()
    }
}

impl Hasher for StableSha256Hasher<'_> {
    fn finish(&self) -> u64 {
        let digest = Sha256::digest(self.0.as_slice());
        u64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest always contains eight prefix bytes"),
        )
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    fn write_u8(&mut self, value: u8) {
        self.write(&value.to_be_bytes());
    }

    fn write_u16(&mut self, value: u16) {
        self.write(&value.to_be_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write(&value.to_be_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write(&value.to_be_bytes());
    }

    fn write_u128(&mut self, value: u128) {
        self.write(&value.to_be_bytes());
    }

    fn write_usize(&mut self, value: usize) {
        self.write(&(value as u64).to_be_bytes());
    }

    fn write_i8(&mut self, value: i8) {
        self.write(&value.to_be_bytes());
    }

    fn write_i16(&mut self, value: i16) {
        self.write(&value.to_be_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write(&value.to_be_bytes());
    }

    fn write_i64(&mut self, value: i64) {
        self.write(&value.to_be_bytes());
    }

    fn write_i128(&mut self, value: i128) {
        self.write(&value.to_be_bytes());
    }

    fn write_isize(&mut self, value: isize) {
        self.write(&(value as i64).to_be_bytes());
    }
}

fn stable_fingerprint<T: Hash + ?Sized>(
    domain: &[u8],
    value: &T,
    scratch: &mut Vec<u8>,
) -> [u8; 32] {
    let mut hasher = StableSha256Hasher::new(domain, scratch);
    value.hash(&mut hasher);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        KernelDefinitionFactsInput, KernelDiagnosticInput, KernelDiagnosticSeverity,
        KernelDiagnosticSite, KernelOwnerEdgeRole, KernelOwnerInputEdge, KernelOwnerNode,
        KernelProjectProgramInput, compile_owner_program_with_definition_facts,
        compile_project_program_with_definition_facts,
    };
    use boon_checked::FlowMode;

    fn value_owner(nodes: Vec<KernelOwnerNode>) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: nodes.into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        }
    }

    fn external_result_owner(provider: u32) -> KernelOwnerProgramInput {
        KernelOwnerProgramInput {
            nodes: Box::new([KernelOwnerNode {
                kind: KernelOwnerNodeKind::ValueRead {
                    fields: Box::new([]),
                    mode_narrowing: None,
                },
                inputs: Box::new([KernelOwnerInputEdge {
                    role: KernelOwnerEdgeRole::ReadProvider,
                    expression: KernelExpressionId(1),
                }]),
                mode: FlowMode::Continuous,
            }]),
            formal_count: 0,
            external_expressions: Box::new([KernelExternalExpression {
                owner: KernelOwnerId(provider),
                target: KernelExternalTarget::Result,
            }]),
            result: KernelExpressionId(0),
        }
    }

    fn solve_project(owners: Vec<KernelOwnerProgramInput>) -> crate::KernelCheckedSnapshot {
        let facts = vec![KernelDefinitionFactsInput::default(); owners.len()].into_boxed_slice();
        compile_project_program_with_definition_facts(
            &KernelProjectProgramInput {
                owners: owners.into_boxed_slice(),
            },
            &facts,
        )
        .expect("dependency fixture compiles")
        .solve()
        .expect("dependency fixture solves")
    }

    #[test]
    fn dependency_graph_keeps_exact_edges_and_reverse_definition_cones() {
        let snapshot = solve_project(vec![
            value_owner(vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]),
            external_result_owner(0),
            external_result_owner(1),
        ]);

        assert_eq!(
            snapshot.dependencies.dependencies(KernelOwnerId(0)),
            Some(&[][..])
        );
        assert_eq!(
            snapshot.dependencies.dependencies(KernelOwnerId(1)),
            Some(
                &[KernelDefinitionDependency {
                    source: KernelDependencySource::ExpressionInput {
                        expression: KernelExpressionId(0),
                        input: 0,
                    },
                    target: KernelDependencyTarget::Result(KernelOwnerId(0)),
                }][..]
            )
        );
        assert_eq!(
            snapshot.dependencies.consumers(KernelOwnerId(0)),
            Some(&[KernelOwnerId(1)][..])
        );
        assert_eq!(
            snapshot.dependencies.dependent_cone(KernelOwnerId(0)),
            Some(vec![KernelOwnerId(1), KernelOwnerId(2)].into_boxed_slice())
        );
    }

    #[test]
    fn semantic_backdating_is_separate_from_exact_definition_currentness() {
        let first = solve_project(vec![
            value_owner(vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]),
            external_result_owner(0),
        ]);
        let second = solve_project(vec![
            value_owner(vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Absent,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]),
            external_result_owner(0),
        ]);

        assert_eq!(
            first.currentness[0].public_result_fingerprint_v1,
            second.currentness[0].public_result_fingerprint_v1,
            "an unused implementation edit must preserve the public type identity"
        );
        assert_ne!(
            first.currentness[0].artifact_fingerprint_v7,
            second.currentness[0].artifact_fingerprint_v7
        );
        assert_ne!(
            first.currentness[0].fingerprint_v7, second.currentness[0].fingerprint_v7,
            "the edited definition must not claim the old exact evaluation receipt"
        );
        assert_eq!(
            first.currentness[1], second.currentness[1],
            "a dependent definition can backdate when its imported public authority is unchanged"
        );
    }

    #[test]
    fn semantic_artifact_fingerprints_alpha_normalize_type_variables() {
        let definition = |variable| DefinitionArtifact {
            result: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(variable)),
            },
            formals: Box::new([]),
            relocations: crate::KernelDefinitionRelocations::default(),
            expression_payloads: Box::new([]),
            expressions: Box::new([crate::KernelExpressionArtifact {
                id: KernelExpressionId(0),
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                flow_type: FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Var(TypeVar(variable)),
                },
            }]),
            statements: Box::new([]),
            declarations: Box::new([]),
            lexical_bindings: Box::new([]),
            calls: Box::new([]),
            effects: Box::new([]),
            sources: Box::new([]),
            states: Box::new([]),
            lists: Box::new([]),
            diagnostics: Box::new([]),
        };
        let mut first_definition = [definition(7)];
        let mut second_definition = [definition(91)];
        let (_, first) = build_snapshot_receipts(&mut first_definition, &[[7; 32]])
            .expect("first generic artifact fingerprints");
        let (_, second) = build_snapshot_receipts(&mut second_definition, &[[91; 32]])
            .expect("second generic artifact fingerprints");

        assert_eq!(
            first[0].public_result_fingerprint_v1,
            second[0].public_result_fingerprint_v1
        );
        assert_eq!(
            first[0].artifact_fingerprint_v7,
            second[0].artifact_fingerprint_v7
        );
        assert_eq!(
            first[0].artifact_fingerprint_v7,
            [
                84, 136, 30, 58, 8, 134, 216, 246, 182, 255, 115, 94, 60, 110, 37, 51, 101, 172,
                179, 89, 69, 239, 230, 213, 21, 33, 182, 69, 29, 123, 19, 195,
            ],
            "the V7 direct structural fingerprint byte contract changed"
        );
        assert_ne!(
            first[0].basis_fingerprint_v5,
            second[0].basis_fingerprint_v5
        );
        assert_ne!(first[0].fingerprint_v7, second[0].fingerprint_v7);
    }

    #[test]
    fn diagnostic_facts_change_exact_artifact_currentness_but_not_public_interfaces() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: crate::KernelPureBuiltinKind::TextLength,
                    },
                    inputs: Box::new([KernelOwnerInputEdge {
                        role: KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        expression: KernelExpressionId(0),
                    }]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([KernelOwnerInputEdge {
                        role: KernelOwnerEdgeRole::CallArgument { ordinal: 0 },
                        expression: KernelExpressionId(0),
                    }]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let mut diagnosed = solve_project(vec![callee, caller]);
        assert_eq!(diagnosed.definitions[1].diagnostics.len(), 1);
        let mut clean = diagnosed.clone();
        clean.definitions[1].diagnostics = Box::new([]);
        let basis = [[9; 32]; 2];
        let (_, clean_currentness) =
            build_snapshot_receipts(&mut clean.definitions, &basis).unwrap();
        let (_, diagnosed_currentness) =
            build_snapshot_receipts(&mut diagnosed.definitions, &basis).unwrap();

        assert_eq!(
            clean_currentness[1].public_result_fingerprint_v1,
            diagnosed_currentness[1].public_result_fingerprint_v1
        );
        assert_ne!(
            clean_currentness[1].artifact_fingerprint_v7,
            diagnosed_currentness[1].artifact_fingerprint_v7
        );
        assert_ne!(
            clean_currentness[1].fingerprint_v7,
            diagnosed_currentness[1].fingerprint_v7
        );
    }

    #[test]
    fn typed_call_diagnostics_change_basis_and_exact_currentness_not_public_type() {
        let input = value_owner(vec![KernelOwnerNode {
            kind: KernelOwnerNodeKind::Unknown,
            inputs: Box::new([]),
            mode: FlowMode::Continuous,
        }]);
        let facts = |kind| KernelDefinitionFactsInput {
            diagnostics: vec![KernelDiagnosticInput {
                severity: KernelDiagnosticSeverity::Error,
                site: KernelDiagnosticSite::Expression {
                    expression: KernelExpressionId(0),
                },
                kind,
            }]
            .into_boxed_slice(),
            ..KernelDefinitionFactsInput::default()
        };
        let first = compile_owner_program_with_definition_facts(
            &input,
            &facts(KernelDiagnosticKind::UnresolvedCallable {
                function: "first".into(),
            }),
        )
        .expect("first call diagnostic compiles")
        .solve()
        .expect("first call diagnostic solves");
        let second = compile_owner_program_with_definition_facts(
            &input,
            &facts(KernelDiagnosticKind::MissingCallEntry {
                function: "second".into(),
                name: "value".into(),
            }),
        )
        .expect("second call diagnostic compiles")
        .solve()
        .expect("second call diagnostic solves");

        assert_eq!(first.definition.result, second.definition.result);
        assert_eq!(
            first.currentness.public_result_fingerprint_v1,
            second.currentness.public_result_fingerprint_v1
        );
        assert_ne!(
            first.currentness.basis_fingerprint_v5,
            second.currentness.basis_fingerprint_v5
        );
        assert_ne!(
            first.currentness.artifact_fingerprint_v7,
            second.currentness.artifact_fingerprint_v7
        );
        assert_ne!(
            first.currentness.fingerprint_v7,
            second.currentness.fingerprint_v7
        );
    }
}
