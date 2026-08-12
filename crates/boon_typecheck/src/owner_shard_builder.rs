//! Construction of immutable checked rows for one authored owner.
//!
//! The builder consumes only span-free owner requests plus frozen interfaces
//! and the authoritative ABI.  It never opens `CheckedProgramDatabase` or a
//! project-wide checked image.  Linked dense identities and source spans are
//! deliberately deferred to the non-checking compatibility assembler.

use crate::{
    InferredOwnerCall, InferredOwnerCallableTarget, OwnerAbiEvaluationScope, OwnerAbiValueContract,
    OwnerArgumentKind, OwnerBodyInferenceCurrentnessReceipt, OwnerBodyInferenceShard,
    OwnerCheckedReceiptSink, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerConstructionAbiEnvironment, OwnerContainingScopeInput, OwnerDeclarationKind,
    OwnerEffectiveLexicalTarget, OwnerInferenceAbiEnvironment, OwnerInterfaceEvaluationScope,
    OwnerInterfaceSccResult, OwnerLexicalDeclarationTarget, OwnerLexicalPlan,
    OwnerLexicalScopeOrigin, OwnerParameterKind, OwnerPublicInterface, OwnerReferenceKind,
    OwnerSignatureCallPlan, OwnerSignatureCallTarget, OwnerSignatureDeclarationKind,
    OwnerSignatureDeclarationPlan, OwnerSignatureDeclarationTarget,
    OwnerSignatureMatchedInputSource, OwnerSignatureOutputBindingPlan, OwnerSignaturePassSource,
    OwnerSourceAnchorSite, OwnerSymbolResolution, OwnerSyntaxGraph, OwnerSyntaxInput,
    hold_alias_declaration_target, owner_abi_value_declaration_key,
};
use boon_checked::{
    CheckedCallContextKind, CheckedCallableKind, CheckedDeclarationKind, CheckedIntrinsicV1,
    CheckedListKeyPolicy, CheckedOwnerRows, CheckedParameterKind, CheckedPassedAccess,
    CheckedScopeKind, CheckedStateKind, FlowMode, FlowType, OwnerAbiDeclarationKey,
    OwnerAbiDeclarationKind, OwnerAbiMemberRef, OwnerBlockBinding, OwnerCallContextRow,
    OwnerCallEntry, OwnerCallId, OwnerCallResultPathRow, OwnerCallRow, OwnerCallableContextRow,
    OwnerCallableRow, OwnerCheckedDomainCount, OwnerCheckedReceiptSet, OwnerCheckedRowDomain,
    OwnerContextBinding, OwnerContextFormalId, OwnerContextFormalRef, OwnerContextFormalRow,
    OwnerContextTypeSubstitution, OwnerDeclarationId, OwnerDeclarationRef, OwnerDeclarationRow,
    OwnerDeclarationStableKey, OwnerEvaluationScope, OwnerExpressionId, OwnerExpressionKind,
    OwnerExpressionRef, OwnerExpressionRow, OwnerInterfaceMemberRef,
    OwnerLexicalDeclarationCapability, OwnerLexicalTargetRef, OwnerListId, OwnerListRow,
    OwnerOccurrenceRow, OwnerParameterRow, OwnerPatternBindingRow, OwnerRecordField,
    OwnerRelocationTarget, OwnerResourceBinding, OwnerResourceProjectionSeedRow, OwnerScopeId,
    OwnerScopeRef, OwnerScopeRow, OwnerScopeStableKey, OwnerSemanticPath, OwnerSourceId,
    OwnerSourceRow, OwnerSourceSite, OwnerSourceStableKey, OwnerStateId, OwnerStateRow,
    OwnerStatementChild, OwnerStatementId, OwnerStatementKind, OwnerStatementRow,
    OwnerStatementScopeRole, OwnerTextSegment, OwnerTypeSubstitution, ProgramRole,
    SemanticOccurrenceKind, Type, Variant,
};
use boon_data::{Bits, ExactNumber};
use boon_syntax::{
    AstBlockBindingDeclaration, AstDrainPath, AstExprKind, AstMatchPattern, AstStatementKind,
    AstTextSegment, BytesSizeSyntax, StableCheckOwnerKey, StableExpressionKey,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const CHECKED_OWNER_SHARD_DOMAIN_V7: &[u8] = b"boon.checked-owner-shard.v7\0";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckedOwnerShardBasis {
    pub owner: StableCheckOwnerKey,
    pub syntax_fingerprint_v1: [u8; 32],
    pub lexical_plan_fingerprint_v1: [u8; 32],
    pub signature_lexical_plan_fingerprint_v1: [u8; 32],
    pub seed_fingerprint_v1: [u8; 32],
    pub summary_fingerprint_v1: [u8; 32],
    pub body_fingerprint_v1: [u8; 32],
    pub body_currentness_fingerprint_v1: [u8; 32],
    pub own_interface_scc_fingerprint_v1: [u8; 32],
    pub construction_abi_fingerprint_v1: [u8; 32],
}

/// Complete span-free checked result for one stable authored owner.
///
/// Diagnostics retain owner source-anchor templates and are materialized
/// against the independently current `OwnerSourceMap`.  `receipts` are
/// construction-owned proof material. The compatibility linker validates this
/// seal before reading the immutable rows for dense projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedOwnerShard {
    basis: CheckedOwnerShardBasis,
    rows: CheckedOwnerRows,
    diagnostics: Box<[crate::OwnerDiagnosticTemplate]>,
    receipts: OwnerCheckedReceiptSet,
    fingerprint_v1: [u8; 32],
}

impl CheckedOwnerShard {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.basis.owner
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub(crate) const fn rows(&self) -> &CheckedOwnerRows {
        &self.rows
    }

    pub(crate) fn validate_seal(
        &self,
        construction_abi: &OwnerConstructionAbiEnvironment,
    ) -> Result<(), CheckedOwnerBuildError> {
        if construction_abi.owner() != self.owner()
            || construction_abi.fingerprint_v1() != self.basis.construction_abi_fingerprint_v1
        {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} does not match its exact construction ABI",
                self.owner()
            )));
        }
        let counts = [
            (OwnerCheckedRowDomain::Scope, self.rows.scopes.len()),
            (
                OwnerCheckedRowDomain::Declaration,
                self.rows.declarations.len(),
            ),
            (OwnerCheckedRowDomain::Statement, self.rows.statements.len()),
            (
                OwnerCheckedRowDomain::Expression,
                self.rows.expressions.len(),
            ),
            (OwnerCheckedRowDomain::Callable, self.rows.callables.len()),
            (
                OwnerCheckedRowDomain::ContextFormal,
                self.rows.context_formals.len(),
            ),
            (OwnerCheckedRowDomain::Call, self.rows.calls.len()),
            (
                OwnerCheckedRowDomain::CallResultPath,
                self.rows.call_result_paths.len(),
            ),
            (
                OwnerCheckedRowDomain::PatternBinding,
                self.rows.pattern_bindings.len(),
            ),
            (
                OwnerCheckedRowDomain::ResourceProjection,
                self.rows.resource_projection_seeds.len(),
            ),
            (OwnerCheckedRowDomain::Source, self.rows.sources.len()),
            (OwnerCheckedRowDomain::State, self.rows.states.len()),
            (OwnerCheckedRowDomain::List, self.rows.lists.len()),
            (
                OwnerCheckedRowDomain::Occurrence,
                self.rows.occurrences.len(),
            ),
            (OwnerCheckedRowDomain::Diagnostic, self.diagnostics.len()),
        ]
        .into_iter()
        .filter(|(_, rows)| *rows != 0)
        .map(|(domain, rows)| {
            Ok(OwnerCheckedDomainCount {
                domain,
                rows: checked_u32(rows, "sealed owner domain row count")?,
            })
        })
        .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        if self.receipts.construction.domain_counts.as_ref() != counts.as_slice() {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} has inconsistent construction receipt counts",
                self.owner()
            )));
        }
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            CHECKED_OWNER_SHARD_DOMAIN_V7,
            &(&self.basis, &self.receipts.construction),
        )
        .map_err(|error| {
            CheckedOwnerBuildError::new(format!(
                "cannot validate checked owner {:?} seal: {error}",
                self.owner()
            ))
        })?;
        if fingerprint_v1 != self.fingerprint_v1 {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} has a stale construction seal",
                self.owner()
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedOwnerBuildError {
    message: String,
}

impl CheckedOwnerBuildError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CheckedOwnerBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CheckedOwnerBuildError {}

impl From<crate::OwnerCheckedReceiptError> for CheckedOwnerBuildError {
    fn from(error: crate::OwnerCheckedReceiptError) -> Self {
        Self::new(error.to_string())
    }
}

fn validate_inputs(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &crate::OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    body: &OwnerBodyInferenceShard,
    body_currentness: &OwnerBodyInferenceCurrentnessReceipt,
    inference_abi: &OwnerInferenceAbiEnvironment,
    construction_abi: &OwnerConstructionAbiEnvironment,
    own_scc: &OwnerInterfaceSccResult,
) -> Result<(), CheckedOwnerBuildError> {
    let owner = &syntax.owner;
    if !lexical_plan.matches_input(syntax)
        || &seed.owner != owner
        || &summary.owner != owner
        || body.owner() != owner
        || !own_scc.key.members.contains(owner)
    {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner inputs disagree on stable owner {owner:?}"
        )));
    }
    if seed.lexical_reads_fingerprint_v1() != lexical_plan.reads_fingerprint_v1() {
        return Err(CheckedOwnerBuildError::new(
            "checked owner seed and lexical plan have different projections",
        ));
    }
    if !body.signature_lexical_plan.matches_base(lexical_plan)
        || !summary.matches_signature_plan(&body.signature_lexical_plan)
        || body.receipt.signature_lexical_plan_fingerprint_v1
            != body.signature_lexical_plan.fingerprint_v1()
    {
        return Err(CheckedOwnerBuildError::new(
            "checked owner body has a stale signature lexical plan",
        ));
    }
    if inference_abi.subjects() != std::slice::from_ref(owner) {
        return Err(CheckedOwnerBuildError::new(
            "checked owner inference ABI does not match its exact owner",
        ));
    }
    if construction_abi.owner() != owner {
        return Err(CheckedOwnerBuildError::new(
            "checked owner construction ABI does not match its exact owner",
        ));
    }
    let expected_callable_names = summary.authoritative_abi_names().into_vec();
    let actual_callable_names = construction_abi
        .callable_lookups()
        .iter()
        .map(|lookup| lookup.canonical_name().to_owned())
        .collect::<Vec<_>>();
    if actual_callable_names != expected_callable_names {
        return Err(CheckedOwnerBuildError::new(
            "checked owner construction ABI does not match its exact callable lookup set",
        ));
    }
    let expected_value_paths = summary.authoritative_value_abi_paths().into_vec();
    let actual_value_paths = construction_abi
        .value_lookups()
        .iter()
        .map(|lookup| lookup.canonical_path().to_owned())
        .collect::<Vec<_>>();
    if actual_value_paths != expected_value_paths {
        return Err(CheckedOwnerBuildError::new(
            "checked owner construction ABI does not match its exact value lookup set",
        ));
    }
    let body_basis = body_currentness.basis();
    if body_currentness.result_fingerprint_v1() != body.fingerprint_v1()
        || body_basis.owner != *owner
        || seed.fingerprint_v1() != body_basis.seed_fingerprint_v1
        || summary.fingerprint_v1() != body_basis.summary_fingerprint_v1
        || syntax.fingerprint_v1() != body_basis.syntax_fingerprint_v1
        || lexical_plan.fingerprint_v1() != body_basis.lexical_plan_fingerprint_v1
        || body.signature_lexical_plan.fingerprint_v1()
            != body_basis.signature_lexical_plan_fingerprint_v1
        || inference_abi.fingerprint_v1() != body_basis.inference_abi_fingerprint_v1
        || own_scc.fingerprint_v1() != body_basis.own_scc.result_fingerprint_v1
    {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner inputs for {owner:?} do not match the frozen body basis"
        )));
    }
    if body.statements.len() != syntax.statements.len()
        || body.expressions.len() != syntax.expressions.len()
    {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner {owner:?} body tables do not cover its syntax tables"
        )));
    }
    for (index, expression) in body.expressions.iter().enumerate() {
        if expression.id.0 as usize != index
            || syntax.expressions[index].stable_key != expression.stable_key
        {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {owner:?} body expression table diverges at row {index}"
            )));
        }
    }
    for call in &body.calls {
        if matches!(call.target, InferredOwnerCallableTarget::Owner { ref owner } if own_scc.owner(owner).is_none())
            && !body_currentness.interface_imports().iter().any(|import| {
                matches!(call.target, InferredOwnerCallableTarget::Owner { owner: ref target } if &import.owner == target)
            })
        {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {owner:?} call `{}` has no frozen target interface",
                call.function
            )));
        }
    }
    Ok(())
}

fn validated_frozen_interfaces<'a>(
    body: &OwnerBodyInferenceShard,
    body_currentness: &OwnerBodyInferenceCurrentnessReceipt,
    own_scc: &'a OwnerInterfaceSccResult,
    imported_sccs: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>, CheckedOwnerBuildError> {
    let body_basis = body_currentness.basis();
    let mut expected = BTreeMap::new();
    if expected
        .insert(body_basis.own_scc.key.clone(), &body_basis.own_scc)
        .is_some()
    {
        return Err(CheckedOwnerBuildError::new(
            "checked owner body repeats its own frozen interface SCC",
        ));
    }
    for frozen in &body_basis.imports {
        if expected.insert(frozen.key.clone(), frozen).is_some() {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner body repeats frozen interface SCC {:?}",
                frozen.key
            )));
        }
    }

    if own_scc.key != body_basis.own_scc.key
        || own_scc.key_fingerprint_v1() != body_basis.own_scc.key_fingerprint_v1
    {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner {:?} received the wrong own interface SCC",
            body.owner()
        )));
    }
    let mut actual = BTreeMap::new();
    actual.insert(own_scc.key.clone(), own_scc);
    for result in imported_sccs {
        if actual.insert(result.key.clone(), result).is_some() {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} received duplicate interface SCC {:?}",
                body.owner(),
                result.key
            )));
        }
    }
    if actual.keys().collect::<Vec<_>>() != expected.keys().collect::<Vec<_>>() {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner {:?} did not receive its exact frozen interface SCC set",
            body.owner()
        )));
    }

    let mut expected_owners = BTreeSet::new();
    for (key, frozen) in &expected {
        let result = actual[key];
        if result.key_fingerprint_v1() != frozen.key_fingerprint_v1
            || result.fingerprint_v1() != frozen.result_fingerprint_v1
            || result.type_variable_count != frozen.type_variable_count
        {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} interface SCC {key:?} differs from its frozen body basis",
                body.owner()
            )));
        }
        let mut referenced = BTreeSet::new();
        for member in &frozen.referenced_members {
            let owner = key.members.get(*member as usize).ok_or_else(|| {
                CheckedOwnerBuildError::new(format!(
                    "checked owner {:?} has an out-of-range frozen interface member {member}",
                    body.owner()
                ))
            })?;
            if !referenced.insert(*member) || result.owner(owner).is_none() {
                return Err(CheckedOwnerBuildError::new(format!(
                    "checked owner {:?} has an invalid frozen interface member {owner:?}",
                    body.owner()
                )));
            }
            if !expected_owners.insert(owner.clone()) {
                return Err(CheckedOwnerBuildError::new(format!(
                    "checked owner {:?} freezes interface {owner:?} through multiple SCCs",
                    body.owner()
                )));
            }
        }
    }

    let mut imports = BTreeMap::new();
    for import in body_currentness.interface_imports() {
        if imports.insert(import.owner.clone(), import).is_some() {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} repeats interface import {:?}",
                body.owner(),
                import.owner
            )));
        }
    }
    if imports.keys().cloned().collect::<BTreeSet<_>>() != expected_owners {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner {:?} interface imports differ from its frozen SCC members",
            body.owner()
        )));
    }

    let frozen_providers = std::iter::once(&body_basis.own_scc)
        .chain(body_basis.imports.iter())
        .collect::<Vec<_>>();
    let actual_providers = frozen_providers
        .iter()
        .map(|frozen| actual[&frozen.key])
        .collect::<Vec<_>>();
    let mut interfaces = BTreeMap::new();
    for (owner, import) in imports {
        let provider = usize::try_from(import.provider_scc).map_err(|_| {
            CheckedOwnerBuildError::new(format!(
                "checked owner {:?} import {owner:?} has an invalid provider index",
                body.owner()
            ))
        })?;
        let frozen = frozen_providers.get(provider).copied().ok_or_else(|| {
            CheckedOwnerBuildError::new(format!(
                "checked owner {:?} import {owner:?} has no frozen provider index {}",
                body.owner(),
                import.provider_scc
            ))
        })?;
        let referenced = frozen
            .key
            .members
            .binary_search(&owner)
            .ok()
            .and_then(|member| u32::try_from(member).ok())
            .is_some_and(|member| frozen.referenced_members.binary_search(&member).is_ok());
        if !referenced {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} import {owner:?} selects an unrelated provider SCC",
                body.owner()
            )));
        }
        let result = actual_providers[provider];
        let interface = result.owner(&owner).ok_or_else(|| {
            CheckedOwnerBuildError::new(format!(
                "checked owner {:?} provider does not publish interface {owner:?}",
                body.owner()
            ))
        })?;
        let fingerprint = crate::owner_body::owner_body_interface_fingerprint_v1(interface)
            .map_err(|error| CheckedOwnerBuildError::new(error.to_string()))?;
        if fingerprint != import.interface_fingerprint_v1 {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} interface {owner:?} differs from the inferred body import",
                body.owner()
            )));
        }
        interfaces.insert(owner, interface);
    }
    Ok(interfaces)
}

#[derive(Clone)]
struct ScopeSpec {
    stable_key: OwnerScopeStableKey,
    parent: Option<OwnerScopeRef>,
    owner: Option<OwnerDeclarationRef>,
    kind: CheckedScopeKind,
    source: Option<OwnerSourceSite>,
}

#[derive(Clone)]
struct DeclarationSpec {
    stable_key: OwnerDeclarationStableKey,
    scope: OwnerScopeRef,
    name: String,
    kind: CheckedDeclarationKind,
    flow_type: FlowType,
    value: Option<OwnerExpressionRef>,
    body_scope: Option<OwnerScopeId>,
    source: OwnerSourceSite,
}

#[derive(Clone)]
struct PreparedCallParameter {
    formal: OwnerDeclarationRef,
    name: String,
    kind: CheckedParameterKind,
    ordinal: u32,
    flow_type: FlowType,
    output_evaluation_ordinal: Option<u32>,
}

#[derive(Clone)]
struct PreparedCallContext {
    name: String,
    kind: CheckedCallContextKind,
    provider_ordinal: u32,
    flow_type: FlowType,
}

#[derive(Clone)]
struct PreparedCallTarget {
    callable: OwnerDeclarationRef,
    intrinsic: Option<CheckedIntrinsicV1>,
    parameters: Vec<PreparedCallParameter>,
    contexts: Vec<PreparedCallContext>,
    context_formal: Option<OwnerContextFormalRef>,
    requires_pass: bool,
    role: ProgramRole,
}

#[derive(Default)]
struct DerivedOwnerRows {
    call_result_paths: Vec<OwnerCallResultPathRow>,
    resource_projection_seeds: Vec<OwnerResourceProjectionSeedRow>,
    sources: Vec<OwnerSourceRow>,
    states: Vec<OwnerStateRow>,
    lists: Vec<OwnerListRow>,
    occurrences: Vec<OwnerOccurrenceRow>,
}

struct OwnerRowConstruction<'a> {
    syntax: &'a OwnerSyntaxInput,
    lexical_plan: &'a OwnerLexicalPlan,
    seed: &'a OwnerConstraintSeed,
    summary: &'a OwnerConstraintSummary,
    body: &'a OwnerBodyInferenceShard,
    own_interface: &'a OwnerPublicInterface,
    abi: &'a OwnerConstructionAbiEnvironment,
    graph: &'a OwnerSyntaxGraph,
    imported_interfaces: BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    containing_scope: OwnerScopeRef,
    scope_ids: BTreeMap<OwnerScopeStableKey, OwnerScopeId>,
    scope_specs: Vec<Option<ScopeSpec>>,
    declaration_ids: BTreeMap<OwnerDeclarationStableKey, OwnerDeclarationId>,
    declaration_specs: Vec<Option<DeclarationSpec>>,
    statement_declarations: BTreeMap<OwnerStatementId, OwnerDeclarationId>,
    parameter_declarations: BTreeMap<u32, OwnerDeclarationId>,
    record_expression_scopes: BTreeMap<OwnerExpressionId, OwnerScopeId>,
    caller_scoped_pass_records: BTreeSet<OwnerExpressionId>,
    record_field_declarations: BTreeMap<(OwnerExpressionId, u32), OwnerDeclarationId>,
    statement_by_expression: BTreeMap<u32, OwnerStatementId>,
    statement_scopes: Vec<OwnerScopeRef>,
    statement_body_scopes: BTreeMap<OwnerStatementId, OwnerScopeId>,
    expression_scopes: Vec<OwnerScopeRef>,
    expression_owned: Vec<bool>,
    expression_declarations: Vec<Option<OwnerDeclarationRef>>,
    pattern_declarations: BTreeMap<(OwnerExpressionId, String), OwnerDeclarationId>,
    pattern_bindings: Vec<OwnerPatternBindingRow>,
    call_ids: BTreeMap<StableExpressionKey, OwnerCallId>,
    call_rows: Vec<OwnerCallRow>,
    call_occurrences: Vec<OwnerOccurrenceRow>,
    diagnostics: Vec<crate::OwnerDiagnosticTemplate>,
}

impl<'a> OwnerRowConstruction<'a> {
    fn new(
        syntax: &'a OwnerSyntaxInput,
        lexical_plan: &'a crate::OwnerLexicalPlan,
        seed: &'a OwnerConstraintSeed,
        summary: &'a OwnerConstraintSummary,
        body: &'a OwnerBodyInferenceShard,
        own_interface: &'a OwnerPublicInterface,
        abi: &'a OwnerConstructionAbiEnvironment,
        imported_interfaces: BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    ) -> Result<Self, CheckedOwnerBuildError> {
        if !lexical_plan.matches_input(syntax) {
            return Err(CheckedOwnerBuildError::new(
                "checked owner construction received a stale lexical plan",
            ));
        }
        let graph = lexical_plan.graph();
        let mut statement_by_expression = BTreeMap::new();
        for (index, statement) in syntax.statements.iter().enumerate() {
            let Some(expression) = statement.expression else {
                continue;
            };
            let statement = OwnerStatementId(checked_u32(index, "owner statement index")?);
            if statement_by_expression
                .insert(expression, statement)
                .is_some()
            {
                return Err(CheckedOwnerBuildError::new(
                    "checked owner statements repeat one direct expression",
                ));
            }
        }
        let containing_scope = body
            .signature_lexical_plan
            .inherited_environment()
            .and_then(|environment| environment.boundary_scope.as_ref())
            .map(|scope| OwnerScopeRef::Imported {
                owner: scope.owner.clone(),
                scope: scope.scope.clone(),
            })
            .unwrap_or_else(|| match &syntax.containing_scope {
                OwnerContainingScopeInput::ProjectRoot => OwnerScopeRef::ProjectRoot,
                OwnerContainingScopeInput::OwnerStatement { owner, statement } => {
                    OwnerScopeRef::Imported {
                        owner: owner.clone(),
                        scope: OwnerScopeStableKey::Statement {
                            statement: statement.clone(),
                            role: OwnerStatementScopeRole::Body,
                        },
                    }
                }
            });
        let mut construction = Self {
            syntax,
            lexical_plan,
            seed,
            summary,
            body,
            own_interface,
            abi,
            graph,
            imported_interfaces,
            containing_scope: containing_scope.clone(),
            scope_ids: BTreeMap::new(),
            scope_specs: Vec::new(),
            declaration_ids: BTreeMap::new(),
            declaration_specs: Vec::new(),
            statement_declarations: BTreeMap::new(),
            parameter_declarations: BTreeMap::new(),
            record_expression_scopes: BTreeMap::new(),
            caller_scoped_pass_records:
                crate::owner_constraints::caller_scoped_pass_record_expressions(syntax)
                    .into_iter()
                    .map(OwnerExpressionId)
                    .collect(),
            record_field_declarations: BTreeMap::new(),
            statement_by_expression,
            statement_scopes: vec![containing_scope; syntax.statements.len()],
            statement_body_scopes: BTreeMap::new(),
            expression_scopes: vec![OwnerScopeRef::ProjectRoot; syntax.expressions.len()],
            expression_owned: vec![false; syntax.expressions.len()],
            expression_declarations: vec![None; syntax.expressions.len()],
            pattern_declarations: BTreeMap::new(),
            pattern_bindings: Vec::new(),
            call_ids: BTreeMap::new(),
            call_rows: Vec::new(),
            call_occurrences: Vec::new(),
            diagnostics: body.diagnostics.to_vec(),
        };
        construction.reserve_authored_declarations()?;
        construction.reserve_lexical_scopes()?;
        construction.define_authored_declarations()?;
        construction.assign_expression_ownership()?;
        construction.prepare_pattern_bindings()?;
        construction.prepare_calls()?;
        construction.validate_signature_lexical_plan()?;
        Ok(construction)
    }

    fn reserve_scope(
        &mut self,
        stable_key: OwnerScopeStableKey,
    ) -> Result<OwnerScopeId, CheckedOwnerBuildError> {
        if self.scope_ids.contains_key(&stable_key) {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} reserves duplicate scope {stable_key:?}",
                self.syntax.owner
            )));
        }
        let id = OwnerScopeId(checked_u32(self.scope_specs.len(), "owner scope id")?);
        self.scope_ids.insert(stable_key, id);
        self.scope_specs.push(None);
        Ok(id)
    }

    fn reserve_declaration(
        &mut self,
        stable_key: OwnerDeclarationStableKey,
    ) -> Result<OwnerDeclarationId, CheckedOwnerBuildError> {
        if self.declaration_ids.contains_key(&stable_key) {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} reserves duplicate declaration {stable_key:?}",
                self.syntax.owner
            )));
        }
        let id = OwnerDeclarationId(checked_u32(
            self.declaration_specs.len(),
            "owner declaration id",
        )?);
        self.declaration_ids.insert(stable_key, id);
        self.declaration_specs.push(None);
        Ok(id)
    }

    fn define_scope(
        &mut self,
        id: OwnerScopeId,
        spec: ScopeSpec,
    ) -> Result<(), CheckedOwnerBuildError> {
        let slot = self
            .scope_specs
            .get_mut(id.0 as usize)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner scope reservation is missing"))?;
        if slot.replace(spec).is_some() {
            return Err(CheckedOwnerBuildError::new(
                "owner scope reservation was defined twice",
            ));
        }
        Ok(())
    }

    fn define_declaration(
        &mut self,
        id: OwnerDeclarationId,
        spec: DeclarationSpec,
    ) -> Result<(), CheckedOwnerBuildError> {
        let slot = self
            .declaration_specs
            .get_mut(id.0 as usize)
            .ok_or_else(|| {
                CheckedOwnerBuildError::new("owner declaration reservation is missing")
            })?;
        if slot.replace(spec).is_some() {
            return Err(CheckedOwnerBuildError::new(
                "owner declaration reservation was defined twice",
            ));
        }
        Ok(())
    }

    fn reserve_authored_declarations(&mut self) -> Result<(), CheckedOwnerBuildError> {
        for declaration in &self.seed.declarations {
            let statement = OwnerStatementId(declaration.statement);
            let Some(statement_input) = self.syntax.statements.get(statement.0 as usize) else {
                return Err(CheckedOwnerBuildError::new(format!(
                    "checked owner {:?} declaration references missing statement {}",
                    self.syntax.owner, statement.0
                )));
            };
            let owns_fieldless_hold = matches!(
                hold_alias_declaration_target(self.syntax, statement.0)
                    .map_err(|error| CheckedOwnerBuildError::new(error.to_string()))?,
                Some(OwnerLexicalDeclarationTarget::Statement {
                    statement: target,
                }) if target == statement.0
            );
            if declaration_name(&statement_input.kind).is_none() && !owns_fieldless_hold {
                continue;
            }
            let stable_key = if declaration.public {
                OwnerDeclarationStableKey::Public
            } else {
                OwnerDeclarationStableKey::Statement {
                    statement: statement_input.stable_key.clone(),
                }
            };
            let id = self.reserve_declaration(stable_key)?;
            self.statement_declarations.insert(statement, id);
        }
        if self.own_interface.declaration_kind == Some(OwnerDeclarationKind::Function) {
            for parameter in &self.own_interface.parameters {
                let id = self.reserve_declaration(OwnerDeclarationStableKey::Parameter {
                    ordinal: parameter.ordinal,
                })?;
                self.parameter_declarations.insert(parameter.ordinal, id);
            }
        }
        // Every explicit record field is a whole-scope declaration. Reserve it
        // independently of whether another expression currently reads it so
        // edits cannot re-anchor state, list, or source identity.
        for field in self.lexical_plan.record_fields() {
            let expression = self
                .syntax
                .expressions
                .get(field.object as usize)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner lexical record field references a missing expression",
                    )
                })?;
            let id = self.reserve_declaration(OwnerDeclarationStableKey::RecordField {
                object: expression.stable_key.clone(),
                ordinal: field.ordinal,
                name: field.name.clone(),
            })?;
            self.record_field_declarations
                .insert((OwnerExpressionId(field.object), field.ordinal), id);
        }
        Ok(())
    }

    fn reserve_lexical_scopes(&mut self) -> Result<(), CheckedOwnerBuildError> {
        for statement_index in 0..self.syntax.statements.len() {
            let statement_id =
                OwnerStatementId(checked_u32(statement_index, "owner statement id")?);
            let statement = &self.syntax.statements[statement_index];
            let parent_scope = statement.parent.map_or_else(
                || self.containing_scope.clone(),
                |parent| {
                    let parent = OwnerStatementId(parent);
                    self.statement_body_scopes
                        .get(&parent)
                        .copied()
                        .map(local_scope_ref)
                        .unwrap_or_else(|| self.statement_scopes[parent.0 as usize].clone())
                },
            );
            self.statement_scopes[statement_index] = parent_scope.clone();
            let graph_statement = self.graph.statement(statement_id).ok_or_else(|| {
                CheckedOwnerBuildError::new("owner syntax graph lost a statement")
            })?;
            let needs_body = matches!(statement.kind, AstStatementKind::Function { .. })
                || !graph_statement.children.is_empty();
            if !needs_body {
                continue;
            }
            let stable_key = OwnerScopeStableKey::Statement {
                statement: statement.stable_key.clone(),
                role: OwnerStatementScopeRole::Body,
            };
            let scope = self.reserve_scope(stable_key.clone())?;
            self.statement_body_scopes.insert(statement_id, scope);
            let owner = self
                .statement_declarations
                .get(&statement_id)
                .copied()
                .map(local_declaration_ref);
            let kind = if matches!(statement.kind, AstStatementKind::Function { .. }) {
                CheckedScopeKind::Function
            } else if statement_body_container(self.syntax, statement)
                .is_some_and(|(_, expression)| matches!(expression.kind, AstExprKind::Object(_)))
            {
                CheckedScopeKind::Record
            } else {
                CheckedScopeKind::Block
            };
            self.define_scope(
                scope,
                ScopeSpec {
                    stable_key,
                    parent: Some(parent_scope),
                    owner,
                    kind,
                    source: Some(statement_source(statement)),
                },
            )?;
        }

        let record_scope_expressions = self
            .lexical_plan
            .record_scopes()
            .iter()
            .map(|(expression, _)| *expression)
            .collect::<Vec<_>>();
        for expression in record_scope_expressions {
            let expression_id = OwnerExpressionId(expression);
            let expression = self
                .syntax
                .expressions
                .get(expression as usize)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner lexical record scope references a missing expression",
                    )
                })?;
            let stable_key = OwnerScopeStableKey::Expression {
                expression: expression.stable_key.clone(),
                role: boon_checked::OwnerExpressionScopeRole::Record,
            };
            let scope = self.reserve_scope(stable_key)?;
            self.record_expression_scopes.insert(expression_id, scope);
        }

        let root_statement = self.syntax.statements.first();
        for parameter in &self.own_interface.parameters {
            if parameter.kind != OwnerParameterKind::Out {
                continue;
            }
            let statement = root_statement.ok_or_else(|| {
                CheckedOwnerBuildError::new("callable owner has no root statement")
            })?;
            let stable_key = OwnerScopeStableKey::Statement {
                statement: statement.stable_key.clone(),
                role: OwnerStatementScopeRole::RepeatedOutput {
                    parameter_ordinal: parameter.ordinal,
                },
            };
            let scope = self.reserve_scope(stable_key.clone())?;
            let declaration = self.parameter_declarations[&parameter.ordinal];
            let function_scope = self
                .statement_body_scopes
                .get(&OwnerStatementId(statement.id))
                .copied()
                .ok_or_else(|| CheckedOwnerBuildError::new("function body scope is missing"))?;
            self.define_scope(
                scope,
                ScopeSpec {
                    stable_key,
                    parent: Some(local_scope_ref(function_scope)),
                    owner: Some(local_declaration_ref(declaration)),
                    kind: CheckedScopeKind::RepeatedOutput,
                    source: Some(OwnerSourceSite::FunctionParameter {
                        statement: statement.stable_key.clone(),
                        ordinal: parameter.ordinal,
                    }),
                },
            )?;
        }
        Ok(())
    }

    fn define_authored_declarations(&mut self) -> Result<(), CheckedOwnerBuildError> {
        for declaration in &self.seed.declarations {
            let statement_id = OwnerStatementId(declaration.statement);
            let Some(id) = self.statement_declarations.get(&statement_id).copied() else {
                continue;
            };
            let statement = &self.syntax.statements[statement_id.0 as usize];
            let stable_key = if declaration.public {
                OwnerDeclarationStableKey::Public
            } else {
                OwnerDeclarationStableKey::Statement {
                    statement: statement.stable_key.clone(),
                }
            };
            let value = self
                .graph
                .statement(statement_id)
                .and_then(|statement| statement.canonical_value.clone());
            let declaration_value = (declaration.kind != OwnerDeclarationKind::Function)
                .then(|| value.clone())
                .flatten();
            let flow_type = if declaration.public {
                public_declaration_flow_type(self.own_interface)
            } else {
                value
                    .as_ref()
                    .and_then(|value| self.expression_flow_type(value))
                    .unwrap_or_else(unknown_flow_type)
            };
            let flow_type = if declaration.kind == OwnerDeclarationKind::Function {
                flow_type
            } else {
                value
                    .as_ref()
                    .map(|value| self.finalize_declaration_flow_type(value, flow_type.clone()))
                    .unwrap_or(flow_type)
            };
            self.define_declaration(
                id,
                DeclarationSpec {
                    stable_key,
                    scope: self.statement_scopes[statement_id.0 as usize].clone(),
                    name: materialized_declaration_name(&statement.kind)
                        .expect("reserved declarations have a lexical name")
                        .to_owned(),
                    kind: checked_declaration_kind(declaration.kind),
                    flow_type,
                    value: declaration_value,
                    body_scope: self.statement_body_scopes.get(&statement_id).copied(),
                    source: statement_source(statement),
                },
            )?;
        }

        if self.own_interface.declaration_kind == Some(OwnerDeclarationKind::Function) {
            let statement = self.syntax.statements.first().ok_or_else(|| {
                CheckedOwnerBuildError::new("callable owner has no root statement")
            })?;
            let function_scope = self
                .statement_body_scopes
                .get(&OwnerStatementId(statement.id))
                .copied()
                .ok_or_else(|| CheckedOwnerBuildError::new("function body scope is missing"))?;
            for parameter in &self.own_interface.parameters {
                let id = self.parameter_declarations[&parameter.ordinal];
                let repeated_output = (parameter.kind == OwnerParameterKind::Out).then(|| {
                    self.scope_ids[&OwnerScopeStableKey::Statement {
                        statement: statement.stable_key.clone(),
                        role: OwnerStatementScopeRole::RepeatedOutput {
                            parameter_ordinal: parameter.ordinal,
                        },
                    }]
                });
                self.define_declaration(
                    id,
                    DeclarationSpec {
                        stable_key: OwnerDeclarationStableKey::Parameter {
                            ordinal: parameter.ordinal,
                        },
                        scope: local_scope_ref(function_scope),
                        name: parameter.name.clone(),
                        kind: match parameter.kind {
                            OwnerParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                            OwnerParameterKind::Out => CheckedDeclarationKind::OutParameter,
                        },
                        flow_type: parameter.flow_type.clone(),
                        value: None,
                        body_scope: repeated_output,
                        source: OwnerSourceSite::FunctionParameter {
                            statement: statement.stable_key.clone(),
                            ordinal: parameter.ordinal,
                        },
                    },
                )?;
            }
        }
        for field in self.lexical_plan.record_fields() {
            let object_id = OwnerExpressionId(field.object);
            let Some(id) = self
                .record_field_declarations
                .get(&(object_id, field.ordinal))
                .copied()
            else {
                continue;
            };
            let object = &self.syntax.expressions[field.object as usize];
            let fields = match &object.kind {
                AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => fields,
                _ => {
                    return Err(CheckedOwnerBuildError::new(
                        "owner lexical record field belongs to a non-record expression",
                    ));
                }
            };
            let value = fields
                .get(field.ordinal as usize)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new("owner lexical record field ordinal is missing")
                })?
                .value;
            let value = owner_expression_ref(self.syntax, value)?;
            let flow_type = self
                .expression_flow_type(&value)
                .unwrap_or_else(unknown_flow_type);
            let flow_type = self.finalize_declaration_flow_type(&value, flow_type);
            let lexical_scope = self
                .lexical_plan
                .scopes()
                .get(field.scope as usize)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner lexical record field references a missing scope",
                    )
                })?;
            let scope = match lexical_scope.origin {
                OwnerLexicalScopeOrigin::Root => self.containing_scope.clone(),
                OwnerLexicalScopeOrigin::StatementBody { statement } => self
                    .statement_body_scopes
                    .get(&OwnerStatementId(statement))
                    .copied()
                    .map(local_scope_ref)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(
                            "owner lexical record field statement scope is missing",
                        )
                    })?,
                OwnerLexicalScopeOrigin::Record { expression } => self
                    .record_expression_scopes
                    .get(&OwnerExpressionId(expression))
                    .copied()
                    .map(local_scope_ref)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(
                            "owner lexical record field expression scope is missing",
                        )
                    })?,
                OwnerLexicalScopeOrigin::PatternArm { .. } => {
                    return Err(CheckedOwnerBuildError::new(
                        "owner lexical record field cannot be owned directly by a pattern scope",
                    ));
                }
            };
            let body_scope = match &value {
                OwnerExpressionRef::Local { expression } => {
                    self.record_expression_scopes.get(expression).copied()
                }
                OwnerExpressionRef::Child { .. } => None,
            };
            self.define_declaration(
                id,
                DeclarationSpec {
                    stable_key: OwnerDeclarationStableKey::RecordField {
                        object: object.stable_key.clone(),
                        ordinal: field.ordinal,
                        name: field.name.clone(),
                    },
                    scope,
                    name: field.name.clone(),
                    kind: CheckedDeclarationKind::Field,
                    flow_type,
                    value: Some(value),
                    body_scope,
                    source: OwnerSourceSite::RecordField {
                        expression: object.stable_key.clone(),
                        ordinal: field.ordinal,
                    },
                },
            )?;
        }
        Ok(())
    }

    fn expression_flow_type(&self, reference: &OwnerExpressionRef) -> Option<FlowType> {
        match reference {
            OwnerExpressionRef::Local { expression } => self
                .body
                .expressions
                .get(expression.0 as usize)
                .map(|expression| expression.flow_type.clone()),
            OwnerExpressionRef::Child { owner, .. } => self
                .imported_interfaces
                .get(owner)
                .map(|interface| interface.result.clone()),
        }
    }

    fn finalize_declaration_flow_type(
        &self,
        reference: &OwnerExpressionRef,
        mut flow_type: FlowType,
    ) -> FlowType {
        let OwnerExpressionRef::Local { expression } = reference else {
            return flow_type;
        };
        let Some(flush_type) = self
            .body
            .expressions
            .get(expression.0 as usize)
            .and_then(|expression| expression.flush_type.as_ref())
        else {
            return flow_type;
        };
        flow_type.ty = crate::union_structural_type(&flow_type.ty, flush_type);
        if flow_type.mode == FlowMode::Absent {
            flow_type.mode = FlowMode::Continuous;
        }
        flow_type
    }

    fn lexical_declaration_for_scope(&self, scope: &OwnerScopeRef) -> Option<OwnerDeclarationRef> {
        match scope {
            OwnerScopeRef::Local { scope } => {
                let scope = self
                    .scope_specs
                    .get(scope.0 as usize)
                    .and_then(Option::as_ref)?;
                scope.owner.clone().or_else(|| {
                    scope
                        .parent
                        .as_ref()
                        .and_then(|parent| self.lexical_declaration_for_scope(parent))
                })
            }
            OwnerScopeRef::Imported { .. } => Some(OwnerDeclarationRef::ScopeOwner {
                scope: scope.clone(),
            }),
            OwnerScopeRef::ProjectRoot => None,
        }
    }

    fn assign_expression_ownership(&mut self) -> Result<(), CheckedOwnerBuildError> {
        let mut assigned = vec![false; self.syntax.expressions.len()];
        for statement_index in 0..self.syntax.statements.len() {
            let statement_id =
                OwnerStatementId(checked_u32(statement_index, "owner statement id")?);
            let statement = &self.syntax.statements[statement_index];
            let scope = self.statement_scopes[statement_index].clone();
            let declaration = self
                .statement_declarations
                .get(&statement_id)
                .copied()
                .map(local_declaration_ref)
                .or_else(|| self.lexical_declaration_for_scope(&scope));
            if let Some(expression) = statement.expression {
                let owns_declaration = self.statement_declarations.contains_key(&statement_id);
                self.assign_expression_tree(
                    expression,
                    scope,
                    declaration.clone(),
                    owns_declaration,
                    &mut assigned,
                )?;
            }
            if let Some((container, _)) = statement_body_container(self.syntax, statement)
                && let Some(body_scope) = self.statement_body_scopes.get(&statement_id).copied()
            {
                self.assign_expression_tree(
                    checked_u32(container, "owner body container expression")?,
                    local_scope_ref(body_scope),
                    declaration.clone(),
                    true,
                    &mut assigned,
                )?;
            }
        }
        for index in 0..self.syntax.expressions.len() {
            if !assigned[index] {
                self.expression_scopes[index] = self.containing_scope.clone();
            }
        }
        self.expression_owned = assigned;
        Ok(())
    }

    fn prepare_pattern_bindings(&mut self) -> Result<(), CheckedOwnerBuildError> {
        let lexical_scope_depths = lexical_scope_depths(self.lexical_plan.scopes())?;
        let mut arms = Vec::new();
        for (arm, expression) in self.syntax.expressions.iter().enumerate() {
            let AstExprKind::MatchArm { pattern, output } = &expression.kind else {
                continue;
            };
            let selector = expression.pattern_selector.ok_or_else(|| {
                CheckedOwnerBuildError::new(format!(
                    "owner match arm {:?} has no structural selector",
                    expression.stable_key
                ))
            })?;
            let lexical_scope =
                *self
                    .lexical_plan
                    .expression_scopes()
                    .get(arm)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(
                            "owner match arm is missing from the lexical scope projection",
                        )
                    })?;
            let depth = *lexical_scope_depths
                .get(lexical_scope as usize)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner match arm lexical scope is outside the scope projection",
                    )
                })?;
            arms.push((depth, arm, selector, pattern.clone(), *output));
        }
        arms.sort_by_key(|(depth, arm, ..)| (*depth, *arm));

        for (_, arm, selector, pattern, output) in arms {
            let arm_id = OwnerExpressionId(checked_u32(arm, "owner pattern arm")?);
            let selector_ref = owner_expression_ref(self.syntax, selector as usize)?;
            let stable_expression = self.syntax.expressions[arm].stable_key.clone();
            let stable_scope = OwnerScopeStableKey::Expression {
                expression: stable_expression.clone(),
                role: boon_checked::OwnerExpressionScopeRole::MatchArm,
            };
            let scope = self.reserve_scope(stable_scope.clone())?;
            self.define_scope(
                scope,
                ScopeSpec {
                    stable_key: stable_scope,
                    parent: Some(self.expression_scopes[arm].clone()),
                    owner: None,
                    kind: CheckedScopeKind::Block,
                    source: Some(expression_source(&self.syntax.expressions[arm])),
                },
            )?;
            self.expression_scopes[arm] = local_scope_ref(scope);

            let statement = self
                .statement_by_expression
                .get(&checked_u32(arm, "owner pattern arm expression")?)
                .copied();
            let body_scope =
                statement.and_then(|statement| self.statement_body_scopes.get(&statement).copied());
            if let Some(statement) = statement {
                self.statement_scopes[statement.0 as usize] = local_scope_ref(scope);
            }
            if let Some(body_scope) = body_scope {
                let spec = self
                    .scope_specs
                    .get_mut(body_scope.0 as usize)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new("pattern-arm body scope is missing")
                    })?;
                spec.parent = Some(local_scope_ref(scope));
            } else if let Some(output) = output
                && output < self.syntax.expressions.len()
            {
                self.rebase_expression_tree(
                    OwnerExpressionId(checked_u32(output, "owner pattern output")?),
                    local_scope_ref(scope),
                )?;
            }

            let selector_type = match &selector_ref {
                OwnerExpressionRef::Local { expression } => {
                    &self
                        .body
                        .expressions
                        .get(expression.0 as usize)
                        .ok_or_else(|| {
                            CheckedOwnerBuildError::new(
                                "owner pattern selector inference is missing",
                            )
                        })?
                        .flow_type
                        .ty
                }
                OwnerExpressionRef::Child { owner, expression } => {
                    &self
                        .own_interface
                        .captures
                        .iter()
                        .find(|capture| {
                            &capture.owner == owner && &capture.expression == expression
                        })
                        .ok_or_else(|| {
                            CheckedOwnerBuildError::new(
                                "owner pattern selector capture is missing from its interface",
                            )
                        })?
                        .flow_type
                        .ty
                }
            };
            for (ordinal, name) in pattern_variable_names(&pattern).into_iter().enumerate() {
                let ordinal = checked_u32(ordinal, "owner pattern binding ordinal")?;
                let stable_key = OwnerDeclarationStableKey::PatternBinding {
                    selector: stable_expression.clone(),
                    ordinal,
                    name: name.clone(),
                };
                let declaration = self.reserve_declaration(stable_key.clone())?;
                self.define_declaration(
                    declaration,
                    DeclarationSpec {
                        stable_key,
                        scope: local_scope_ref(scope),
                        name: name.clone(),
                        kind: CheckedDeclarationKind::PatternBinding,
                        flow_type: FlowType {
                            mode: FlowMode::Continuous,
                            ty: pattern_binding_type(selector_type, &pattern, &name),
                        },
                        value: None,
                        body_scope: None,
                        source: expression_source(&self.syntax.expressions[arm]),
                    },
                )?;
                if self
                    .pattern_declarations
                    .insert((arm_id, name.clone()), declaration)
                    .is_some()
                {
                    return Err(CheckedOwnerBuildError::new(
                        "owner pattern declares one binding name more than once",
                    ));
                }
                self.pattern_bindings.push(OwnerPatternBindingRow {
                    declaration,
                    selector: selector_ref.clone(),
                    projection: match &pattern {
                        AstMatchPattern::Tag { fields, .. } if fields.contains(&name) => {
                            vec![name]
                        }
                        _ => Vec::new(),
                    },
                });
            }
        }
        Ok(())
    }

    fn assign_expression_tree(
        &mut self,
        expression: u32,
        scope: OwnerScopeRef,
        declaration: Option<OwnerDeclarationRef>,
        override_existing: bool,
        assigned: &mut [bool],
    ) -> Result<(), CheckedOwnerBuildError> {
        let index = expression as usize;
        if index >= self.syntax.expressions.len() {
            // Child-owner expressions are linked by stable relocation and do
            // not acquire a local scope or declaration.
            return Ok(());
        }
        if assigned[index] && !override_existing {
            return Ok(());
        }
        let expression_id = OwnerExpressionId(expression);
        let inherited_scope = scope.clone();
        let scope = if let Some(record_scope) =
            self.record_expression_scopes.get(&expression_id).copied()
        {
            let record_scope_ref = local_scope_ref(record_scope);
            let stable_key = OwnerScopeStableKey::Expression {
                expression: self.syntax.expressions[index].stable_key.clone(),
                role: boon_checked::OwnerExpressionScopeRole::Record,
            };
            let slot = self
                .scope_specs
                .get_mut(record_scope.0 as usize)
                .ok_or_else(|| CheckedOwnerBuildError::new("owner record scope is missing"))?;
            if let Some(spec) = slot.as_mut() {
                if spec.parent.as_ref() != Some(&scope) && !override_existing {
                    return Err(CheckedOwnerBuildError::new(
                        "owner record expression has conflicting lexical parents",
                    ));
                }
                spec.parent = Some(scope);
            } else {
                self.define_scope(
                    record_scope,
                    ScopeSpec {
                        stable_key,
                        parent: Some(scope),
                        owner: declaration.clone(),
                        kind: CheckedScopeKind::Record,
                        source: Some(expression_source(&self.syntax.expressions[index])),
                    },
                )?;
            }
            record_scope_ref
        } else {
            scope
        };
        assigned[index] = true;
        self.expression_scopes[index] = scope.clone();
        if declaration.is_some() || override_existing {
            self.expression_declarations[index] = declaration.clone();
        }
        let record_fields = match &self.syntax.expressions[index].kind {
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. }
                if self.record_expression_scopes.contains_key(&expression_id) =>
            {
                Some(fields.clone())
            }
            _ => None,
        };
        if let Some(fields) = record_fields {
            let field_scope = if self.caller_scoped_pass_records.contains(&expression_id) {
                inherited_scope
            } else {
                scope.clone()
            };
            for (ordinal, field) in fields.into_iter().enumerate() {
                let field_declaration = if field.spread {
                    declaration.clone()
                } else {
                    let ordinal = checked_u32(ordinal, "owner record field ordinal")?;
                    self.record_field_declarations
                        .get(&(expression_id, ordinal))
                        .copied()
                        .map(local_declaration_ref)
                        .or_else(|| declaration.clone())
                };
                self.assign_expression_tree(
                    checked_u32(field.value, "owner record field value")?,
                    field_scope.clone(),
                    field_declaration,
                    true,
                    assigned,
                )?;
            }
            return Ok(());
        }
        let inputs = self
            .graph
            .expression_inputs(expression_id)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner expression graph is missing"))?
            .to_vec();
        for input in inputs {
            if let OwnerExpressionRef::Local { expression } = input {
                self.assign_expression_tree(
                    expression.0,
                    scope.clone(),
                    declaration.clone(),
                    override_existing,
                    assigned,
                )?;
            }
        }
        Ok(())
    }

    fn prepare_call_target(
        &self,
        call: &InferredOwnerCall,
    ) -> Result<Option<PreparedCallTarget>, CheckedOwnerBuildError> {
        match &call.target {
            InferredOwnerCallableTarget::Owner { owner } => {
                let interface = if owner == &self.syntax.owner {
                    self.own_interface
                } else {
                    self.imported_interfaces
                        .get(owner)
                        .copied()
                        .ok_or_else(|| {
                            CheckedOwnerBuildError::new(format!(
                                "owner call `{}` has no imported target interface",
                                call.function
                            ))
                        })?
                };
                let callable = self.owner_interface_member_ref(
                    owner,
                    OwnerInterfaceMemberRef::PublicDeclaration,
                )?;
                let parameters = interface
                    .parameters
                    .iter()
                    .map(|parameter| {
                        Ok(PreparedCallParameter {
                            formal: self.owner_interface_member_ref(
                                owner,
                                OwnerInterfaceMemberRef::Parameter {
                                    ordinal: parameter.ordinal,
                                },
                            )?,
                            name: parameter.name.clone(),
                            kind: match parameter.kind {
                                OwnerParameterKind::Value => CheckedParameterKind::Value,
                                OwnerParameterKind::Out => CheckedParameterKind::Out,
                            },
                            ordinal: parameter.ordinal,
                            flow_type: parameter.flow_type.clone(),
                            output_evaluation_ordinal: match parameter.evaluation_scope {
                                OwnerInterfaceEvaluationScope::Parent => None,
                                OwnerInterfaceEvaluationScope::Output { parameter_ordinal } => {
                                    Some(parameter_ordinal)
                                }
                            },
                        })
                    })
                    .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
                Ok(Some(PreparedCallTarget {
                    callable,
                    intrinsic: None,
                    parameters,
                    contexts: Vec::new(),
                    context_formal: interface.context.as_ref().map(|_| {
                        if owner == &self.syntax.owner {
                            OwnerContextFormalRef::Local {
                                formal: OwnerContextFormalId(0),
                            }
                        } else {
                            OwnerContextFormalRef::Imported {
                                owner: owner.clone(),
                            }
                        }
                    }),
                    requires_pass: interface.context.is_some(),
                    role: self.abi.role(),
                }))
            }
            InferredOwnerCallableTarget::Authoritative => {
                let lookup = self.abi.callable_lookup(&call.function).ok_or_else(|| {
                    CheckedOwnerBuildError::new(format!(
                        "authoritative owner call `{}` has no ABI contract",
                        call.function
                    ))
                })?;
                let contract = lookup.contract().ok_or_else(|| {
                    CheckedOwnerBuildError::new(format!(
                        "authoritative owner call `{}` has a missing ABI contract",
                        call.function
                    ))
                })?;
                let key = lookup.declaration_key().ok_or_else(|| {
                    CheckedOwnerBuildError::new(format!(
                        "authoritative owner call `{}` has no ABI declaration key",
                        call.function
                    ))
                })?;
                let callable = OwnerDeclarationRef::Abi {
                    canonical_name: call.function.clone(),
                    declaration: key,
                    member: OwnerAbiMemberRef::Declaration,
                };
                let parameters = contract
                    .parameters
                    .iter()
                    .map(|parameter| PreparedCallParameter {
                        formal: OwnerDeclarationRef::Abi {
                            canonical_name: call.function.clone(),
                            declaration: key,
                            member: OwnerAbiMemberRef::Parameter {
                                ordinal: parameter.ordinal,
                            },
                        },
                        name: parameter.name.clone(),
                        kind: parameter.kind,
                        ordinal: parameter.ordinal,
                        flow_type: parameter.flow_type.clone(),
                        output_evaluation_ordinal: match parameter.evaluation_scope {
                            OwnerAbiEvaluationScope::Parent => None,
                            OwnerAbiEvaluationScope::Output { parameter_ordinal } => {
                                Some(parameter_ordinal)
                            }
                        },
                    })
                    .collect();
                Ok(Some(PreparedCallTarget {
                    callable,
                    intrinsic: contract.intrinsic,
                    parameters,
                    contexts: contract
                        .contexts
                        .iter()
                        .map(|context| PreparedCallContext {
                            name: context.name.clone(),
                            kind: context.kind,
                            provider_ordinal: context.provider_parameter_ordinal,
                            flow_type: context.flow_type.clone(),
                        })
                        .collect(),
                    context_formal: None,
                    requires_pass: false,
                    role: contract.role,
                }))
            }
            InferredOwnerCallableTarget::Unresolved
            | InferredOwnerCallableTarget::Ambiguous { .. } => Ok(None),
        }
    }

    fn owner_interface_member_ref(
        &self,
        owner: &StableCheckOwnerKey,
        member: OwnerInterfaceMemberRef,
    ) -> Result<OwnerDeclarationRef, CheckedOwnerBuildError> {
        if owner != &self.syntax.owner {
            return Ok(OwnerDeclarationRef::Imported {
                owner: owner.clone(),
                member,
            });
        }
        let declaration = match member {
            OwnerInterfaceMemberRef::PublicDeclaration => self
                .declaration_ids
                .get(&OwnerDeclarationStableKey::Public)
                .copied(),
            OwnerInterfaceMemberRef::Parameter { ordinal } => {
                self.parameter_declarations.get(&ordinal).copied()
            }
            OwnerInterfaceMemberRef::ContextFormal => None,
        }
        .ok_or_else(|| {
            CheckedOwnerBuildError::new(format!(
                "owner {:?} has no local declaration for interface member {member:?}",
                self.syntax.owner
            ))
        })?;
        Ok(local_declaration_ref(declaration))
    }

    fn signature_calls_outer_first(
        &self,
    ) -> Result<Vec<(OwnerSignatureCallPlan, InferredOwnerCall)>, CheckedOwnerBuildError> {
        let mut inferred = BTreeMap::new();
        for call in self.body.calls.iter().cloned() {
            if inferred.insert(call.expression.clone(), call).is_some() {
                return Err(CheckedOwnerBuildError::new(
                    "owner body contains duplicate call expression identity",
                ));
            }
        }
        let mut planned = self.body.signature_lexical_plan.calls().to_vec();
        planned.sort_by_key(|call| (call.structural_ordinal, call.stable_expression.clone()));
        let mut ordered = Vec::with_capacity(planned.len());
        for (ordinal, plan) in planned.into_iter().enumerate() {
            if plan.structural_ordinal as usize != ordinal {
                return Err(CheckedOwnerBuildError::new(
                    "owner signature call structural order is not dense",
                ));
            }
            let expression = self
                .syntax
                .expressions
                .get(plan.expression as usize)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner signature call references a missing local expression",
                    )
                })?;
            if expression.stable_key != plan.stable_expression {
                return Err(CheckedOwnerBuildError::new(
                    "owner signature call expression identity is stale",
                ));
            }
            let call = inferred.remove(&plan.stable_expression).ok_or_else(|| {
                CheckedOwnerBuildError::new(format!(
                    "owner signature call `{}` is missing from body inference",
                    plan.function
                ))
            })?;
            if call.function != plan.function
                || !signature_call_target_matches(&plan.target, &call.target)
            {
                return Err(CheckedOwnerBuildError::new(format!(
                    "owner signature call `{}` diverges from body inference",
                    plan.function
                )));
            }
            ordered.push((plan, call));
        }
        if !inferred.is_empty() {
            return Err(CheckedOwnerBuildError::new(
                "owner body contains calls absent from its signature lexical plan",
            ));
        }
        Ok(ordered)
    }

    fn reserve_signature_declarations(&mut self) -> Result<(), CheckedOwnerBuildError> {
        let declarations = self
            .body
            .signature_lexical_plan
            .declarations()
            .iter()
            .map(|declaration| declaration.target.clone())
            .collect::<BTreeSet<_>>();
        let mut calls = self.body.signature_lexical_plan.calls().to_vec();
        calls.sort_by_key(|call| (call.structural_ordinal, call.stable_expression.clone()));
        let mut reserved = BTreeSet::new();
        for call in calls {
            for target in call
                .outputs
                .iter()
                .filter_map(|output| match output {
                    OwnerSignatureOutputBindingPlan::Fresh { target, .. } => Some(target),
                    OwnerSignatureOutputBindingPlan::Forward { .. } => None,
                })
                .chain(call.contexts.iter().map(|context| &context.target))
            {
                if !declarations.contains(target) || !reserved.insert(target.clone()) {
                    return Err(CheckedOwnerBuildError::new(
                        "owner signature call has a missing or duplicate dynamic declaration",
                    ));
                }
                self.reserve_declaration(signature_declaration_key(target))?;
                self.reserve_scope(signature_scope_key(target))?;
            }
        }
        if reserved != declarations {
            return Err(CheckedOwnerBuildError::new(
                "owner signature declarations are not owned by exact call plans",
            ));
        }
        Ok(())
    }

    fn authored_scope_projection(
        &self,
    ) -> Result<BTreeMap<u32, OwnerScopeRef>, CheckedOwnerBuildError> {
        let mut projection = BTreeMap::new();
        for (index, lexical_scope) in self.lexical_plan.scopes().iter().enumerate() {
            let checked_scope = match lexical_scope.origin {
                OwnerLexicalScopeOrigin::Root => self.containing_scope.clone(),
                OwnerLexicalScopeOrigin::StatementBody { statement } => self
                    .statement_body_scopes
                    .get(&OwnerStatementId(statement))
                    .copied()
                    .map(local_scope_ref)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner lexical statement-body scope references missing statement {statement}"
                        ))
                    })?,
                OwnerLexicalScopeOrigin::PatternArm { expression } => {
                    let expression = self.syntax.expressions.get(expression as usize).ok_or_else(
                        || {
                            CheckedOwnerBuildError::new(format!(
                                "owner lexical pattern scope references missing expression {expression}"
                            ))
                        },
                    )?;
                    self.scope_ids
                        .get(&OwnerScopeStableKey::Expression {
                            expression: expression.stable_key.clone(),
                            role: boon_checked::OwnerExpressionScopeRole::MatchArm,
                        })
                        .copied()
                        .map(local_scope_ref)
                        .ok_or_else(|| {
                            CheckedOwnerBuildError::new(
                                "owner lexical pattern scope has no checked projection",
                            )
                        })?
                }
                OwnerLexicalScopeOrigin::Record { expression } => self
                    .record_expression_scopes
                    .get(&OwnerExpressionId(expression))
                    .copied()
                    .map(local_scope_ref)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner lexical record scope references missing expression {expression}"
                        ))
                    })?,
            };
            projection.insert(checked_u32(index, "owner lexical scope")?, checked_scope);
        }
        Ok(projection)
    }

    fn effective_target_declaration(
        &self,
        target: &OwnerEffectiveLexicalTarget,
    ) -> Result<OwnerDeclarationRef, CheckedOwnerBuildError> {
        let declaration = match target {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => self
                .parameter_declarations
                .get(ordinal)
                .copied()
                .map(local_declaration_ref),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Statement { statement },
            } => self
                .statement_declarations
                .get(&OwnerStatementId(*statement))
                .copied()
                .map(local_declaration_ref),
            OwnerEffectiveLexicalTarget::Static {
                target:
                    OwnerLexicalDeclarationTarget::RecordField {
                        object, ordinal, ..
                    },
            } => self
                .record_field_declarations
                .get(&(OwnerExpressionId(*object), *ordinal))
                .copied()
                .map(local_declaration_ref),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::PatternBinding { arm, name },
            } => self
                .pattern_declarations
                .get(&(OwnerExpressionId(*arm), name.clone()))
                .copied()
                .map(local_declaration_ref),
            OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            } => self
                .declaration_ids
                .get(&OwnerDeclarationStableKey::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                })
                .copied()
                .map(local_declaration_ref),
            OwnerEffectiveLexicalTarget::CallContext {
                call,
                context_ordinal,
            } => self
                .declaration_ids
                .get(&OwnerDeclarationStableKey::CallContext {
                    call: call.clone(),
                    ordinal: *context_ordinal,
                })
                .copied()
                .map(local_declaration_ref),
            OwnerEffectiveLexicalTarget::Imported {
                target:
                    OwnerLexicalTargetRef::Declaration {
                        owner, declaration, ..
                    },
            } => Some(OwnerDeclarationRef::ImportedStable {
                owner: owner.clone(),
                declaration: declaration.clone(),
            }),
            OwnerEffectiveLexicalTarget::Static {
                target:
                    OwnerLexicalDeclarationTarget::Passed
                    | OwnerLexicalDeclarationTarget::Imported { .. }
                    | OwnerLexicalDeclarationTarget::Ambiguous { .. },
            }
            | OwnerEffectiveLexicalTarget::Imported { .. }
            | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
            | OwnerEffectiveLexicalTarget::Ambiguous { .. } => None,
        };
        declaration.ok_or_else(|| {
            CheckedOwnerBuildError::new(format!(
                "owner signature lexical target {target:?} has no checked declaration"
            ))
        })
    }

    fn effective_target_is_out(
        &self,
        target: &OwnerEffectiveLexicalTarget,
    ) -> Result<bool, CheckedOwnerBuildError> {
        match target {
            OwnerEffectiveLexicalTarget::Imported {
                target:
                    OwnerLexicalTargetRef::Declaration {
                        capability: OwnerLexicalDeclarationCapability::Out { .. },
                        ..
                    },
            }
            | OwnerEffectiveLexicalTarget::FreshOut { .. } => Ok(true),
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => {
                let declaration = self
                    .parameter_declarations
                    .get(ordinal)
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new("owner ForwardOut parameter target is missing")
                    })?;
                Ok(self
                    .declaration_specs
                    .get(declaration.0 as usize)
                    .and_then(Option::as_ref)
                    .is_some_and(|declaration| {
                        matches!(declaration.kind, CheckedDeclarationKind::OutParameter)
                    }))
            }
            _ => Ok(false),
        }
    }

    fn effective_evaluation_scope(
        &self,
        target: &OwnerEffectiveLexicalTarget,
    ) -> Result<OwnerScopeRef, CheckedOwnerBuildError> {
        match target {
            OwnerEffectiveLexicalTarget::Static {
                target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
            } => {
                let declaration = self
                    .parameter_declarations
                    .get(ordinal)
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner evaluation scope references missing parameter {ordinal}"
                        ))
                    })?;
                let scope = self
                    .declaration_specs
                    .get(declaration.0 as usize)
                    .and_then(Option::as_ref)
                    .and_then(|declaration| declaration.body_scope)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner evaluation scope parameter {ordinal} is not an OUT"
                        ))
                    })?;
                Ok(local_scope_ref(scope))
            }
            OwnerEffectiveLexicalTarget::FreshOut {
                call,
                formal_ordinal,
            } => self
                .scope_ids
                .get(&OwnerScopeStableKey::GeneratedOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                })
                .copied()
                .map(local_scope_ref)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner evaluation scope references an unreserved FreshOut",
                    )
                }),
            OwnerEffectiveLexicalTarget::Imported {
                target:
                    OwnerLexicalTargetRef::Declaration {
                        capability: OwnerLexicalDeclarationCapability::Out { evaluation_scope },
                        ..
                    },
            } => Ok(OwnerScopeRef::Imported {
                owner: evaluation_scope.owner.clone(),
                scope: evaluation_scope.scope.clone(),
            }),
            OwnerEffectiveLexicalTarget::Static { .. }
            | OwnerEffectiveLexicalTarget::Imported { .. }
            | OwnerEffectiveLexicalTarget::CallContext { .. }
            | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
            | OwnerEffectiveLexicalTarget::Ambiguous { .. } => Err(CheckedOwnerBuildError::new(
                format!("owner signature lexical target {target:?} cannot own evaluation"),
            )),
        }
    }

    fn signature_declaration_parent_scope(
        &self,
        declaration: &OwnerSignatureDeclarationPlan,
    ) -> Result<OwnerScopeRef, CheckedOwnerBuildError> {
        self.expression_scopes
            .get(declaration.call_expression as usize)
            .cloned()
            .ok_or_else(|| {
                CheckedOwnerBuildError::new(format!(
                    "owner signature declaration has missing call expression {}",
                    declaration.call_expression
                ))
            })
    }

    fn define_signature_fresh_out(
        &mut self,
        plan: &OwnerSignatureDeclarationPlan,
        parameter: &PreparedCallParameter,
        source: &OwnerSourceSite,
    ) -> Result<(OwnerDeclarationId, OwnerScopeId), CheckedOwnerBuildError> {
        let OwnerSignatureDeclarationKind::FreshOut { formal_ordinal } = &plan.declaration_kind
        else {
            return Err(CheckedOwnerBuildError::new(
                "owner signature FreshOut target has the wrong declaration kind",
            ));
        };
        if *formal_ordinal != parameter.ordinal || plan.name != parameter.name {
            return Err(CheckedOwnerBuildError::new(
                "owner signature FreshOut declaration diverges from its formal",
            ));
        }
        let declaration_key = signature_declaration_key(&plan.target);
        let scope_key = signature_scope_key(&plan.target);
        let declaration = self
            .declaration_ids
            .get(&declaration_key)
            .copied()
            .ok_or_else(|| {
                CheckedOwnerBuildError::new("owner signature FreshOut declaration was not reserved")
            })?;
        let scope = self.scope_ids.get(&scope_key).copied().ok_or_else(|| {
            CheckedOwnerBuildError::new("owner signature FreshOut scope was not reserved")
        })?;
        let parent = self.signature_declaration_parent_scope(plan)?;
        self.define_scope(
            scope,
            ScopeSpec {
                stable_key: scope_key,
                parent: Some(parent.clone()),
                owner: Some(local_declaration_ref(declaration)),
                kind: CheckedScopeKind::RepeatedOutput,
                source: Some(source.clone()),
            },
        )?;
        self.define_declaration(
            declaration,
            DeclarationSpec {
                stable_key: declaration_key,
                scope: parent,
                name: parameter.name.clone(),
                kind: CheckedDeclarationKind::FreshOut,
                flow_type: parameter.flow_type.clone(),
                value: None,
                body_scope: Some(scope),
                source: source.clone(),
            },
        )?;
        self.call_occurrences.push(OwnerOccurrenceRow {
            target: local_declaration_ref(declaration),
            kind: SemanticOccurrenceKind::FreshOut,
            source: source.clone(),
        });
        Ok((declaration, scope))
    }

    fn define_signature_call_context(
        &mut self,
        plan: &OwnerSignatureDeclarationPlan,
        context: &PreparedCallContext,
        source: &OwnerSourceSite,
    ) -> Result<OwnerCallContextRow, CheckedOwnerBuildError> {
        let OwnerSignatureDeclarationKind::CallContext {
            context_ordinal,
            context_kind,
            provider_parameter_ordinal,
        } = &plan.declaration_kind
        else {
            return Err(CheckedOwnerBuildError::new(
                "owner signature call-context target has the wrong declaration kind",
            ));
        };
        if plan.name != context.name
            || *context_kind != context.kind
            || *provider_parameter_ordinal != context.provider_ordinal
        {
            return Err(CheckedOwnerBuildError::new(
                "owner signature call-context declaration diverges from its ABI context",
            ));
        }
        let declaration_key = signature_declaration_key(&plan.target);
        let scope_key = signature_scope_key(&plan.target);
        let declaration = self
            .declaration_ids
            .get(&declaration_key)
            .copied()
            .ok_or_else(|| {
                CheckedOwnerBuildError::new(
                    "owner signature call-context declaration was not reserved",
                )
            })?;
        let scope = self.scope_ids.get(&scope_key).copied().ok_or_else(|| {
            CheckedOwnerBuildError::new("owner signature call-context scope was not reserved")
        })?;
        let parent = self.signature_declaration_parent_scope(plan)?;
        self.define_scope(
            scope,
            ScopeSpec {
                stable_key: scope_key,
                parent: Some(parent),
                owner: Some(local_declaration_ref(declaration)),
                kind: CheckedScopeKind::CallContext,
                source: Some(source.clone()),
            },
        )?;
        self.define_declaration(
            declaration,
            DeclarationSpec {
                stable_key: declaration_key,
                scope: local_scope_ref(scope),
                name: context.name.clone(),
                kind: match context.kind {
                    CheckedCallContextKind::ElementState => CheckedDeclarationKind::ElementState,
                },
                flow_type: context.flow_type.clone(),
                value: None,
                body_scope: None,
                source: source.clone(),
            },
        )?;
        self.call_occurrences.push(OwnerOccurrenceRow {
            target: local_declaration_ref(declaration),
            kind: SemanticOccurrenceKind::Declaration,
            source: source.clone(),
        });
        Ok(OwnerCallContextRow {
            declaration,
            context_ordinal: *context_ordinal,
            scope_id: scope,
        })
    }

    fn apply_signature_evaluation_scopes(
        &mut self,
        authored_scopes: &BTreeMap<u32, OwnerScopeRef>,
    ) -> Result<(), CheckedOwnerBuildError> {
        let planned = self
            .body
            .signature_lexical_plan
            .expression_evaluation_scopes()
            .to_vec();
        if planned.len() != self.syntax.expressions.len() {
            return Err(CheckedOwnerBuildError::new(
                "owner signature evaluation-scope projection is incomplete",
            ));
        }
        let mut direct = Vec::new();
        let mut reparented = BTreeMap::<OwnerScopeId, OwnerScopeRef>::new();
        let mut ancestry = BTreeMap::<(u32, u32), Option<u32>>::new();
        for (index, evaluation) in planned.into_iter().enumerate() {
            let Some(evaluation) = evaluation else {
                continue;
            };
            let lexical_scope = self.lexical_plan.expression_scopes()[index];
            let relation = if let Some(relation) = ancestry
                .get(&(lexical_scope, evaluation.boundary_scope))
                .copied()
            {
                relation
            } else {
                let relation = first_lexical_scope_below_or_equal(
                    self.lexical_plan.scopes(),
                    lexical_scope,
                    evaluation.boundary_scope,
                )?;
                ancestry.insert((lexical_scope, evaluation.boundary_scope), relation);
                relation
            };
            let target_scope = self.effective_evaluation_scope(&evaluation.target)?;
            let Some(first_child) = relation else {
                direct.push((index, target_scope));
                continue;
            };
            let authored = authored_scopes.get(&first_child).ok_or_else(|| {
                CheckedOwnerBuildError::new(format!(
                    "owner nested lexical scope {first_child} has no checked projection"
                ))
            })?;
            let OwnerScopeRef::Local { scope } = authored else {
                return Err(CheckedOwnerBuildError::new(
                    "owner nested lexical scope projected outside the local owner",
                ));
            };
            if let Some(previous) = reparented.insert(*scope, target_scope.clone())
                && previous != target_scope
            {
                return Err(CheckedOwnerBuildError::new(format!(
                    "owner authored scope {scope:?} has conflicting evaluation parents"
                )));
            }
        }
        for (scope, parent) in reparented {
            let spec = self
                .scope_specs
                .get_mut(scope.0 as usize)
                .and_then(Option::as_mut)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner signature evaluation scope references an undefined authored scope",
                    )
                })?;
            spec.parent = Some(parent);
        }
        for (index, scope) in direct {
            self.expression_scopes[index] = scope;
        }
        Ok(())
    }

    fn prepare_calls(&mut self) -> Result<(), CheckedOwnerBuildError> {
        let authored_scopes = self.authored_scope_projection()?;
        let ordered = self.signature_calls_outer_first()?;
        self.reserve_signature_declarations()?;
        self.apply_signature_evaluation_scopes(&authored_scopes)?;
        let declaration_plans = self
            .body
            .signature_lexical_plan
            .declarations()
            .iter()
            .cloned()
            .map(|declaration| (declaration.target.clone(), declaration))
            .collect::<BTreeMap<_, _>>();
        let mut materialized = BTreeSet::new();

        for (signature_call, call) in ordered {
            if !signature_call.valid {
                if !signature_call.outputs.is_empty() || !signature_call.contexts.is_empty() {
                    return Err(CheckedOwnerBuildError::new(
                        "invalid owner signature call retained dynamic bindings",
                    ));
                }
                continue;
            }
            let target = self.prepare_call_target(&call)?.ok_or_else(|| {
                CheckedOwnerBuildError::new(format!(
                    "valid owner signature call `{}` has no construction target",
                    signature_call.function
                ))
            })?;
            let expression = signature_call.expression as usize;
            let expression_id = OwnerExpressionId(signature_call.expression);
            let expression_input = self.syntax.expressions.get(expression).ok_or_else(|| {
                CheckedOwnerBuildError::new("owner signature call expression disappeared")
            })?;
            let call_source = expression_source(expression_input);
            let parameters = target
                .parameters
                .iter()
                .map(|parameter| (parameter.ordinal, parameter))
                .collect::<BTreeMap<_, _>>();
            let outputs = signature_call
                .outputs
                .iter()
                .map(|output| (output.formal_ordinal(), output))
                .collect::<BTreeMap<_, _>>();
            let mut entries = Vec::with_capacity(signature_call.matched_inputs.len());

            for input in &signature_call.matched_inputs {
                let parameter =
                    parameters
                        .get(&input.formal_ordinal)
                        .copied()
                        .ok_or_else(|| {
                            CheckedOwnerBuildError::new(format!(
                                "owner signature call `{}` references missing formal {}",
                                signature_call.function, input.formal_ordinal
                            ))
                        })?;
                if parameter.name != input.formal_name
                    || checked_parameter_kind(input.formal_kind) != parameter.kind
                    || owner_evaluation_scope(parameter.output_evaluation_ordinal)
                        != input.evaluation_scope
                {
                    return Err(CheckedOwnerBuildError::new(format!(
                        "owner signature call `{}` has stale formal {}",
                        signature_call.function, input.formal_ordinal
                    )));
                }
                let value = owner_expression_ref(self.syntax, input.expression as usize)?;
                let evaluation_scope = match input.evaluation_scope {
                    OwnerInterfaceEvaluationScope::Parent => OwnerEvaluationScope::Parent,
                    OwnerInterfaceEvaluationScope::Output { parameter_ordinal } => {
                        let output = parameters.get(&parameter_ordinal).copied().ok_or_else(|| {
                            CheckedOwnerBuildError::new(format!(
                                "owner signature input references missing output formal {parameter_ordinal}"
                            ))
                        })?;
                        OwnerEvaluationScope::Output {
                            formal: output.formal.clone(),
                        }
                    }
                };
                match (parameter.kind, input.argument_kind) {
                    (CheckedParameterKind::Value, OwnerArgumentKind::Named) => {
                        entries.push(OwnerCallEntry::Input {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            value,
                            from_pipe: input.from_pipe,
                            evaluation_scope,
                        });
                    }
                    (CheckedParameterKind::Value, OwnerArgumentKind::BareBinding) => {
                        return Err(CheckedOwnerBuildError::new(format!(
                            "valid signature call binds ordinary input `{}` as a bare OUT",
                            parameter.name
                        )));
                    }
                    (CheckedParameterKind::Out, OwnerArgumentKind::BareBinding) => {
                        let OwnerSignatureOutputBindingPlan::Fresh {
                            name,
                            expression: output_expression,
                            target: declaration_target,
                            ..
                        } = outputs.get(&parameter.ordinal).copied().ok_or_else(|| {
                            CheckedOwnerBuildError::new(format!(
                                "owner signature call lost FreshOut formal {}",
                                parameter.ordinal
                            ))
                        })?
                        else {
                            return Err(CheckedOwnerBuildError::new(
                                "owner signature call changed a fresh OUT to a forward",
                            ));
                        };
                        if name != &parameter.name || *output_expression != input.expression {
                            return Err(CheckedOwnerBuildError::new(
                                "owner signature FreshOut no longer matches its formal input",
                            ));
                        }
                        let declaration_plan =
                            declaration_plans.get(declaration_target).ok_or_else(|| {
                                CheckedOwnerBuildError::new(
                                    "owner signature FreshOut declaration plan is missing",
                                )
                            })?;
                        let (declaration, scope) = self.define_signature_fresh_out(
                            declaration_plan,
                            parameter,
                            &call_source,
                        )?;
                        if !materialized.insert(declaration_target.clone()) {
                            return Err(CheckedOwnerBuildError::new(
                                "owner signature FreshOut was materialized twice",
                            ));
                        }
                        entries.push(OwnerCallEntry::FreshOut {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            output: declaration,
                            scope_id: scope,
                        });
                    }
                    (CheckedParameterKind::Out, OwnerArgumentKind::Named) => {
                        let OwnerSignatureOutputBindingPlan::Forward {
                            name,
                            expression: output_expression,
                            target: output_target,
                            ..
                        } = outputs.get(&parameter.ordinal).copied().ok_or_else(|| {
                            CheckedOwnerBuildError::new(format!(
                                "owner signature call lost ForwardOut formal {}",
                                parameter.ordinal
                            ))
                        })?
                        else {
                            return Err(CheckedOwnerBuildError::new(
                                "owner signature call changed a forwarded OUT to fresh",
                            ));
                        };
                        if *output_expression != input.expression {
                            return Err(CheckedOwnerBuildError::new(
                                "owner signature ForwardOut no longer matches its formal input",
                            ));
                        }
                        let target_declaration =
                            self.effective_target_declaration(output_target)?;
                        if !self.effective_target_is_out(output_target)? {
                            return Err(CheckedOwnerBuildError::new(
                                "owner signature ForwardOut targets a non-OUT declaration",
                            ));
                        }
                        let source = matched_input_source_site(
                            &signature_call.stable_expression,
                            input.source,
                        )
                        .ok_or_else(|| {
                            CheckedOwnerBuildError::new(
                                "owner signature ForwardOut has no argument source",
                            )
                        })?;
                        self.call_occurrences.push(OwnerOccurrenceRow {
                            target: target_declaration.clone(),
                            kind: SemanticOccurrenceKind::ForwardOut,
                            source,
                        });
                        entries.push(OwnerCallEntry::ForwardOut {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            target: target_declaration,
                            target_name: name.clone(),
                        });
                    }
                }
            }

            if entries.len() != signature_call.matched_inputs.len()
                || outputs.len()
                    != entries
                        .iter()
                        .filter(|entry| {
                            matches!(
                                entry,
                                OwnerCallEntry::FreshOut { .. } | OwnerCallEntry::ForwardOut { .. }
                            )
                        })
                        .count()
            {
                return Err(CheckedOwnerBuildError::new(
                    "owner signature call entry projection is incomplete",
                ));
            }

            let mut contexts = Vec::with_capacity(signature_call.contexts.len());
            for context in &signature_call.contexts {
                let prepared = target
                    .contexts
                    .get(context.context_ordinal as usize)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner signature call context {} is missing from construction ABI",
                            context.context_ordinal
                        ))
                    })?;
                if prepared.name != context.name
                    || prepared.kind != context.kind
                    || prepared.provider_ordinal != context.provider_parameter_ordinal
                {
                    return Err(CheckedOwnerBuildError::new(
                        "owner signature call context diverges from construction ABI",
                    ));
                }
                let declaration_plan = declaration_plans.get(&context.target).ok_or_else(|| {
                    CheckedOwnerBuildError::new(
                        "owner signature call-context declaration plan is missing",
                    )
                })?;
                let row =
                    self.define_signature_call_context(declaration_plan, prepared, &call_source)?;
                if !materialized.insert(context.target.clone()) {
                    return Err(CheckedOwnerBuildError::new(
                        "owner signature call context was materialized twice",
                    ));
                }
                contexts.push(row);
            }
            if !call.valid {
                continue;
            }
            let call_id = OwnerCallId(checked_u32(self.call_rows.len(), "owner call id")?);
            if self
                .call_ids
                .insert(signature_call.stable_expression.clone(), call_id)
                .is_some()
            {
                return Err(CheckedOwnerBuildError::new(
                    "owner body contains duplicate call expression identity",
                ));
            }
            let explicit_pass = signature_call
                .explicit_pass
                .as_ref()
                .map(|pass| -> Result<_, CheckedOwnerBuildError> {
                    Ok((
                        owner_expression_ref(self.syntax, pass.expression as usize)?,
                        signature_pass_source_site(&signature_call.stable_expression, pass.source),
                    ))
                })
                .transpose()?;
            let explicit_pass_source = explicit_pass.as_ref().map(|(_, source)| source.clone());
            let context_binding = explicit_pass.map_or_else(
                || {
                    let inherited = if self.own_interface.context.is_some() {
                        Some(OwnerContextFormalRef::Local {
                            formal: OwnerContextFormalId(0),
                        })
                    } else {
                        self.body
                            .signature_lexical_plan
                            .imported_captures()
                            .iter()
                            .find_map(|target| match target {
                                OwnerLexicalTargetRef::ContextFormal { owner } => {
                                    Some(OwnerContextFormalRef::Imported {
                                        owner: owner.clone(),
                                    })
                                }
                                _ => None,
                            })
                    };
                    if target.requires_pass {
                        inherited.map_or(OwnerContextBinding::None, |formal| {
                            OwnerContextBinding::Inherited { formal }
                        })
                    } else {
                        OwnerContextBinding::None
                    }
                },
                |(value, source)| OwnerContextBinding::Explicit { value, source },
            );
            let owner_callable = (self.own_interface.declaration_kind
                == Some(OwnerDeclarationKind::Function))
            .then(|| {
                self.declaration_ids
                    .get(&OwnerDeclarationStableKey::Public)
                    .copied()
                    .map(local_declaration_ref)
            })
            .flatten();
            self.call_occurrences.push(OwnerOccurrenceRow {
                target: target.callable.clone(),
                kind: SemanticOccurrenceKind::Call,
                source: call_source.clone(),
            });
            if let Some(source) = explicit_pass_source {
                self.call_occurrences.push(OwnerOccurrenceRow {
                    target: target.callable.clone(),
                    kind: SemanticOccurrenceKind::Pass,
                    source,
                });
            }
            let contextual_substitutions = target
                .context_formal
                .as_ref()
                .into_iter()
                .flat_map(|formal| {
                    call.type_substitutions
                        .iter()
                        .filter(|substitution| {
                            call.contextual_type_variables
                                .contains(&substitution.variable)
                        })
                        .map(move |substitution| OwnerContextTypeSubstitution {
                            formal: formal.clone(),
                            variable: substitution.variable,
                            value: substitution.value.clone(),
                        })
                })
                .collect();
            self.call_rows.push(OwnerCallRow {
                id: call_id,
                stable_key: signature_call.stable_expression.clone(),
                expression: expression_id,
                callable: target.callable,
                owner_callable,
                function: signature_call.function,
                intrinsic: target.intrinsic,
                entries,
                contexts,
                context_binding,
                contextual_substitutions,
                type_substitutions: call
                    .type_substitutions
                    .iter()
                    .map(|substitution| OwnerTypeSubstitution {
                        variable: substitution.variable,
                        value: substitution.value.clone(),
                    })
                    .collect(),
                syntax_discriminated_result: call.syntax_discriminated_result,
                result: call.result,
                role: target.role,
                source: call_source,
            });
        }
        if materialized.len() != declaration_plans.len() {
            return Err(CheckedOwnerBuildError::new(
                "owner checked construction did not materialize every signature declaration",
            ));
        }
        Ok(())
    }

    fn validate_signature_lexical_plan(&self) -> Result<(), CheckedOwnerBuildError> {
        let expected = self
            .body
            .signature_lexical_plan
            .declarations()
            .iter()
            .map(|declaration| match &declaration.target {
                OwnerSignatureDeclarationTarget::FreshOut {
                    call,
                    formal_ordinal,
                } => OwnerDeclarationStableKey::FreshOut {
                    call: call.clone(),
                    formal_ordinal: *formal_ordinal,
                },
                OwnerSignatureDeclarationTarget::CallContext {
                    call,
                    context_ordinal,
                } => OwnerDeclarationStableKey::CallContext {
                    call: call.clone(),
                    ordinal: *context_ordinal,
                },
            })
            .collect::<BTreeSet<_>>();
        let actual = self
            .declaration_ids
            .keys()
            .filter(|key| {
                matches!(
                    key,
                    OwnerDeclarationStableKey::FreshOut { .. }
                        | OwnerDeclarationStableKey::CallContext { .. }
                )
            })
            .cloned()
            .collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(CheckedOwnerBuildError::new(format!(
                "checked owner {:?} materialized dynamic declarations different from its signature lexical plan",
                self.syntax.owner
            )));
        }
        Ok(())
    }

    fn rebase_expression_tree(
        &mut self,
        expression: OwnerExpressionId,
        scope: OwnerScopeRef,
    ) -> Result<(), CheckedOwnerBuildError> {
        let mut assigned = vec![true; self.syntax.expressions.len()];
        let declaration = self.expression_declarations[expression.0 as usize].clone();
        self.assign_expression_tree(expression.0, scope, declaration, true, &mut assigned)
    }

    fn declaration_statement(&self, declaration: OwnerDeclarationId) -> Option<OwnerStatementId> {
        self.statement_declarations
            .iter()
            .find_map(|(statement, candidate)| (*candidate == declaration).then_some(*statement))
    }

    fn declaration_statement_ref(
        &self,
        declaration: &OwnerDeclarationRef,
    ) -> Option<OwnerStatementId> {
        match declaration {
            OwnerDeclarationRef::Local { declaration } => self.declaration_statement(*declaration),
            OwnerDeclarationRef::Imported { .. }
            | OwnerDeclarationRef::ImportedStable { .. }
            | OwnerDeclarationRef::Abi { .. }
            | OwnerDeclarationRef::ScopeOwner { .. } => None,
        }
    }

    fn resource_statement(
        &self,
        declaration: &OwnerDeclarationRef,
        expression: OwnerExpressionId,
        containing_statements: &[Option<OwnerStatementId>],
    ) -> Option<OwnerStatementId> {
        self.declaration_statement_ref(declaration).or_else(|| {
            containing_statements
                .get(expression.0 as usize)
                .copied()
                .flatten()
        })
    }

    fn projection_to_expression(
        &self,
        rows: &[OwnerExpressionRow],
        current: &OwnerExpressionRef,
        target: OwnerExpressionId,
        visiting: &mut BTreeSet<OwnerExpressionId>,
    ) -> Option<Vec<String>> {
        let OwnerExpressionRef::Local {
            expression: current,
        } = current
        else {
            return None;
        };
        if *current == target {
            return Some(Vec::new());
        }
        if !visiting.insert(*current) {
            return None;
        }
        let expression = rows.get(current.0 as usize)?;
        let direct = |child: &OwnerExpressionRef, visiting: &mut BTreeSet<_>| {
            self.projection_to_expression(rows, child, target, visiting)
        };
        let result = match &expression.kind {
            OwnerExpressionKind::TaggedObject { fields, .. }
            | OwnerExpressionKind::Object { fields } => fields.iter().find_map(|field| {
                let mut projection = direct(&field.value, visiting)?;
                projection.insert(0, field.name.clone());
                Some(projection)
            }),
            OwnerExpressionKind::Call { call } => self
                .call_rows
                .get(call.0 as usize)
                .into_iter()
                .flat_map(|call| &call.entries)
                .find_map(|entry| match entry {
                    OwnerCallEntry::Input { value, .. } => direct(value, visiting),
                    OwnerCallEntry::FreshOut { .. } | OwnerCallEntry::ForwardOut { .. } => None,
                }),
            OwnerExpressionKind::Draining { input }
            | OwnerExpressionKind::Hold { initial: input, .. }
            | OwnerExpressionKind::Flush { payload: input } => direct(input, visiting),
            OwnerExpressionKind::When { input, arms }
            | OwnerExpressionKind::While { input, arms } => direct(input, visiting)
                .or_else(|| arms.iter().find_map(|arm| direct(arm, visiting))),
            OwnerExpressionKind::Then { input, output } => direct(input, visiting)
                .or_else(|| output.as_ref().and_then(|output| direct(output, visiting))),
            OwnerExpressionKind::Infix { left, right, .. } => {
                direct(left, visiting).or_else(|| direct(right, visiting))
            }
            OwnerExpressionKind::MatchArm { output, .. } => {
                output.as_ref().and_then(|output| direct(output, visiting))
            }
            OwnerExpressionKind::Block { bindings, result } => bindings
                .iter()
                .find_map(|binding| direct(&binding.value, visiting))
                .or_else(|| result.as_ref().and_then(|result| direct(result, visiting))),
            OwnerExpressionKind::List { items, .. }
            | OwnerExpressionKind::Bytes { items, .. }
            | OwnerExpressionKind::Set { items }
            | OwnerExpressionKind::Latest { branches: items } => {
                items.iter().find_map(|item| direct(item, visiting))
            }
            OwnerExpressionKind::Map { entries } => {
                entries.iter().find_map(|entry| direct(entry, visiting))
            }
            OwnerExpressionKind::MapEntry { key, value } => {
                direct(key, visiting).or_else(|| direct(value, visiting))
            }
            OwnerExpressionKind::TextTemplate { segments } => {
                segments.iter().find_map(|segment| match segment {
                    OwnerTextSegment::Static { .. } => None,
                    OwnerTextSegment::Dynamic { value } => direct(value, visiting),
                })
            }
            OwnerExpressionKind::Read { .. }
            | OwnerExpressionKind::Passed { .. }
            | OwnerExpressionKind::ExternalRead { .. }
            | OwnerExpressionKind::Drain { .. }
            | OwnerExpressionKind::Text { .. }
            | OwnerExpressionKind::Number { .. }
            | OwnerExpressionKind::Bits { .. }
            | OwnerExpressionKind::BytesByte { .. }
            | OwnerExpressionKind::Absent
            | OwnerExpressionKind::Tag { .. }
            | OwnerExpressionKind::Source
            | OwnerExpressionKind::Delimiter
            | OwnerExpressionKind::Invalid { .. } => None,
        };
        visiting.remove(current);
        result
    }

    fn declaration_resource_projection(
        &self,
        rows: &[OwnerExpressionRow],
        declaration: &OwnerDeclarationRef,
        target: OwnerExpressionId,
    ) -> Vec<String> {
        let OwnerDeclarationRef::Local { declaration } = declaration else {
            return Vec::new();
        };
        self.declaration_specs
            .get(declaration.0 as usize)
            .and_then(Option::as_ref)
            .and_then(|declaration| declaration.value.as_ref())
            .and_then(|root| {
                self.projection_to_expression(rows, root, target, &mut BTreeSet::new())
            })
            .unwrap_or_default()
    }

    fn duration_milliseconds(
        &self,
        rows: &[OwnerExpressionRow],
        expression: &OwnerExpressionRef,
    ) -> Option<u64> {
        let OwnerExpressionRef::Local { expression } = expression else {
            return None;
        };
        let OwnerExpressionKind::TaggedObject { tag, fields } =
            &rows.get(expression.0 as usize)?.kind
        else {
            return None;
        };
        if tag != "Duration" {
            return None;
        }
        fields.iter().find_map(|field| {
            let scale = match field.name.as_str() {
                "milliseconds" => 1,
                "seconds" => 1_000,
                _ => return None,
            };
            let OwnerExpressionRef::Local { expression } = field.value else {
                return None;
            };
            let OwnerExpressionKind::Number { value } = &rows.get(expression.0 as usize)?.kind
            else {
                return None;
            };
            value
                .checked_mul(&ExactNumber::from_u64(scale))
                .ok()?
                .to_u64_exact()
                .ok()
        })
    }

    fn stateful_call_initial(&self, call: OwnerCallId) -> Option<OwnerExpressionRef> {
        let call = self.call_rows.get(call.0 as usize)?;
        if !matches!(
            call.callable,
            OwnerDeclarationRef::Abi {
                declaration: OwnerAbiDeclarationKey {
                    kind: OwnerAbiDeclarationKind::BuiltinCallable,
                    ..
                },
                ..
            }
        ) {
            return None;
        }
        call.entries
            .iter()
            .filter_map(|entry| match entry {
                OwnerCallEntry::Input { formal, value, .. } => {
                    owner_parameter_ordinal(formal).map(|ordinal| (ordinal, value))
                }
                OwnerCallEntry::FreshOut { .. } | OwnerCallEntry::ForwardOut { .. } => None,
            })
            .min_by_key(|(ordinal, _)| *ordinal)
            .map(|(_, value)| value.clone())
    }

    fn expression_is_startup_safe(
        &self,
        rows: &[OwnerExpressionRow],
        root: OwnerExpressionId,
    ) -> bool {
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if !visited.insert(expression) {
                continue;
            }
            let Some(row) = rows.get(expression.0 as usize) else {
                return false;
            };
            if row.effect.invokes_host || row.effect.emits_source {
                return false;
            }
            match &row.kind {
                OwnerExpressionKind::Hold { initial, .. } => match initial {
                    OwnerExpressionRef::Local { expression } => pending.push(*expression),
                    OwnerExpressionRef::Child { .. } => return false,
                },
                OwnerExpressionKind::Latest { branches } => match branches.first() {
                    Some(OwnerExpressionRef::Local { expression }) => pending.push(*expression),
                    _ => return false,
                },
                OwnerExpressionKind::Call { call } if row.effect.writes_state => {
                    match self.stateful_call_initial(*call) {
                        Some(OwnerExpressionRef::Local { expression }) => pending.push(expression),
                        _ => return false,
                    }
                }
                _ => {
                    pending.extend(
                        self.graph
                            .expression_inputs(expression)
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|child| match child {
                                OwnerExpressionRef::Local { expression } => Some(*expression),
                                OwnerExpressionRef::Child { .. } => None,
                            }),
                    );
                }
            }
        }
        true
    }

    fn statement_child_values(
        statements: &[OwnerStatementRow],
        statement: OwnerStatementId,
    ) -> Vec<OwnerExpressionId> {
        let Some(statement) = statements.get(statement.0 as usize) else {
            return Vec::new();
        };
        let mut pending = statement
            .children
            .iter()
            .rev()
            .filter_map(|child| match child {
                OwnerStatementChild::Local { statement } => Some(*statement),
                OwnerStatementChild::Owner { .. } => None,
            })
            .collect::<Vec<_>>();
        let mut visited = BTreeSet::new();
        let mut values = Vec::new();
        while let Some(statement) = pending.pop() {
            if !visited.insert(statement) {
                continue;
            }
            let Some(statement) = statements.get(statement.0 as usize) else {
                continue;
            };
            if let Some(OwnerExpressionRef::Local { expression }) = statement.value {
                values.push(expression);
                continue;
            }
            pending.extend(
                statement
                    .children
                    .iter()
                    .rev()
                    .filter_map(|child| match child {
                        OwnerStatementChild::Local { statement } => Some(*statement),
                        OwnerStatementChild::Owner { .. } => None,
                    }),
            );
        }
        values
    }

    fn hold_update_mergers(
        &self,
        statements: &[OwnerStatementRow],
        expressions: &[OwnerExpressionRow],
    ) -> BTreeSet<OwnerExpressionId> {
        let mut roots = Vec::new();
        for expression in expressions
            .iter()
            .filter(|expression| matches!(expression.kind, OwnerExpressionKind::Hold { .. }))
        {
            if let Some(statement) = expression
                .declaration
                .as_ref()
                .and_then(|declaration| self.declaration_statement_ref(declaration))
            {
                roots.extend(Self::statement_child_values(statements, statement));
            }
        }
        for statement in statements
            .iter()
            .filter(|statement| matches!(statement.kind, OwnerStatementKind::Hold { .. }))
        {
            roots.extend(Self::statement_child_values(statements, statement.id));
        }

        let mut mergers = BTreeSet::new();
        let mut pending = roots;
        let mut visited = BTreeSet::new();
        while let Some(expression) = pending.pop() {
            if !visited.insert(expression) {
                continue;
            }
            let Some(row) = expressions.get(expression.0 as usize) else {
                continue;
            };
            if matches!(row.kind, OwnerExpressionKind::Hold { .. })
                || matches!(row.kind, OwnerExpressionKind::Call { .. }) && row.effect.writes_state
            {
                continue;
            }
            if matches!(row.kind, OwnerExpressionKind::Latest { .. }) {
                mergers.insert(expression);
            }
            pending.extend(
                self.graph
                    .expression_inputs(expression)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|input| match input {
                        OwnerExpressionRef::Local { expression } => Some(*expression),
                        OwnerExpressionRef::Child { .. } => None,
                    }),
            );
        }
        mergers
    }

    fn containing_expression_statements(
        &self,
        statements: &[OwnerStatementRow],
        expressions: &[OwnerExpressionRow],
    ) -> Vec<Option<OwnerStatementId>> {
        let mut owners = vec![None; expressions.len()];
        for statement in statements.iter().rev() {
            let statement_id = statement.id;
            let mut pending = Vec::with_capacity(2);
            if let Some(OwnerExpressionRef::Local { expression }) = &statement.value {
                pending.push(*expression);
            }
            if let Some(expression) = self
                .syntax
                .statements
                .get(statement.id.0 as usize)
                .and_then(|statement| statement.expression)
            {
                let expression = OwnerExpressionId(expression);
                if !pending.contains(&expression) {
                    pending.push(expression);
                }
            }
            let mut visited = BTreeSet::new();
            while let Some(expression) = pending.pop() {
                if !visited.insert(expression) {
                    continue;
                }
                let Some(row) = expressions.get(expression.0 as usize) else {
                    continue;
                };
                owners[expression.0 as usize].get_or_insert(statement_id);
                match &row.kind {
                    OwnerExpressionKind::Call { call } => {
                        if let Some(call) = self.call_rows.get(call.0 as usize) {
                            pending.extend(call.entries.iter().filter_map(|entry| match entry {
                                OwnerCallEntry::Input {
                                    value: OwnerExpressionRef::Local { expression },
                                    ..
                                } => Some(*expression),
                                OwnerCallEntry::Input { .. }
                                | OwnerCallEntry::FreshOut { .. }
                                | OwnerCallEntry::ForwardOut { .. } => None,
                            }));
                        }
                    }
                    OwnerExpressionKind::Hold { initial, .. }
                    | OwnerExpressionKind::Draining { input: initial }
                    | OwnerExpressionKind::When { input: initial, .. }
                    | OwnerExpressionKind::While { input: initial, .. } => {
                        if let OwnerExpressionRef::Local { expression } = initial {
                            pending.push(*expression);
                        }
                    }
                    OwnerExpressionKind::Then { input, output } => {
                        pending.extend(std::iter::once(input).chain(output.iter()).filter_map(
                            |value| match value {
                                OwnerExpressionRef::Local { expression } => Some(*expression),
                                OwnerExpressionRef::Child { .. } => None,
                            },
                        ));
                    }
                    OwnerExpressionKind::MatchArm { output, .. } => {
                        if let Some(OwnerExpressionRef::Local { expression }) = output {
                            pending.push(*expression);
                        }
                    }
                    OwnerExpressionKind::Infix { left, right, .. } => {
                        pending.extend([left, right].into_iter().filter_map(|value| match value {
                            OwnerExpressionRef::Local { expression } => Some(*expression),
                            OwnerExpressionRef::Child { .. } => None,
                        }));
                    }
                    OwnerExpressionKind::Object { fields }
                    | OwnerExpressionKind::TaggedObject { fields, .. } => {
                        pending.extend(fields.iter().filter_map(|field| match &field.value {
                            OwnerExpressionRef::Local { expression } => Some(*expression),
                            OwnerExpressionRef::Child { .. } => None,
                        }));
                    }
                    OwnerExpressionKind::Read { .. }
                    | OwnerExpressionKind::Passed { .. }
                    | OwnerExpressionKind::ExternalRead { .. }
                    | OwnerExpressionKind::Drain { .. }
                    | OwnerExpressionKind::Text { .. }
                    | OwnerExpressionKind::TextTemplate { .. }
                    | OwnerExpressionKind::Number { .. }
                    | OwnerExpressionKind::Bits { .. }
                    | OwnerExpressionKind::BytesByte { .. }
                    | OwnerExpressionKind::Absent
                    | OwnerExpressionKind::Flush { .. }
                    | OwnerExpressionKind::Tag { .. }
                    | OwnerExpressionKind::Source
                    | OwnerExpressionKind::Latest { .. }
                    | OwnerExpressionKind::Block { .. }
                    | OwnerExpressionKind::List { .. }
                    | OwnerExpressionKind::Bytes { .. }
                    | OwnerExpressionKind::MapEntry { .. }
                    | OwnerExpressionKind::Map { .. }
                    | OwnerExpressionKind::Set { .. }
                    | OwnerExpressionKind::Delimiter
                    | OwnerExpressionKind::Invalid { .. } => {}
                }
            }
        }
        owners
    }

    fn inline_list_authority_root(
        &self,
        expressions: &[OwnerExpressionRow],
        root: OwnerExpressionId,
    ) -> Option<OwnerExpressionId> {
        let mut current = root;
        let mut visited = BTreeSet::new();
        while visited.insert(current) {
            let expression = expressions.get(current.0 as usize)?;
            current = match &expression.kind {
                OwnerExpressionKind::List { .. } => return Some(current),
                OwnerExpressionKind::Call { call } => {
                    let call = self.call_rows.get(call.0 as usize)?;
                    if call.function == "List/range" {
                        return Some(current);
                    }
                    let mut inputs =
                        call.entries.iter().filter_map(|entry| match entry {
                            OwnerCallEntry::Input {
                                value: OwnerExpressionRef::Local { expression },
                                ..
                            } if expressions.get(expression.0 as usize).is_some_and(
                                |expression| matches!(expression.flow_type.ty, Type::List(_)),
                            ) =>
                            {
                                Some(*expression)
                            }
                            OwnerCallEntry::Input { .. }
                            | OwnerCallEntry::FreshOut { .. }
                            | OwnerCallEntry::ForwardOut { .. } => None,
                        });
                    let input = inputs.next()?;
                    if inputs.next().is_some() {
                        return None;
                    }
                    input
                }
                OwnerExpressionKind::Draining {
                    input: OwnerExpressionRef::Local { expression },
                }
                | OwnerExpressionKind::Block {
                    result: Some(OwnerExpressionRef::Local { expression }),
                    ..
                }
                | OwnerExpressionKind::Then {
                    output: Some(OwnerExpressionRef::Local { expression }),
                    ..
                }
                | OwnerExpressionKind::MatchArm {
                    output: Some(OwnerExpressionRef::Local { expression }),
                    ..
                } => *expression,
                _ => return None,
            };
        }
        None
    }

    /// Whether `target` is the value published by a declaration after walking
    /// only transparent result wrappers. This is the owner-row equivalent of
    /// the retiring dense checker's declaration-result authority walk.
    fn state_is_declaration_result(
        expressions: &[OwnerExpressionRow],
        root: &OwnerExpressionRef,
        target: OwnerExpressionId,
    ) -> bool {
        let OwnerExpressionRef::Local { expression } = root else {
            return false;
        };
        let mut current = *expression;
        let mut visited = BTreeSet::new();
        while visited.insert(current) {
            if current == target {
                return true;
            }
            let Some(expression) = expressions
                .get(current.0 as usize)
                .filter(|expression| expression.id == current)
            else {
                return false;
            };
            let next = match &expression.kind {
                OwnerExpressionKind::Draining { input }
                | OwnerExpressionKind::Flush { payload: input } => input,
                OwnerExpressionKind::Block {
                    result: Some(result),
                    ..
                }
                | OwnerExpressionKind::MatchArm {
                    output: Some(result),
                    ..
                } => result,
                _ => return false,
            };
            let OwnerExpressionRef::Local { expression } = next else {
                return false;
            };
            current = *expression;
        }
        false
    }

    /// Return the count of state rows already owned by an enclosing declaration
    /// before this fieldless-HOLD owner begins. Owner-local row counts cannot
    /// distinguish nested child owners because every child starts at row zero;
    /// the stable item-route ancestry supplies the missing authored prefix.
    fn inherited_hold_state_ordinal_base(
        &self,
        declaration: &OwnerDeclarationRef,
    ) -> Result<Option<usize>, CheckedOwnerBuildError> {
        if !matches!(
            declaration,
            OwnerDeclarationRef::ScopeOwner {
                scope: OwnerScopeRef::Imported { .. },
            } | OwnerDeclarationRef::ImportedStable { .. }
                | OwnerDeclarationRef::Imported { .. }
        ) {
            return Ok(None);
        }
        let Some(root) = self.syntax.statements.first() else {
            return Ok(None);
        };
        if !matches!(
            &root.kind,
            AstStatementKind::Hold {
                field: None,
                name: Some(_),
            }
        ) {
            return Ok(None);
        }
        Ok(self.syntax.containing_hold_state_ordinal_base)
    }

    fn derive_resource_rows(
        &self,
        statements: &mut [OwnerStatementRow],
        expressions: &[OwnerExpressionRow],
    ) -> Result<DerivedOwnerRows, CheckedOwnerBuildError> {
        let mut derived = DerivedOwnerRows::default();
        let hold_update_mergers = self.hold_update_mergers(statements, expressions);
        let containing_statements = self.containing_expression_statements(statements, expressions);

        for expression in expressions.iter().filter(|expression| {
            if matches!(expression.kind, OwnerExpressionKind::Source) {
                return true;
            }
            let OwnerExpressionKind::Call { call } = &expression.kind else {
                return false;
            };
            expression.effect.emits_source
                && self
                    .call_rows
                    .get(call.0 as usize)
                    .is_some_and(|call| matches!(call.callable, OwnerDeclarationRef::Abi { .. }))
        }) {
            let Some(declaration) = expression.declaration.clone() else {
                continue;
            };
            let Some(statement) =
                self.resource_statement(&declaration, expression.id, &containing_statements)
            else {
                continue;
            };
            let interval_ms = match &expression.kind {
                OwnerExpressionKind::Call { call } => {
                    self.call_rows.get(call.0 as usize).and_then(|call| {
                        call.entries.iter().find_map(|entry| match entry {
                            OwnerCallEntry::Input { name, value, .. } if name == "duration" => {
                                self.duration_milliseconds(expressions, value)
                            }
                            _ => None,
                        })
                    })
                }
                _ => None,
            };
            let source = OwnerSourceRow {
                id: OwnerSourceId(checked_u32(derived.sources.len(), "owner source id")?),
                stable_key: OwnerSourceStableKey {
                    owner: self.syntax.owner.clone(),
                    statement: self.syntax.statements[statement.0 as usize]
                        .stable_key
                        .clone(),
                    expression: expression.stable_key.clone(),
                },
                declaration: declaration.clone(),
                statement,
                expression: expression.id,
                owner_scope: expression.scope.clone(),
                path: OwnerSemanticPath {
                    anchor: declaration.clone(),
                    projection: self.declaration_resource_projection(
                        expressions,
                        &declaration,
                        expression.id,
                    ),
                },
                interval_ms,
                payload_type: expression.flow_type.ty.clone(),
                source: expression.source.clone(),
            };
            statements[statement.0 as usize]
                .resources
                .push(OwnerResourceBinding::Source {
                    source: boon_checked::OwnerSourceRef::Local { source: source.id },
                });
            derived.sources.push(source);
        }

        for expression in expressions {
            let state = match &expression.kind {
                OwnerExpressionKind::Hold { initial, .. } => {
                    Some((CheckedStateKind::Hold, initial.clone()))
                }
                OwnerExpressionKind::Latest { branches }
                    if !hold_update_mergers.contains(&expression.id)
                        && branches.first().is_some_and(|branch| {
                            matches!(branch, OwnerExpressionRef::Local { expression: initial }
                            if expressions.get(initial.0 as usize).is_some_and(|initial_row| {
                                initial_row.flow_type.mode == FlowMode::Continuous
                                    && self.expression_is_startup_safe(expressions, *initial)
                            }))
                        }) =>
                {
                    branches
                        .first()
                        .cloned()
                        .map(|initial| (CheckedStateKind::InitialLatest, initial))
                }
                OwnerExpressionKind::Call { call } if expression.effect.writes_state => self
                    .stateful_call_initial(*call)
                    .map(|initial| (CheckedStateKind::StatefulCall, initial)),
                _ => None,
            };
            let Some((kind, initial)) = state else {
                continue;
            };
            let Some(declaration) = expression.declaration.clone() else {
                continue;
            };
            let Some(statement) =
                self.resource_statement(&declaration, expression.id, &containing_statements)
            else {
                continue;
            };
            let mut projection =
                self.declaration_resource_projection(expressions, &declaration, expression.id);
            let inherited_ordinal_base = self.inherited_hold_state_ordinal_base(&declaration)?;
            let local_declaration = match &declaration {
                OwnerDeclarationRef::Local { declaration } => self
                    .declaration_specs
                    .get(declaration.0 as usize)
                    .and_then(Option::as_ref),
                OwnerDeclarationRef::Imported { .. }
                | OwnerDeclarationRef::ImportedStable { .. }
                | OwnerDeclarationRef::Abi { .. }
                | OwnerDeclarationRef::ScopeOwner { .. } => None,
            };
            let declaration_result = local_declaration
                .and_then(|declaration| declaration.value.as_ref())
                .is_some_and(|root| {
                    Self::state_is_declaration_result(expressions, root, expression.id)
                })
                || inherited_ordinal_base.is_some()
                    && self.syntax.containing_hold_is_declaration_result;
            let function_declaration = local_declaration
                .is_some_and(|declaration| declaration.kind == CheckedDeclarationKind::Function);
            if projection.is_empty() && (!declaration_result || function_declaration) {
                let local_ordinal = derived
                    .states
                    .iter()
                    .filter(|state| state.declaration == declaration)
                    .count();
                let ordinal = inherited_ordinal_base
                    .unwrap_or_default()
                    .checked_add(local_ordinal)
                    .ok_or_else(|| CheckedOwnerBuildError::new("owner state ordinal overflowed"))?;
                projection.push(format!("state_{ordinal}"));
            }
            let state = OwnerStateRow {
                id: OwnerStateId(checked_u32(derived.states.len(), "owner state id")?),
                declaration: declaration.clone(),
                statement,
                expression: expression.id,
                initial,
                owner_scope: expression.scope.clone(),
                path: OwnerSemanticPath {
                    anchor: declaration,
                    projection,
                },
                kind,
                flow_type: expression.flow_type.clone(),
                source: expression.source.clone(),
            };
            statements[statement.0 as usize]
                .resources
                .push(OwnerResourceBinding::State { state: state.id });
            derived.states.push(state);
        }

        let state_declarations = derived
            .states
            .iter()
            .map(|state| state.declaration.clone())
            .collect::<BTreeSet<_>>();
        for statement in statements
            .iter_mut()
            .filter(|statement| matches!(statement.kind, OwnerStatementKind::Hold { .. }))
        {
            let OwnerStatementKind::Hold {
                declaration: Some(declaration),
                ..
            } = statement.kind
            else {
                continue;
            };
            let declaration_ref = local_declaration_ref(declaration);
            if state_declarations.contains(&declaration_ref) {
                continue;
            }
            let Some(OwnerExpressionRef::Local { expression }) = statement.value.clone() else {
                continue;
            };
            let Some(value) = expressions.get(expression.0 as usize) else {
                continue;
            };
            let state = OwnerStateRow {
                id: OwnerStateId(checked_u32(derived.states.len(), "owner state id")?),
                declaration: declaration_ref.clone(),
                statement: statement.id,
                expression,
                initial: OwnerExpressionRef::Local { expression },
                owner_scope: statement.scope.clone(),
                path: OwnerSemanticPath {
                    anchor: declaration_ref,
                    projection: Vec::new(),
                },
                kind: CheckedStateKind::StatementHold,
                flow_type: self
                    .declaration_specs
                    .get(declaration.0 as usize)
                    .and_then(Option::as_ref)
                    .map(|declaration| declaration.flow_type.clone())
                    .unwrap_or_else(|| value.flow_type.clone()),
                source: statement.source.clone(),
            };
            statement
                .resources
                .push(OwnerResourceBinding::State { state: state.id });
            derived.states.push(state);
        }

        for expression in expressions
            .iter()
            .filter(|expression| matches!(expression.kind, OwnerExpressionKind::List { .. }))
        {
            let Some(declaration) = expression.declaration.clone() else {
                continue;
            };
            let local_declaration = match &declaration {
                OwnerDeclarationRef::Local { declaration } => Some(*declaration),
                OwnerDeclarationRef::Imported { .. }
                | OwnerDeclarationRef::ImportedStable { .. }
                | OwnerDeclarationRef::Abi { .. }
                | OwnerDeclarationRef::ScopeOwner { .. } => None,
            };
            let declaration_authority = local_declaration
                .and_then(|declaration| {
                    self.declaration_specs
                        .get(declaration.0 as usize)
                        .and_then(Option::as_ref)
                })
                .and_then(|declaration| match declaration.value {
                    Some(OwnerExpressionRef::Local { expression }) => Some(expression),
                    Some(OwnerExpressionRef::Child { .. }) | None => None,
                })
                .and_then(|root| self.inline_list_authority_root(expressions, root))
                == Some(expression.id);
            let statement = declaration_authority
                .then(|| {
                    local_declaration
                        .and_then(|declaration| self.declaration_statement(declaration))
                })
                .flatten()
                .or_else(|| {
                    containing_statements
                        .get(expression.id.0 as usize)
                        .copied()
                        .flatten()
                })
                .or_else(|| self.declaration_statement_ref(&declaration));
            let Some(statement) = statement else {
                continue;
            };
            let OwnerExpressionKind::List { capacity, .. } = &expression.kind else {
                continue;
            };
            let Type::List(item_type) = &expression.flow_type.ty else {
                continue;
            };
            let authority_item_type = local_declaration
                .and_then(|declaration| {
                    self.declaration_specs
                        .get(declaration.0 as usize)
                        .and_then(Option::as_ref)
                })
                .and_then(|declaration| match &declaration.flow_type.ty {
                    Type::List(item_type) => Some(item_type.as_ref().clone()),
                    _ => None,
                })
                .unwrap_or_else(|| item_type.as_ref().clone());
            let mut projection =
                self.declaration_resource_projection(expressions, &declaration, expression.id);
            if projection.is_empty()
                && local_declaration.is_some_and(|declaration| {
                    self.declaration_specs
                        .get(declaration.0 as usize)
                        .and_then(Option::as_ref)
                        .is_some_and(|declaration| {
                            declaration.kind == CheckedDeclarationKind::Function
                        })
                })
            {
                projection.push(format!(
                    "list_{}",
                    derived
                        .lists
                        .iter()
                        .filter(|list| list.declaration == declaration)
                        .count()
                ));
            }
            let list = OwnerListRow {
                id: OwnerListId(checked_u32(derived.lists.len(), "owner list id")?),
                declaration: declaration.clone(),
                statement,
                producer: expression.id,
                owner_scope: expression.scope.clone(),
                path: OwnerSemanticPath {
                    anchor: declaration,
                    projection,
                },
                item_type: authority_item_type,
                capacity: *capacity,
                key_policy: CheckedListKeyPolicy::GeneratedOccurrenceU64 {
                    has_generation: true,
                },
                source: expression.source.clone(),
            };
            statements[statement.0 as usize]
                .resources
                .push(OwnerResourceBinding::ListAuthority { list: list.id });
            derived.lists.push(list);
        }

        derived.resource_projection_seeds = expressions
            .iter()
            .filter_map(|expression| {
                let (target, projection) = match &expression.kind {
                    OwnerExpressionKind::Read {
                        target,
                        projection,
                        source_seed: None,
                    }
                    | OwnerExpressionKind::Drain { target, projection } => (target, projection),
                    _ => return None,
                };
                if projection.is_empty() {
                    return None;
                }
                let required_type = if crate::is_specific_type(&expression.flow_type.ty) {
                    expression.flow_type.ty.clone()
                } else {
                    projection.last().map_or(Type::Unknown, |field| {
                        crate::source_payload_field_type(field)
                    })
                };
                Some(OwnerResourceProjectionSeedRow {
                    expression: expression.id,
                    target: target.clone(),
                    projection: projection.clone(),
                    required_type,
                })
            })
            .collect();

        derived.call_result_paths = self
            .call_rows
            .iter()
            .filter_map(|call| {
                let OwnerDeclarationRef::Local { declaration } = expressions
                    .get(call.expression.0 as usize)?
                    .declaration
                    .as_ref()?
                else {
                    return None;
                };
                self.declaration_specs
                    .get(declaration.0 as usize)
                    .and_then(Option::as_ref)?
                    .value
                    .as_ref()?;
                self.projection_to_expression(
                    expressions,
                    self.declaration_specs[declaration.0 as usize]
                        .as_ref()?
                        .value
                        .as_ref()?,
                    call.expression,
                    &mut BTreeSet::new(),
                )
                .map(|projection| OwnerCallResultPathRow {
                    call: call.id,
                    anchor: local_declaration_ref(*declaration),
                    projection,
                })
            })
            .collect();
        Ok(derived)
    }

    fn derive_occurrence_rows(
        &self,
        expressions: &[OwnerExpressionRow],
    ) -> Result<Vec<OwnerOccurrenceRow>, CheckedOwnerBuildError> {
        let mut occurrences = Vec::with_capacity(
            self.declaration_specs.len() + self.call_occurrences.len() + expressions.len(),
        );
        let mut declaration_occurrences = BTreeSet::new();
        let mut push_declaration = |declaration: OwnerDeclarationId| {
            if !declaration_occurrences.insert(declaration) {
                return Ok(());
            }
            let row = self
                .declaration_specs
                .get(declaration.0 as usize)
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new("owner declaration occurrence has no finalized row")
                })?;
            occurrences.push(OwnerOccurrenceRow {
                target: local_declaration_ref(declaration),
                kind: SemanticOccurrenceKind::Declaration,
                source: row.source.clone(),
            });
            Ok::<_, CheckedOwnerBuildError>(())
        };
        if let Some(public) = self
            .declaration_ids
            .get(&OwnerDeclarationStableKey::Public)
            .copied()
        {
            push_declaration(public)?;
        }
        for declaration in self.parameter_declarations.values().copied() {
            push_declaration(declaration)?;
        }
        for declaration in self.statement_declarations.values().copied() {
            push_declaration(declaration)?;
        }
        for declaration in self.record_field_declarations.values().copied() {
            push_declaration(declaration)?;
        }
        for binding in &self.pattern_bindings {
            push_declaration(binding.declaration)?;
        }
        drop(push_declaration);
        for (index, declaration) in self.declaration_specs.iter().enumerate() {
            let declaration = declaration.as_ref().ok_or_else(|| {
                CheckedOwnerBuildError::new("owner declaration occurrence has no finalized row")
            })?;
            let id = OwnerDeclarationId(checked_u32(index, "owner declaration occurrence")?);
            if declaration_occurrences.contains(&id)
                || matches!(
                    declaration.stable_key,
                    OwnerDeclarationStableKey::FreshOut { .. }
                        | OwnerDeclarationStableKey::CallContext { .. }
                )
            {
                continue;
            }
            return Err(CheckedOwnerBuildError::new(format!(
                "owner declaration {id:?} has no exact occurrence construction lane"
            )));
        }
        occurrences.extend(self.call_occurrences.iter().cloned());
        occurrences.extend(expressions.iter().filter_map(|expression| {
            let target = match &expression.kind {
                OwnerExpressionKind::Read { target, .. }
                | OwnerExpressionKind::Drain { target, .. } => target.clone(),
                _ => return None,
            };
            Some(OwnerOccurrenceRow {
                target,
                kind: SemanticOccurrenceKind::Read,
                source: expression.source.clone(),
            })
        }));
        Ok(occurrences)
    }

    fn local_scope_key(
        &self,
        scope: OwnerScopeId,
    ) -> Result<&OwnerScopeStableKey, CheckedOwnerBuildError> {
        self.scope_specs
            .get(scope.0 as usize)
            .and_then(Option::as_ref)
            .map(|scope| &scope.stable_key)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner local scope has no stable key"))
    }

    fn local_declaration_key(
        &self,
        declaration: OwnerDeclarationId,
    ) -> Result<&OwnerDeclarationStableKey, CheckedOwnerBuildError> {
        self.declaration_specs
            .get(declaration.0 as usize)
            .and_then(Option::as_ref)
            .map(|declaration| &declaration.stable_key)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner local declaration has no stable key"))
    }

    fn local_expression_key(
        &self,
        expression: OwnerExpressionId,
    ) -> Result<&StableExpressionKey, CheckedOwnerBuildError> {
        self.syntax
            .expressions
            .get(expression.0 as usize)
            .map(|expression| &expression.stable_key)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner local expression has no stable key"))
    }

    fn local_statement_key(
        &self,
        statement: OwnerStatementId,
    ) -> Result<&boon_syntax::StableStatementKey, CheckedOwnerBuildError> {
        self.syntax
            .statements
            .get(statement.0 as usize)
            .map(|statement| &statement.stable_key)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner local statement has no stable key"))
    }

    fn local_call_key(
        &self,
        call: OwnerCallId,
    ) -> Result<&StableExpressionKey, CheckedOwnerBuildError> {
        self.call_rows
            .get(call.0 as usize)
            .filter(|row| row.id == call)
            .map(|row| &row.stable_key)
            .ok_or_else(|| CheckedOwnerBuildError::new("owner local call has no stable key"))
    }

    fn normalize_scope_ref(
        &self,
        scope: &OwnerScopeRef,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match scope {
            OwnerScopeRef::Local { scope } => json!({
                "kind": "local",
                "scope": self.local_scope_key(*scope)?,
            }),
            OwnerScopeRef::Imported { owner, scope } => {
                relocations.push(OwnerRelocationTarget::Scope {
                    owner: owner.clone(),
                    scope: scope.clone(),
                });
                json!({"kind": "imported", "owner": owner, "scope": scope})
            }
            OwnerScopeRef::ProjectRoot => {
                relocations.push(OwnerRelocationTarget::ProjectRootScope);
                json!({"kind": "project_root"})
            }
        })
    }

    fn normalize_declaration_ref(
        &self,
        declaration: &OwnerDeclarationRef,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match declaration {
            OwnerDeclarationRef::Local { declaration } => json!({
                "kind": "local",
                "declaration": self.local_declaration_key(*declaration)?,
            }),
            OwnerDeclarationRef::Imported { owner, member } => {
                relocations.push(OwnerRelocationTarget::Declaration {
                    owner: owner.clone(),
                    member: member.clone(),
                });
                json!({"kind": "imported", "owner": owner, "member": member})
            }
            OwnerDeclarationRef::ImportedStable { owner, declaration } => {
                relocations.push(OwnerRelocationTarget::StableDeclaration {
                    owner: owner.clone(),
                    declaration: declaration.clone(),
                });
                json!({
                    "kind": "imported_stable",
                    "owner": owner,
                    "declaration": declaration,
                })
            }
            OwnerDeclarationRef::Abi {
                canonical_name,
                declaration,
                member,
            } => {
                relocations.push(OwnerRelocationTarget::AbiDeclaration {
                    canonical_name: canonical_name.clone(),
                    declaration: *declaration,
                    member: *member,
                });
                json!({
                    "kind": "abi",
                    "canonical_name": canonical_name,
                    "declaration": declaration,
                    "member": member,
                })
            }
            OwnerDeclarationRef::ScopeOwner { scope } => json!({
                "kind": "scope_owner",
                "scope": self.normalize_scope_ref(scope, relocations)?,
            }),
        })
    }

    fn normalize_expression_ref(
        &self,
        expression: &OwnerExpressionRef,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match expression {
            OwnerExpressionRef::Local { expression } => json!({
                "kind": "local",
                "expression": self.local_expression_key(*expression)?,
            }),
            OwnerExpressionRef::Child { owner, expression } => {
                relocations.push(OwnerRelocationTarget::ChildExpression {
                    owner: owner.clone(),
                    expression: expression.clone(),
                });
                json!({"kind": "child", "owner": owner, "expression": expression})
            }
        })
    }

    fn normalize_context_formal_ref(
        &self,
        formal: &OwnerContextFormalRef,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match formal {
            OwnerContextFormalRef::Local { formal } => {
                if formal.0 != 0 || self.own_interface.context.is_none() {
                    return Err(CheckedOwnerBuildError::new(
                        "owner local context formal is not defined",
                    ));
                }
                json!({"kind": "local", "owner": self.syntax.owner})
            }
            OwnerContextFormalRef::Imported { owner } => {
                relocations.push(OwnerRelocationTarget::ContextFormal {
                    owner: owner.clone(),
                });
                json!({"kind": "imported", "owner": owner})
            }
        })
    }

    fn normalize_source_ref(
        &self,
        source: &boon_checked::OwnerSourceRef,
        rows: &CheckedOwnerRows,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match source {
            boon_checked::OwnerSourceRef::Local { source } => {
                let stable_key = &rows
                    .sources
                    .get(source.0 as usize)
                    .filter(|row| row.id == *source)
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new("owner source read seed is missing")
                    })?
                    .stable_key;
                json!({"kind": "local", "source": stable_key})
            }
            boon_checked::OwnerSourceRef::Imported { source } => {
                relocations.push(OwnerRelocationTarget::Source {
                    source: source.clone(),
                });
                json!({"kind": "imported", "source": source})
            }
        })
    }

    fn normalize_record_fields(
        &self,
        fields: &[OwnerRecordField],
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Vec<Value>, CheckedOwnerBuildError> {
        fields
            .iter()
            .map(|field| {
                Ok(json!({
                    "declaration": field
                        .declaration
                        .as_ref()
                        .map(|declaration| self.normalize_declaration_ref(declaration, relocations))
                        .transpose()?,
                    "name": field.name,
                    "value": self.normalize_expression_ref(&field.value, relocations)?,
                    "spread": field.spread,
                    "source": field.source,
                }))
            })
            .collect()
    }

    fn normalize_expression_kind(
        &self,
        kind: &OwnerExpressionKind,
        rows: &CheckedOwnerRows,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        let expressions = |values: &[OwnerExpressionRef], relocations: &mut Vec<_>| {
            values
                .iter()
                .map(|value| self.normalize_expression_ref(value, relocations))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(match kind {
            OwnerExpressionKind::Read {
                target,
                projection,
                source_seed,
            } => json!({
                "kind": "read",
                "target": self.normalize_declaration_ref(target, relocations)?,
                "projection": projection,
                "source_seed": source_seed
                    .as_ref()
                    .map(|seed| {
                        Ok::<Value, CheckedOwnerBuildError>(json!({
                            "source": self.normalize_source_ref(
                                &seed.source,
                                rows,
                                relocations,
                            )?,
                            "payload_projection": seed.payload_projection,
                        }))
                    })
                    .transpose()?,
            }),
            OwnerExpressionKind::Passed {
                formal,
                projection,
                access,
            } => json!({
                "kind": "passed",
                "formal": self.normalize_context_formal_ref(formal, relocations)?,
                "projection": projection,
                "access": access,
            }),
            OwnerExpressionKind::ExternalRead {
                canonical_path,
                declaration,
            } => {
                if let Some(declaration) = declaration {
                    relocations.push(OwnerRelocationTarget::AbiDeclaration {
                        canonical_name: canonical_path.clone(),
                        declaration: *declaration,
                        member: OwnerAbiMemberRef::Declaration,
                    });
                }
                json!({
                    "kind": "external_read",
                    "canonical_path": canonical_path,
                    "declaration": declaration,
                })
            }
            OwnerExpressionKind::Drain { target, projection } => json!({
                "kind": "drain",
                "target": self.normalize_declaration_ref(target, relocations)?,
                "projection": projection,
            }),
            OwnerExpressionKind::Text { value } => json!({"kind": "text", "value": value}),
            OwnerExpressionKind::TextTemplate { segments } => json!({
                "kind": "text_template",
                "segments": segments
                    .iter()
                    .map(|segment| match segment {
                        OwnerTextSegment::Static { value } => Ok(json!({
                            "kind": "static",
                            "value": value,
                        })),
                        OwnerTextSegment::Dynamic { value } => Ok(json!({
                            "kind": "dynamic",
                            "value": self.normalize_expression_ref(value, relocations)?,
                        })),
                    })
                    .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?,
            }),
            OwnerExpressionKind::Number { value } => {
                json!({"kind": "number", "value": value})
            }
            OwnerExpressionKind::BytesByte { value } => {
                json!({"kind": "bytes_byte", "value": value})
            }
            OwnerExpressionKind::Absent => json!({"kind": "absent"}),
            OwnerExpressionKind::Flush { payload } => json!({
                "kind": "flush",
                "payload": self.normalize_expression_ref(payload, relocations)?,
            }),
            OwnerExpressionKind::Tag { name } => json!({"kind": "tag", "name": name}),
            OwnerExpressionKind::TaggedObject { tag, fields } => json!({
                "kind": "tagged_object",
                "tag": tag,
                "fields": self.normalize_record_fields(fields, relocations)?,
            }),
            OwnerExpressionKind::Source => json!({"kind": "source"}),
            OwnerExpressionKind::Call { call } => json!({
                "kind": "call",
                "call": self.local_call_key(*call)?,
            }),
            OwnerExpressionKind::Draining { input } => json!({
                "kind": "draining",
                "input": self.normalize_expression_ref(input, relocations)?,
            }),
            OwnerExpressionKind::Hold { initial, name } => json!({
                "kind": "hold",
                "initial": self.normalize_expression_ref(initial, relocations)?,
                "name": name,
            }),
            OwnerExpressionKind::Latest { branches } => json!({
                "kind": "latest",
                "branches": expressions(branches, relocations)?,
            }),
            OwnerExpressionKind::When { input, arms } => json!({
                "kind": "when",
                "input": self.normalize_expression_ref(input, relocations)?,
                "arms": expressions(arms, relocations)?,
            }),
            OwnerExpressionKind::While { input, arms } => json!({
                "kind": "while",
                "input": self.normalize_expression_ref(input, relocations)?,
                "arms": expressions(arms, relocations)?,
            }),
            OwnerExpressionKind::Then { input, output } => json!({
                "kind": "then",
                "input": self.normalize_expression_ref(input, relocations)?,
                "output": output
                    .as_ref()
                    .map(|output| self.normalize_expression_ref(output, relocations))
                    .transpose()?,
            }),
            OwnerExpressionKind::Infix { left, op, right } => json!({
                "kind": "infix",
                "left": self.normalize_expression_ref(left, relocations)?,
                "op": op,
                "right": self.normalize_expression_ref(right, relocations)?,
            }),
            OwnerExpressionKind::MatchArm {
                pattern,
                bindings,
                output,
            } => json!({
                "kind": "match_arm",
                "pattern": pattern,
                "bindings": bindings
                    .iter()
                    .map(|binding| self.local_declaration_key(*binding))
                    .collect::<Result<Vec<_>, _>>()?,
                "output": output
                    .as_ref()
                    .map(|output| self.normalize_expression_ref(output, relocations))
                    .transpose()?,
            }),
            OwnerExpressionKind::Block { bindings, result } => json!({
                "kind": "block",
                "bindings": bindings
                    .iter()
                    .map(|binding| Ok(json!({
                        "declaration": self.normalize_declaration_ref(
                            &binding.declaration,
                            relocations,
                        )?,
                        "value": self.normalize_expression_ref(&binding.value, relocations)?,
                        "source": binding.source,
                    })))
                    .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?,
                "result": result
                    .as_ref()
                    .map(|result| self.normalize_expression_ref(result, relocations))
                    .transpose()?,
            }),
            OwnerExpressionKind::Object { fields } => json!({
                "kind": "object",
                "fields": self.normalize_record_fields(fields, relocations)?,
            }),
            OwnerExpressionKind::List { capacity, items } => json!({
                "kind": "list",
                "capacity": capacity,
                "items": expressions(items, relocations)?,
            }),
            OwnerExpressionKind::Bytes { fixed_size, items } => json!({
                "kind": "bytes",
                "fixed_size": fixed_size,
                "items": expressions(items, relocations)?,
            }),
            OwnerExpressionKind::Delimiter => json!({"kind": "delimiter"}),
            OwnerExpressionKind::Invalid { tokens } => {
                json!({"kind": "invalid", "tokens": tokens})
            }
            OwnerExpressionKind::MapEntry { key, value } => json!({
                "kind": "map_entry",
                "key": self.normalize_expression_ref(key, relocations)?,
                "value": self.normalize_expression_ref(value, relocations)?,
            }),
            OwnerExpressionKind::Map { entries } => json!({
                "kind": "map",
                "entries": expressions(entries, relocations)?,
            }),
            OwnerExpressionKind::Set { items } => json!({
                "kind": "set",
                "items": expressions(items, relocations)?,
            }),
            OwnerExpressionKind::Bits { value } => json!({"kind": "bits", "value": value}),
        })
    }

    fn normalize_statement_kind(
        &self,
        kind: &OwnerStatementKind,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match kind {
            OwnerStatementKind::Function { declaration } => json!({
                "kind": "function",
                "declaration": self.local_declaration_key(*declaration)?,
            }),
            OwnerStatementKind::Field { declaration } => json!({
                "kind": "field",
                "declaration": self.local_declaration_key(*declaration)?,
            }),
            OwnerStatementKind::Source { declaration, event } => json!({
                "kind": "source",
                "declaration": declaration
                    .map(|declaration| self.local_declaration_key(declaration))
                    .transpose()?,
                "event": event,
            }),
            OwnerStatementKind::Hold { declaration, name } => json!({
                "kind": "hold",
                "declaration": declaration
                    .map(|declaration| self.local_declaration_key(declaration))
                    .transpose()?,
                "name": name,
            }),
            OwnerStatementKind::List {
                declaration,
                capacity,
            } => json!({
                "kind": "list",
                "declaration": declaration
                    .map(|declaration| self.local_declaration_key(declaration))
                    .transpose()?,
                "capacity": capacity,
            }),
            OwnerStatementKind::Block => json!({"kind": "block"}),
            OwnerStatementKind::Spread => json!({"kind": "spread"}),
            OwnerStatementKind::Expression => json!({"kind": "expression"}),
        })
    }

    fn normalize_call_entry(
        &self,
        entry: &OwnerCallEntry,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match entry {
            OwnerCallEntry::Input {
                formal,
                name,
                value,
                from_pipe,
                evaluation_scope,
            } => json!({
                "kind": "input",
                "formal": self.normalize_declaration_ref(formal, relocations)?,
                "name": name,
                "value": self.normalize_expression_ref(value, relocations)?,
                "from_pipe": from_pipe,
                "evaluation_scope": match evaluation_scope {
                    OwnerEvaluationScope::Parent => json!({"kind": "parent"}),
                    OwnerEvaluationScope::Output { formal } => json!({
                        "kind": "output",
                        "formal": self.normalize_declaration_ref(formal, relocations)?,
                    }),
                },
            }),
            OwnerCallEntry::FreshOut {
                formal,
                name,
                output,
                scope_id,
            } => json!({
                "kind": "fresh_out",
                "formal": self.normalize_declaration_ref(formal, relocations)?,
                "name": name,
                "output": self.local_declaration_key(*output)?,
                "scope": self.local_scope_key(*scope_id)?,
            }),
            OwnerCallEntry::ForwardOut {
                formal,
                name,
                target,
                target_name,
            } => json!({
                "kind": "forward_out",
                "formal": self.normalize_declaration_ref(formal, relocations)?,
                "name": name,
                "target": self.normalize_declaration_ref(target, relocations)?,
                "target_name": target_name,
            }),
        })
    }

    fn normalize_context_binding(
        &self,
        binding: &OwnerContextBinding,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match binding {
            OwnerContextBinding::Explicit { value, source } => json!({
                "kind": "explicit",
                "value": self.normalize_expression_ref(value, relocations)?,
                "source": source,
            }),
            OwnerContextBinding::Inherited { formal } => json!({
                "kind": "inherited",
                "formal": self.normalize_context_formal_ref(formal, relocations)?,
            }),
            OwnerContextBinding::None => json!({"kind": "none"}),
        })
    }

    fn normalized_scope_payload(
        &self,
        row: &OwnerScopeRow,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(json!({
            "parent": row
                .parent
                .as_ref()
                .map(|parent| self.normalize_scope_ref(parent, relocations))
                .transpose()?,
            "owner": row
                .owner
                .as_ref()
                .map(|owner| self.normalize_declaration_ref(owner, relocations))
                .transpose()?,
            "kind": row.kind,
            "source": row.source,
        }))
    }

    fn normalized_declaration_payload(
        &self,
        row: &OwnerDeclarationRow,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(json!({
            "scope": self.normalize_scope_ref(&row.scope, relocations)?,
            "name": row.name,
            "kind": row.kind,
            "flow_type": row.flow_type,
            "value": row
                .value
                .as_ref()
                .map(|value| self.normalize_expression_ref(value, relocations))
                .transpose()?,
            "body_scope": row
                .body_scope
                .map(|scope| self.local_scope_key(scope))
                .transpose()?,
            "source": row.source,
        }))
    }

    fn normalized_statement_payload(
        &self,
        row: &OwnerStatementRow,
        rows: &CheckedOwnerRows,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        let children = row
            .children
            .iter()
            .map(|child| match child {
                boon_checked::OwnerStatementChild::Local { statement } => Ok(json!({
                    "kind": "local",
                    "statement": self.local_statement_key(*statement)?,
                })),
                boon_checked::OwnerStatementChild::Owner { owner } => {
                    relocations.push(OwnerRelocationTarget::ChildOwner {
                        owner: owner.clone(),
                    });
                    Ok(json!({
                        "kind": "owner",
                        "owner": owner,
                    }))
                }
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        let resources = row
            .resources
            .iter()
            .map(|resource| {
                Ok(match resource {
                    OwnerResourceBinding::Source { source } => match source {
                        boon_checked::OwnerSourceRef::Local { source } => json!({
                            "kind": "source",
                            "source": &rows
                                .sources
                                .get(source.0 as usize)
                                .filter(|row| row.id == *source)
                                .ok_or_else(|| CheckedOwnerBuildError::new("owner source binding is missing"))?
                                .stable_key,
                        }),
                        boon_checked::OwnerSourceRef::Imported { source } => {
                            relocations.push(OwnerRelocationTarget::Source {
                                source: source.clone(),
                            });
                            json!({"kind": "source", "source": source})
                        }
                    },
                    OwnerResourceBinding::State { state } => {
                        let state = rows
                            .states
                            .get(state.0 as usize)
                            .filter(|row| row.id == *state)
                            .ok_or_else(|| CheckedOwnerBuildError::new("owner state binding is missing"))?;
                        json!({
                            "kind": "state",
                            "expression": self.local_expression_key(state.expression)?,
                            "state_kind": state.kind,
                        })
                    }
                    OwnerResourceBinding::ListAuthority { list } => {
                        let list = rows
                            .lists
                            .get(list.0 as usize)
                            .filter(|row| row.id == *list)
                            .ok_or_else(|| CheckedOwnerBuildError::new("owner list binding is missing"))?;
                        json!({
                            "kind": "list_authority",
                            "producer": self.local_expression_key(list.producer)?,
                        })
                    }
                    OwnerResourceBinding::ListAlias { target } => json!({
                        "kind": "list_alias",
                        "target": self.normalize_declaration_ref(target, relocations)?,
                    }),
                })
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        Ok(json!({
            "scope": self.normalize_scope_ref(&row.scope, relocations)?,
            "kind": self.normalize_statement_kind(&row.kind)?,
            "resources": resources,
            "value": row
                .value
                .as_ref()
                .map(|value| self.normalize_expression_ref(value, relocations))
                .transpose()?,
            "value_use": row.value_use,
            "children": children,
            "source": row.source,
        }))
    }

    fn normalized_expression_payload(
        &self,
        row: &OwnerExpressionRow,
        rows: &CheckedOwnerRows,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(json!({
            "scope": self.normalize_scope_ref(&row.scope, relocations)?,
            "declaration": row
                .declaration
                .as_ref()
                .map(|declaration| self.normalize_declaration_ref(declaration, relocations))
                .transpose()?,
            "flow_type": row.flow_type,
            "flush_type": row.flush_type,
            "effect": row.effect,
            "kind": self.normalize_expression_kind(&row.kind, rows, relocations)?,
            "source": row.source,
        }))
    }

    fn normalized_callable_payload(
        &self,
        row: &OwnerCallableRow,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        let parameters = row
            .parameters
            .iter()
            .map(|parameter| {
                Ok(json!({
                    "declaration": self.local_declaration_key(parameter.declaration)?,
                    "name": parameter.name,
                    "kind": parameter.kind,
                    "ordinal": parameter.ordinal,
                    "flow_type": parameter.flow_type,
                    "requirement": parameter.requirement,
                    "evaluation_scope": match &parameter.evaluation_scope {
                        OwnerEvaluationScope::Parent => json!({"kind": "parent"}),
                        OwnerEvaluationScope::Output { formal } => json!({
                            "kind": "output",
                            "formal": self.normalize_declaration_ref(formal, relocations)?,
                        }),
                    },
                    "source": parameter.source,
                }))
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        let contexts = row
            .contexts
            .iter()
            .map(|context| {
                Ok(json!({
                    "name": context.name,
                    "kind": context.kind,
                    "provider": self.normalize_declaration_ref(&context.provider, relocations)?,
                    "flow_type": context.flow_type,
                }))
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        Ok(json!({
            "declaration": self.local_declaration_key(row.declaration)?,
            "scope": self.normalize_scope_ref(&row.scope, relocations)?,
            "kind": row.kind,
            "name": row.name,
            "intrinsic": row.intrinsic,
            "external_identity": row.external_identity,
            "parameters": parameters,
            "contexts": contexts,
            "context_formal": row.context_formal.map(|formal| {
                if formal.0 == 0 { json!({"owner": self.syntax.owner}) } else { Value::Null }
            }),
            "result": row.result,
            "role": row.role,
            "effect": row.effect,
            "body": row.body.map(|body| self.local_statement_key(body)).transpose()?,
            "result_expression": row
                .result_expression
                .as_ref()
                .map(|result| self.normalize_expression_ref(result, relocations))
                .transpose()?,
            "contextual_operation": row.contextual_operation,
        }))
    }

    fn normalized_call_payload(
        &self,
        row: &OwnerCallRow,
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(json!({
            "expression": self.local_expression_key(row.expression)?,
            "callable": self.normalize_declaration_ref(&row.callable, relocations)?,
            "owner_callable": row
                .owner_callable
                .as_ref()
                .map(|owner| self.normalize_declaration_ref(owner, relocations))
                .transpose()?,
            "function": row.function,
            "intrinsic": row.intrinsic,
            "entries": row
                .entries
                .iter()
                .map(|entry| self.normalize_call_entry(entry, relocations))
                .collect::<Result<Vec<_>, _>>()?,
            "contexts": row
                .contexts
                .iter()
                .map(|context| Ok(json!({
                    "declaration": self.local_declaration_key(context.declaration)?,
                    "context_ordinal": context.context_ordinal,
                    "scope": self.local_scope_key(context.scope_id)?,
                })))
                .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?,
            "context_binding": self.normalize_context_binding(&row.context_binding, relocations)?,
            "contextual_substitutions": row
                .contextual_substitutions
                .iter()
                .map(|substitution| {
                    Ok(json!({
                        "formal": self.normalize_context_formal_ref(
                            &substitution.formal,
                            relocations,
                        )?,
                        "variable": substitution.variable,
                        "value": substitution.value,
                    }))
                })
                .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?,
            "type_substitutions": row.type_substitutions,
            "syntax_discriminated_result": row.syntax_discriminated_result,
            "result": row.result,
            "role": row.role,
            "source": row.source,
        }))
    }

    fn normalized_diagnostic_site(
        &self,
        site: &OwnerSourceAnchorSite,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(match site {
            OwnerSourceAnchorSite::Statement { statement } => json!({
                "kind": "statement",
                "statement": self.local_statement_key(OwnerStatementId(*statement))?,
            }),
            OwnerSourceAnchorSite::Expression { expression } => json!({
                "kind": "expression",
                "expression": expression,
            }),
        })
    }

    fn build_base_rows(
        self,
    ) -> Result<
        (
            CheckedOwnerRows,
            OwnerCheckedReceiptSet,
            Box<[crate::OwnerDiagnosticTemplate]>,
        ),
        CheckedOwnerBuildError,
    > {
        let mut statements = self.build_statement_rows()?;
        let expressions = self.build_expression_rows()?;
        let mut derived = self.derive_resource_rows(&mut statements, &expressions)?;
        derived.occurrences = self.derive_occurrence_rows(&expressions)?;
        let callables = self.build_callable_rows()?;
        let context_formals = self.build_context_formal_rows()?;
        let calls = self.call_rows.clone();
        let pattern_bindings = self.pattern_bindings.clone();
        let mut rows = CheckedOwnerRows::default();
        rows.scopes = self
            .scope_specs
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let spec = spec.as_ref().ok_or_else(|| {
                    CheckedOwnerBuildError::new("owner scope reservation was never defined")
                })?;
                Ok(OwnerScopeRow {
                    id: OwnerScopeId(checked_u32(index, "owner scope row")?),
                    stable_key: spec.stable_key.clone(),
                    parent: spec.parent.clone(),
                    owner: spec.owner.clone(),
                    kind: spec.kind,
                    source: spec.source.clone(),
                })
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        rows.declarations = self
            .declaration_specs
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let spec = spec.as_ref().ok_or_else(|| {
                    CheckedOwnerBuildError::new("owner declaration reservation was never defined")
                })?;
                Ok(OwnerDeclarationRow {
                    id: OwnerDeclarationId(checked_u32(index, "owner declaration row")?),
                    stable_key: spec.stable_key.clone(),
                    scope: spec.scope.clone(),
                    name: spec.name.clone(),
                    kind: spec.kind,
                    flow_type: spec.flow_type.clone(),
                    value: spec.value.clone(),
                    body_scope: spec.body_scope,
                    source: spec.source.clone(),
                })
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        rows.statements = statements;
        rows.expressions = expressions;
        rows.callables = callables;
        rows.context_formals = context_formals;
        rows.calls = calls;
        rows.call_result_paths = derived.call_result_paths;
        rows.pattern_bindings = pattern_bindings;
        rows.resource_projection_seeds = derived.resource_projection_seeds;
        rows.sources = derived.sources;
        rows.states = derived.states;
        rows.lists = derived.lists;
        rows.occurrences = derived.occurrences;

        let mut sink = OwnerCheckedReceiptSink::new();
        for row in &rows.scopes {
            let mut relocations = Vec::new();
            let payload = self.normalized_scope_payload(row, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Scope,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.declarations {
            let mut relocations = Vec::new();
            let payload = self.normalized_declaration_payload(row, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Declaration,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.statements {
            let mut relocations = Vec::new();
            let payload = self.normalized_statement_payload(row, &rows, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Statement,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.expressions {
            let mut relocations = Vec::new();
            let payload = self.normalized_expression_payload(row, &rows, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Expression,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.callables {
            let mut relocations = Vec::new();
            let stable_key = self.local_declaration_key(row.declaration)?;
            let payload = self.normalized_callable_payload(row, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Callable,
                stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.context_formals {
            let stable_key = json!({
                "owner": self.syntax.owner,
                "context_formal": row.id.0,
            });
            let payload = json!({
                "callable": self.local_declaration_key(row.callable)?,
                "flow_type": row.flow_type,
                "projections": row.projections,
            });
            sink.record(
                OwnerCheckedRowDomain::ContextFormal,
                &stable_key,
                &payload,
                std::iter::empty(),
            )?;
        }
        for row in &rows.calls {
            let mut relocations = Vec::new();
            let payload = self.normalized_call_payload(row, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Call,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.call_result_paths {
            let mut relocations = Vec::new();
            let stable_key = self.local_call_key(row.call)?;
            let payload = json!({
                "call": stable_key,
                "anchor": self.normalize_declaration_ref(&row.anchor, &mut relocations)?,
                "projection": row.projection,
            });
            sink.record(
                OwnerCheckedRowDomain::CallResultPath,
                stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.pattern_bindings {
            let mut relocations = Vec::new();
            let stable_key = self.local_declaration_key(row.declaration)?;
            let payload = json!({
                "declaration": stable_key,
                "selector": self.normalize_expression_ref(&row.selector, &mut relocations)?,
                "projection": row.projection,
            });
            sink.record(
                OwnerCheckedRowDomain::PatternBinding,
                stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.resource_projection_seeds {
            let mut relocations = Vec::new();
            let stable_key = self.local_expression_key(row.expression)?;
            let payload = json!({
                "expression": stable_key,
                "target": self.normalize_declaration_ref(&row.target, &mut relocations)?,
                "projection": row.projection,
                "required_type": row.required_type,
            });
            sink.record(
                OwnerCheckedRowDomain::ResourceProjection,
                stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.sources {
            let mut relocations = Vec::new();
            let payload = json!({
                "declaration": self.normalize_declaration_ref(&row.declaration, &mut relocations)?,
                "statement": self.local_statement_key(row.statement)?,
                "expression": self.local_expression_key(row.expression)?,
                "owner_scope": self.normalize_scope_ref(&row.owner_scope, &mut relocations)?,
                "path": {
                    "anchor": self.normalize_declaration_ref(&row.path.anchor, &mut relocations)?,
                    "projection": row.path.projection,
                },
                "interval_ms": row.interval_ms,
                "payload_type": row.payload_type,
                "source": row.source,
            });
            sink.record(
                OwnerCheckedRowDomain::Source,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.states {
            let mut relocations = Vec::new();
            let stable_key = json!({
                "expression": self.local_expression_key(row.expression)?,
                "kind": row.kind,
            });
            let payload = json!({
                "declaration": self.normalize_declaration_ref(&row.declaration, &mut relocations)?,
                "statement": self.local_statement_key(row.statement)?,
                "expression": self.local_expression_key(row.expression)?,
                "initial": self.normalize_expression_ref(&row.initial, &mut relocations)?,
                "owner_scope": self.normalize_scope_ref(&row.owner_scope, &mut relocations)?,
                "path": {
                    "anchor": self.normalize_declaration_ref(&row.path.anchor, &mut relocations)?,
                    "projection": row.path.projection,
                },
                "kind": row.kind,
                "flow_type": row.flow_type,
                "source": row.source,
            });
            sink.record(
                OwnerCheckedRowDomain::State,
                &stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.lists {
            let mut relocations = Vec::new();
            let stable_key = self.local_expression_key(row.producer)?;
            let payload = json!({
                "declaration": self.normalize_declaration_ref(&row.declaration, &mut relocations)?,
                "statement": self.local_statement_key(row.statement)?,
                "producer": stable_key,
                "owner_scope": self.normalize_scope_ref(&row.owner_scope, &mut relocations)?,
                "path": {
                    "anchor": self.normalize_declaration_ref(&row.path.anchor, &mut relocations)?,
                    "projection": row.path.projection,
                },
                "item_type": row.item_type,
                "capacity": row.capacity,
                "key_policy": row.key_policy,
                "source": row.source,
            });
            sink.record(
                OwnerCheckedRowDomain::List,
                stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.occurrences {
            let mut relocations = Vec::new();
            let target = self.normalize_declaration_ref(&row.target, &mut relocations)?;
            let stable_key = json!({
                "target": target,
                "kind": row.kind,
                "source": row.source,
            });
            let payload = json!({
                "target": stable_key["target"],
                "kind": row.kind,
                "source": row.source,
            });
            sink.record(
                OwnerCheckedRowDomain::Occurrence,
                &stable_key,
                &payload,
                relocations,
            )?;
        }
        for diagnostic in &self.diagnostics {
            let site = self.normalized_diagnostic_site(&diagnostic.site)?;
            let stable_key = json!({
                "site": site,
                "role": diagnostic.role,
                "code": diagnostic.code,
                "message": diagnostic.message,
            });
            let payload = json!({
                "severity": diagnostic.severity,
                "code": diagnostic.code,
                "message": diagnostic.message,
                "site": stable_key["site"],
                "role": diagnostic.role,
            });
            sink.record(
                OwnerCheckedRowDomain::Diagnostic,
                &stable_key,
                &payload,
                std::iter::empty(),
            )?;
        }
        let receipts = sink.finish()?;
        Ok((rows, receipts, self.diagnostics.into_boxed_slice()))
    }

    fn build_callable_rows(&self) -> Result<Vec<OwnerCallableRow>, CheckedOwnerBuildError> {
        if self.own_interface.declaration_kind != Some(OwnerDeclarationKind::Function) {
            return Ok(Vec::new());
        }
        let root =
            self.syntax.statements.first().ok_or_else(|| {
                CheckedOwnerBuildError::new("callable owner has no root statement")
            })?;
        let declaration = self
            .declaration_ids
            .get(&OwnerDeclarationStableKey::Public)
            .copied()
            .ok_or_else(|| CheckedOwnerBuildError::new("callable declaration is missing"))?;
        let scope = self
            .statement_body_scopes
            .get(&OwnerStatementId(root.id))
            .copied()
            .ok_or_else(|| CheckedOwnerBuildError::new("callable body scope is missing"))?;
        let parameters = self
            .own_interface
            .parameters
            .iter()
            .map(|parameter| {
                let declaration = self.parameter_declarations[&parameter.ordinal];
                Ok(OwnerParameterRow {
                    declaration,
                    name: parameter.name.clone(),
                    kind: match parameter.kind {
                        OwnerParameterKind::Value => CheckedParameterKind::Value,
                        OwnerParameterKind::Out => CheckedParameterKind::Out,
                    },
                    ordinal: parameter.ordinal,
                    flow_type: parameter.flow_type.clone(),
                    requirement: parameter.requirement.clone(),
                    evaluation_scope: match parameter.evaluation_scope {
                        OwnerInterfaceEvaluationScope::Parent => OwnerEvaluationScope::Parent,
                        OwnerInterfaceEvaluationScope::Output { parameter_ordinal } => {
                            OwnerEvaluationScope::Output {
                                formal: local_declaration_ref(
                                    self.parameter_declarations[&parameter_ordinal],
                                ),
                            }
                        }
                    },
                    source: OwnerSourceSite::FunctionParameter {
                        statement: root.stable_key.clone(),
                        ordinal: parameter.ordinal,
                    },
                })
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        let result_expression = self
            .graph
            .statement(OwnerStatementId(root.id))
            .and_then(|statement| statement.canonical_value.clone());
        Ok(vec![OwnerCallableRow {
            declaration,
            scope: local_scope_ref(scope),
            kind: CheckedCallableKind::User,
            name: self
                .own_interface
                .names
                .first()
                .cloned()
                .unwrap_or_default(),
            intrinsic: None,
            external_identity: None,
            parameters,
            contexts: Vec::<OwnerCallableContextRow>::new(),
            context_formal: self
                .own_interface
                .context
                .as_ref()
                .map(|_| OwnerContextFormalId(0)),
            result: self.own_interface.result.clone(),
            role: self.abi.role(),
            effect: self.own_interface.effect,
            body: Some(OwnerStatementId(root.id)),
            result_expression,
            contextual_operation: None,
        }])
    }

    fn build_context_formal_rows(
        &self,
    ) -> Result<Vec<OwnerContextFormalRow>, CheckedOwnerBuildError> {
        let Some(context) = &self.own_interface.context else {
            return Ok(Vec::new());
        };
        if self.own_interface.declaration_kind != Some(crate::OwnerDeclarationKind::Function) {
            return Err(CheckedOwnerBuildError::new(
                "only a FUNCTION owner can publish a context formal",
            ));
        }
        let callable = self
            .declaration_ids
            .get(&OwnerDeclarationStableKey::Public)
            .copied()
            .ok_or_else(|| {
                CheckedOwnerBuildError::new("contextual owner has no callable declaration")
            })?;
        Ok(vec![OwnerContextFormalRow {
            id: OwnerContextFormalId(0),
            callable,
            flow_type: context.flow_type.clone(),
            projections: context
                .projections
                .iter()
                .map(|projection| projection.to_vec())
                .collect(),
        }])
    }

    fn build_statement_rows(&self) -> Result<Vec<OwnerStatementRow>, CheckedOwnerBuildError> {
        self.syntax
            .statements
            .iter()
            .map(|statement| {
                let id = OwnerStatementId(statement.id);
                let declaration = self.statement_declarations.get(&id).copied();
                let kind = match &statement.kind {
                    AstStatementKind::Function { .. } => OwnerStatementKind::Function {
                        declaration: declaration.ok_or_else(|| {
                            CheckedOwnerBuildError::new("function declaration is missing")
                        })?,
                    },
                    AstStatementKind::Field { .. } => OwnerStatementKind::Field {
                        declaration: declaration.ok_or_else(|| {
                            CheckedOwnerBuildError::new("field declaration is missing")
                        })?,
                    },
                    AstStatementKind::Source { event, .. } => OwnerStatementKind::Source {
                        declaration,
                        event: event.clone(),
                    },
                    AstStatementKind::Hold { name, .. } => OwnerStatementKind::Hold {
                        declaration,
                        name: name.clone(),
                    },
                    AstStatementKind::List { capacity, .. } => OwnerStatementKind::List {
                        declaration,
                        capacity: *capacity,
                    },
                    AstStatementKind::Block => OwnerStatementKind::Block,
                    AstStatementKind::Spread => OwnerStatementKind::Spread,
                    AstStatementKind::Expression => OwnerStatementKind::Expression,
                };
                let graph = self.graph.statement(id).ok_or_else(|| {
                    CheckedOwnerBuildError::new("owner syntax graph lost a statement")
                })?;
                Ok(OwnerStatementRow {
                    id,
                    stable_key: statement.stable_key.clone(),
                    scope: self.statement_scopes[id.0 as usize].clone(),
                    kind,
                    resources: Vec::new(),
                    value: graph.canonical_value.clone(),
                    value_use: statement.value_use,
                    children: graph.children.to_vec(),
                    source: statement_source(statement),
                })
            })
            .collect()
    }

    fn build_expression_rows(&self) -> Result<Vec<OwnerExpressionRow>, CheckedOwnerBuildError> {
        self.syntax
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| {
                let inferred = &self.body.expressions[index];
                let id = OwnerExpressionId(checked_u32(index, "owner expression row")?);
                let kind = self.lower_expression_kind(id, &expression.kind)?;
                Ok(OwnerExpressionRow {
                    id,
                    stable_key: expression.stable_key.clone(),
                    scope: self.expression_scopes[index].clone(),
                    declaration: self.expression_declarations[index].clone(),
                    flow_type: inferred.flow_type.clone(),
                    flush_type: inferred.flush_type.clone(),
                    effect: inferred.direct_effect,
                    kind,
                    source: expression_source(expression),
                })
            })
            .collect()
    }

    fn lower_expression_kind(
        &self,
        id: OwnerExpressionId,
        kind: &AstExprKind,
    ) -> Result<OwnerExpressionKind, CheckedOwnerBuildError> {
        if !self
            .expression_owned
            .get(id.0 as usize)
            .copied()
            .unwrap_or(false)
        {
            return Ok(OwnerExpressionKind::Invalid {
                tokens: vec!["unowned_parser_expression".to_owned()],
            });
        }
        let expression = &self.syntax.expressions[id.0 as usize];
        let expression_ref = |reference| owner_expression_ref(self.syntax, reference);
        let fields = |fields: &[boon_syntax::AstRecordField]| {
            fields
                .iter()
                .enumerate()
                .map(|(ordinal, field)| {
                    Ok(OwnerRecordField {
                        declaration: (field.value < self.syntax.expressions.len())
                            .then(|| self.expression_declarations[field.value].clone())
                            .flatten(),
                        name: field.name.clone(),
                        value: expression_ref(field.value)?,
                        spread: field.spread,
                        source: OwnerSourceSite::RecordField {
                            expression: expression.stable_key.clone(),
                            ordinal: checked_u32(ordinal, "record field ordinal")?,
                        },
                    })
                })
                .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()
        };
        Ok(match kind {
            AstExprKind::Identifier(name) => {
                self.lower_read(id, expression, &[name.clone()], false)?
            }
            AstExprKind::Path(parts) => self.lower_read(id, expression, parts, false)?,
            AstExprKind::Drain { path } => {
                let parts = match path {
                    AstDrainPath::Binding { name } => vec![name.clone()],
                    AstDrainPath::Field { binding, fields } => std::iter::once(binding.clone())
                        .chain(fields.iter().cloned())
                        .collect(),
                    AstDrainPath::Passed { fields } => std::iter::once("PASSED".to_owned())
                        .chain(fields.iter().cloned())
                        .collect(),
                };
                self.lower_read(id, expression, &parts, true)?
            }
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                OwnerExpressionKind::Text {
                    value: value.clone(),
                }
            }
            AstExprKind::TextTemplate { segments } => OwnerExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| match segment {
                        AstTextSegment::Static { value } => Ok(OwnerTextSegment::Static {
                            value: value.clone(),
                        }),
                        AstTextSegment::Dynamic { value } => Ok(OwnerTextSegment::Dynamic {
                            value: expression_ref(*value)?,
                        }),
                    })
                    .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?,
            },
            AstExprKind::Number(literal) => ExactNumber::parse_strict(literal, None).map_or_else(
                |_| OwnerExpressionKind::Invalid {
                    tokens: vec!["invalid_exact_number_literal".to_owned()],
                },
                |value| OwnerExpressionKind::Number { value },
            ),
            AstExprKind::BitsLiteral {
                width,
                radix,
                digits,
            } => Bits::parse_encoded(*width, *radix, digits).map_or_else(
                |_| OwnerExpressionKind::Invalid {
                    tokens: vec!["invalid_bits_literal".to_owned()],
                },
                |value| OwnerExpressionKind::Bits { value },
            ),
            AstExprKind::ByteLiteral { value, .. } => {
                OwnerExpressionKind::BytesByte { value: *value }
            }
            AstExprKind::Tag(name) if name == "SKIP" => OwnerExpressionKind::Absent,
            AstExprKind::Flush {
                payload: Some(payload),
            } => OwnerExpressionKind::Flush {
                payload: expression_ref(*payload)?,
            },
            AstExprKind::Flush { payload: None } => OwnerExpressionKind::Invalid {
                tokens: vec!["missing_flush_payload".to_owned()],
            },
            AstExprKind::Tag(name) => OwnerExpressionKind::Tag { name: name.clone() },
            AstExprKind::TaggedObject {
                tag,
                fields: record,
            } => OwnerExpressionKind::TaggedObject {
                tag: tag.clone(),
                fields: fields(record)?,
            },
            AstExprKind::Source => OwnerExpressionKind::Source,
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" => OwnerExpressionKind::While {
                input: exact_linked_input(self.syntax, expression, *input)?,
                arms: arms
                    .iter()
                    .map(|arm| expression_ref(*arm))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. } => self
                .call_ids
                .get(&expression.stable_key)
                .copied()
                .map_or_else(
                    || OwnerExpressionKind::Invalid {
                        tokens: vec!["unbound_owner_call".to_owned()],
                    },
                    |call| OwnerExpressionKind::Call { call },
                ),
            AstExprKind::Draining { input } => OwnerExpressionKind::Draining {
                input: exact_linked_input(self.syntax, expression, *input)?,
            },
            AstExprKind::Hold { initial, name } => OwnerExpressionKind::Hold {
                initial: exact_linked_input(self.syntax, expression, *initial)?,
                name: name.clone(),
            },
            AstExprKind::Latest { branches } => OwnerExpressionKind::Latest {
                branches: branches
                    .iter()
                    .map(|branch| expression_ref(*branch))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            AstExprKind::When { input, arms } => OwnerExpressionKind::When {
                input: exact_linked_input(self.syntax, expression, *input)?,
                arms: arms
                    .iter()
                    .map(|arm| expression_ref(*arm))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            AstExprKind::Then { input, output } => OwnerExpressionKind::Then {
                input: exact_linked_input(self.syntax, expression, *input)?,
                output: output.map(expression_ref).transpose()?,
            },
            AstExprKind::Infix { left, op, right } => OwnerExpressionKind::Infix {
                left: expression_ref(*left)?,
                op: op.clone(),
                right: expression_ref(*right)?,
            },
            AstExprKind::MatchArm { pattern, output } => OwnerExpressionKind::MatchArm {
                pattern: checked_match_pattern(pattern)?,
                bindings: pattern_variable_names(pattern)
                    .into_iter()
                    .map(|name| {
                        self.pattern_declarations
                            .get(&(id, name.clone()))
                            .copied()
                            .ok_or_else(|| {
                                CheckedOwnerBuildError::new(format!(
                                    "owner match arm is missing pattern binding `{name}`"
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                output: output.map(expression_ref).transpose()?,
            },
            AstExprKind::Block { bindings, result } => OwnerExpressionKind::Block {
                bindings: bindings
                    .iter()
                    .enumerate()
                    .map(|(ordinal, binding)| {
                        let declaration = match binding.declaration {
                            AstBlockBindingDeclaration::Local { statement } => {
                                let statement = OwnerStatementId(checked_u32(
                                    statement,
                                    "block binding statement",
                                )?);
                                OwnerDeclarationRef::Local {
                                    declaration: *self
                                        .statement_declarations
                                        .get(&statement)
                                        .ok_or_else(|| {
                                            CheckedOwnerBuildError::new(
                                                "block binding declaration is missing",
                                            )
                                        })?,
                                }
                            }
                            AstBlockBindingDeclaration::Child { child } => {
                                let child =
                                    self.syntax.child_owners.get(child).ok_or_else(|| {
                                        CheckedOwnerBuildError::new(
                                            "block binding child declaration is missing",
                                        )
                                    })?;
                                OwnerDeclarationRef::ImportedStable {
                                    owner: child.owner.clone(),
                                    declaration: OwnerDeclarationStableKey::Public,
                                }
                            }
                        };
                        Ok(OwnerBlockBinding {
                            declaration,
                            value: expression_ref(binding.value)?,
                            source: OwnerSourceSite::BlockBinding {
                                expression: expression.stable_key.clone(),
                                ordinal: checked_u32(ordinal, "block binding ordinal")?,
                            },
                        })
                    })
                    .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?,
                result: result.map(expression_ref).transpose()?,
            },
            AstExprKind::Object(record) => OwnerExpressionKind::Object {
                fields: fields(record)?,
            },
            AstExprKind::ListLiteral { capacity, items } => OwnerExpressionKind::List {
                capacity: *capacity,
                items: items
                    .iter()
                    .map(|item| expression_ref(*item))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            AstExprKind::BytesLiteral { size, items } => OwnerExpressionKind::Bytes {
                fixed_size: match size {
                    BytesSizeSyntax::Fixed(size) => Some(*size),
                    BytesSizeSyntax::Dynamic | BytesSizeSyntax::Infer => None,
                },
                items: items
                    .iter()
                    .map(|item| expression_ref(*item))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            AstExprKind::Delimiter => OwnerExpressionKind::Delimiter,
            AstExprKind::Unknown(tokens) => OwnerExpressionKind::Invalid {
                tokens: tokens.clone(),
            },
            AstExprKind::Arrow { .. } => OwnerExpressionKind::Invalid {
                tokens: vec!["unconsumed_arrow".to_owned()],
            },
            AstExprKind::MapEntry { key, value } => OwnerExpressionKind::MapEntry {
                key: expression_ref(*key)?,
                value: expression_ref(*value)?,
            },
            AstExprKind::MapLiteral { entries } => OwnerExpressionKind::Map {
                entries: entries
                    .iter()
                    .map(|entry| expression_ref(*entry))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            AstExprKind::SetLiteral { items } => OwnerExpressionKind::Set {
                items: items
                    .iter()
                    .map(|item| expression_ref(*item))
                    .collect::<Result<Vec<_>, _>>()?,
            },
        })
    }

    fn lower_read(
        &self,
        id: OwnerExpressionId,
        expression: &crate::OwnerExpressionInput,
        parts: &[String],
        drain: bool,
    ) -> Result<OwnerExpressionKind, CheckedOwnerBuildError> {
        let planned = self
            .body
            .signature_lexical_plan
            .reads()
            .get(id.0 as usize)
            .and_then(Option::as_ref);
        if let Some(fields) = parts.strip_prefix(&["PASSED".to_owned()]) {
            let formal = match planned.map(|read| &read.target) {
                Some(OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Passed,
                }) if self.own_interface.context.is_some() => Some(OwnerContextFormalRef::Local {
                    formal: OwnerContextFormalId(0),
                }),
                Some(OwnerEffectiveLexicalTarget::Imported {
                    target: OwnerLexicalTargetRef::ContextFormal { owner },
                }) => Some(OwnerContextFormalRef::Imported {
                    owner: owner.clone(),
                }),
                _ => None,
            };
            return Ok(if let Some(formal) = formal {
                if planned.is_some_and(|read| read.projection.as_ref() != fields) {
                    return Err(CheckedOwnerBuildError::new(format!(
                        "owner signature PASSED projection diverges for read {id:?}"
                    )));
                }
                OwnerExpressionKind::Passed {
                    formal,
                    projection: fields.to_vec(),
                    access: if drain {
                        CheckedPassedAccess::Drain
                    } else {
                        CheckedPassedAccess::Read
                    },
                }
            } else {
                OwnerExpressionKind::Invalid {
                    tokens: std::iter::once(if drain {
                        "unbound_passed_drain"
                    } else {
                        "unbound_passed_context"
                    })
                    .chain(fields.iter().map(String::as_str))
                    .map(str::to_owned)
                    .collect(),
                }
            });
        }
        if let Some((root, projection)) = parts.split_first() {
            let invalid = match planned.map(|read| &read.target) {
                Some(OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Statement { statement },
                }) if matches!(
                    self.syntax
                        .statements
                        .get(*statement as usize)
                        .map(|statement| &statement.kind),
                    Some(AstStatementKind::Function { .. })
                ) =>
                {
                    Some("function_must_be_called")
                }
                Some(OwnerEffectiveLexicalTarget::Imported {
                    target:
                        OwnerLexicalTargetRef::Declaration {
                            capability: OwnerLexicalDeclarationCapability::CallableOnly,
                            ..
                        },
                }) => Some("function_must_be_called"),
                Some(
                    OwnerEffectiveLexicalTarget::Static {
                        target: OwnerLexicalDeclarationTarget::Ambiguous { .. },
                    }
                    | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                    | OwnerEffectiveLexicalTarget::Ambiguous { .. }
                    | OwnerEffectiveLexicalTarget::Imported {
                        target:
                            OwnerLexicalTargetRef::ContextFormal { .. }
                            | OwnerLexicalTargetRef::Ambiguous { .. },
                    },
                ) => Some("ambiguous_lexical_read"),
                _ => None,
            };
            let lexical = match planned.map(|read| &read.target) {
                Some(OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Parameter { ordinal },
                }) => Some(self
                    .parameter_declarations
                    .get(ordinal)
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner read references missing parameter {ordinal}"
                        ))
                    })
                    .map(local_declaration_ref)?),
                Some(OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::Statement { statement },
                }) => Some(self
                    .statement_declarations
                    .get(&OwnerStatementId(*statement))
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner read references missing statement declaration {statement}"
                        ))
                    })
                    .map(local_declaration_ref)?),
                Some(OwnerEffectiveLexicalTarget::Static {
                    target:
                        OwnerLexicalDeclarationTarget::RecordField {
                            object, ordinal, ..
                        },
                }) => Some(self
                    .record_field_declarations
                    .get(&(OwnerExpressionId(*object), *ordinal))
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner read references missing record field {object}:{ordinal}"
                        ))
                    })
                    .map(local_declaration_ref)?),
                Some(OwnerEffectiveLexicalTarget::Static {
                    target: OwnerLexicalDeclarationTarget::PatternBinding { arm, name },
                }) => Some(self
                    .pattern_declarations
                    .get(&(OwnerExpressionId(*arm), name.clone()))
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(format!(
                            "owner read references missing pattern binding {name:?} in arm {arm}"
                        ))
                    })
                    .map(local_declaration_ref)?),
                Some(OwnerEffectiveLexicalTarget::FreshOut {
                    call,
                    formal_ordinal,
                }) => Some(self
                    .declaration_ids
                    .get(&OwnerDeclarationStableKey::FreshOut {
                        call: call.clone(),
                        formal_ordinal: *formal_ordinal,
                    })
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(
                            "owner read references missing planned FreshOut declaration",
                        )
                    })
                    .map(local_declaration_ref)?),
                Some(OwnerEffectiveLexicalTarget::CallContext {
                    call,
                    context_ordinal,
                }) => Some(self
                    .declaration_ids
                    .get(&OwnerDeclarationStableKey::CallContext {
                        call: call.clone(),
                        ordinal: *context_ordinal,
                    })
                    .copied()
                    .ok_or_else(|| {
                        CheckedOwnerBuildError::new(
                            "owner read references missing planned call-context declaration",
                        )
                    })
                    .map(local_declaration_ref)?),
                Some(OwnerEffectiveLexicalTarget::Imported {
                    target:
                        OwnerLexicalTargetRef::Declaration {
                            owner,
                            declaration,
                            capability:
                                OwnerLexicalDeclarationCapability::Value
                                | OwnerLexicalDeclarationCapability::Out { .. },
                        },
                }) => Some(OwnerDeclarationRef::ImportedStable {
                    owner: owner.clone(),
                    declaration: declaration.clone(),
                }),
                Some(
                    OwnerEffectiveLexicalTarget::Static {
                        target:
                            OwnerLexicalDeclarationTarget::Passed
                            | OwnerLexicalDeclarationTarget::Imported { .. }
                            | OwnerLexicalDeclarationTarget::Ambiguous { .. },
                    }
                    | OwnerEffectiveLexicalTarget::Imported { .. }
                    | OwnerEffectiveLexicalTarget::InvalidBareBinding { .. }
                    | OwnerEffectiveLexicalTarget::Ambiguous { .. },
                )
                | None => None,
            };
            if let Some(reason) = invalid {
                return Ok(OwnerExpressionKind::Invalid {
                    tokens: vec![reason.to_owned(), root.clone()],
                });
            }
            if let Some(declaration) = lexical {
                if planned.is_some_and(|read| read.projection.as_ref() != projection) {
                    return Err(CheckedOwnerBuildError::new(format!(
                        "owner signature lexical projection diverges for read {id:?}"
                    )));
                }
                return Ok(if drain {
                    OwnerExpressionKind::Drain {
                        target: declaration,
                        projection: projection.to_vec(),
                    }
                } else {
                    OwnerExpressionKind::Read {
                        target: declaration,
                        projection: projection.to_vec(),
                        source_seed: None,
                    }
                });
            }
        }
        if let Some(resolved) = self.summary.resolved_reference_for_parts(
            &expression.stable_key,
            OwnerReferenceKind::Value,
            parts,
        ) {
            let target = if resolved.owner == self.syntax.owner {
                self.declaration_ids
                    .get(&OwnerDeclarationStableKey::Public)
                    .copied()
                    .map(local_declaration_ref)
            } else {
                Some(OwnerDeclarationRef::Imported {
                    owner: resolved.owner.clone(),
                    member: boon_checked::OwnerInterfaceMemberRef::PublicDeclaration,
                })
            };
            if let Some(target) = target {
                return Ok(if drain {
                    OwnerExpressionKind::Drain {
                        target,
                        projection: resolved.projection.to_vec(),
                    }
                } else {
                    OwnerExpressionKind::Read {
                        target,
                        projection: resolved.projection.to_vec(),
                        source_seed: None,
                    }
                });
            }
        }
        if let Some(resolution) = self.summary.symbol_resolution_for_parts(
            &expression.stable_key,
            OwnerReferenceKind::Value,
            parts,
        ) {
            let invalid = match resolution {
                OwnerSymbolResolution::CallableAsValue { .. } => "function_must_be_called",
                OwnerSymbolResolution::Unresolved { .. } => "unknown_identifier",
                OwnerSymbolResolution::Ambiguous { .. } => "ambiguous_value",
                OwnerSymbolResolution::Resolved { .. }
                | OwnerSymbolResolution::Authoritative { .. } => "",
            };
            if !invalid.is_empty() {
                return Ok(OwnerExpressionKind::Invalid {
                    tokens: vec![invalid.to_owned(), parts.join("/")],
                });
            }
        }
        let canonical_path = boon_syntax::canonical_value_path(parts);
        let declaration = self
            .abi
            .value(&canonical_path)
            .map(|contract| abi_value_key(self.abi.role(), contract))
            .transpose()?;
        Ok(OwnerExpressionKind::ExternalRead {
            canonical_path,
            declaration,
        })
    }
}

fn signature_call_target_matches(
    planned: &OwnerSignatureCallTarget,
    inferred: &InferredOwnerCallableTarget,
) -> bool {
    match (planned, inferred) {
        (
            OwnerSignatureCallTarget::Owner { owner: planned },
            InferredOwnerCallableTarget::Owner { owner: inferred },
        ) => planned == inferred,
        (OwnerSignatureCallTarget::Authoritative, InferredOwnerCallableTarget::Authoritative)
        | (OwnerSignatureCallTarget::Unresolved, InferredOwnerCallableTarget::Unresolved) => true,
        (
            OwnerSignatureCallTarget::Ambiguous {
                candidates: planned,
            },
            InferredOwnerCallableTarget::Ambiguous {
                candidates: inferred,
            },
        ) => planned == inferred,
        _ => false,
    }
}

fn signature_declaration_key(
    target: &OwnerSignatureDeclarationTarget,
) -> OwnerDeclarationStableKey {
    match target {
        OwnerSignatureDeclarationTarget::FreshOut {
            call,
            formal_ordinal,
        } => OwnerDeclarationStableKey::FreshOut {
            call: call.clone(),
            formal_ordinal: *formal_ordinal,
        },
        OwnerSignatureDeclarationTarget::CallContext {
            call,
            context_ordinal,
        } => OwnerDeclarationStableKey::CallContext {
            call: call.clone(),
            ordinal: *context_ordinal,
        },
    }
}

fn signature_scope_key(target: &OwnerSignatureDeclarationTarget) -> OwnerScopeStableKey {
    match target {
        OwnerSignatureDeclarationTarget::FreshOut {
            call,
            formal_ordinal,
        } => OwnerScopeStableKey::GeneratedOut {
            call: call.clone(),
            formal_ordinal: *formal_ordinal,
        },
        OwnerSignatureDeclarationTarget::CallContext {
            call,
            context_ordinal,
        } => OwnerScopeStableKey::CallContext {
            call: call.clone(),
            ordinal: *context_ordinal,
        },
    }
}

fn first_lexical_scope_below_or_equal(
    scopes: &[crate::OwnerLexicalScopePlan],
    mut scope: u32,
    ancestor: u32,
) -> Result<Option<u32>, CheckedOwnerBuildError> {
    if scope == ancestor {
        return Ok(None);
    }
    for _ in 0..=scopes.len() {
        let parent = scopes
            .get(scope as usize)
            .ok_or_else(|| {
                CheckedOwnerBuildError::new("owner lexical scope ancestry is out of bounds")
            })?
            .parent
            .ok_or_else(|| {
                CheckedOwnerBuildError::new(
                    "owner lexical scope does not descend from its evaluation boundary",
                )
            })?;
        if parent == ancestor {
            return Ok(Some(scope));
        }
        scope = parent;
    }
    Err(CheckedOwnerBuildError::new(
        "owner lexical scope ancestry contains a cycle",
    ))
}

fn checked_parameter_kind(kind: OwnerParameterKind) -> CheckedParameterKind {
    match kind {
        OwnerParameterKind::Value => CheckedParameterKind::Value,
        OwnerParameterKind::Out => CheckedParameterKind::Out,
    }
}

fn owner_evaluation_scope(output: Option<u32>) -> OwnerInterfaceEvaluationScope {
    output.map_or(OwnerInterfaceEvaluationScope::Parent, |parameter_ordinal| {
        OwnerInterfaceEvaluationScope::Output { parameter_ordinal }
    })
}

fn matched_input_source_site(
    call: &StableExpressionKey,
    source: OwnerSignatureMatchedInputSource,
) -> Option<OwnerSourceSite> {
    match source {
        OwnerSignatureMatchedInputSource::PipeInput => None,
        OwnerSignatureMatchedInputSource::CallArgument { ordinal } => {
            Some(OwnerSourceSite::CallArgument {
                expression: call.clone(),
                ordinal,
            })
        }
        OwnerSignatureMatchedInputSource::PipeArgument { ordinal } => {
            Some(OwnerSourceSite::PipeArgument {
                expression: call.clone(),
                ordinal,
            })
        }
    }
}

fn signature_pass_source_site(
    call: &StableExpressionKey,
    source: OwnerSignaturePassSource,
) -> OwnerSourceSite {
    match source {
        OwnerSignaturePassSource::Call => OwnerSourceSite::CallPass {
            expression: call.clone(),
        },
        OwnerSignaturePassSource::Pipe => OwnerSourceSite::PipePass {
            expression: call.clone(),
        },
    }
}

fn checked_u32(value: usize, context: &str) -> Result<u32, CheckedOwnerBuildError> {
    u32::try_from(value).map_err(|_| CheckedOwnerBuildError::new(format!("{context} exceeds u32")))
}

fn local_scope_ref(scope: OwnerScopeId) -> OwnerScopeRef {
    OwnerScopeRef::Local { scope }
}

fn local_declaration_ref(declaration: OwnerDeclarationId) -> OwnerDeclarationRef {
    OwnerDeclarationRef::Local { declaration }
}

fn owner_parameter_ordinal(reference: &OwnerDeclarationRef) -> Option<u32> {
    match reference {
        OwnerDeclarationRef::Imported {
            member: OwnerInterfaceMemberRef::Parameter { ordinal },
            ..
        }
        | OwnerDeclarationRef::Abi {
            member: OwnerAbiMemberRef::Parameter { ordinal },
            ..
        } => Some(*ordinal),
        OwnerDeclarationRef::Local { .. }
        | OwnerDeclarationRef::Imported { .. }
        | OwnerDeclarationRef::ImportedStable { .. }
        | OwnerDeclarationRef::Abi { .. }
        | OwnerDeclarationRef::ScopeOwner { .. } => None,
    }
}

fn abi_value_key(
    role: ProgramRole,
    contract: &OwnerAbiValueContract,
) -> Result<OwnerAbiDeclarationKey, CheckedOwnerBuildError> {
    owner_abi_value_declaration_key(role, contract)
        .map_err(|error| CheckedOwnerBuildError::new(error.to_string()))
}

fn unknown_flow_type() -> FlowType {
    FlowType {
        mode: FlowMode::Continuous,
        ty: Type::Unknown,
    }
}

fn checked_declaration_kind(kind: OwnerDeclarationKind) -> CheckedDeclarationKind {
    match kind {
        OwnerDeclarationKind::Function => CheckedDeclarationKind::Function,
        OwnerDeclarationKind::Field => CheckedDeclarationKind::Field,
        OwnerDeclarationKind::Source => CheckedDeclarationKind::Source,
        OwnerDeclarationKind::Hold => CheckedDeclarationKind::Hold,
        OwnerDeclarationKind::List => CheckedDeclarationKind::List,
    }
}

fn declaration_name(kind: &AstStatementKind) -> Option<&str> {
    match kind {
        AstStatementKind::Function { name, .. } | AstStatementKind::Field { name } => Some(name),
        AstStatementKind::Source { field, .. }
        | AstStatementKind::List { field, .. }
        | AstStatementKind::Hold { field, .. } => field.as_deref(),
        AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => None,
    }
}

/// Return the name of a declaration that the checked shard actually
/// materializes. `declaration_name` intentionally excludes fieldless HOLD
/// update aliases because most of them reuse an enclosing declaration. The
/// reservation pass admits the one alias selected as the lexical authority;
/// only that reserved row may use the authored HOLD name here.
fn materialized_declaration_name(kind: &AstStatementKind) -> Option<&str> {
    declaration_name(kind).or_else(|| match kind {
        AstStatementKind::Hold { field: None, name } => name.as_deref(),
        AstStatementKind::Function { .. }
        | AstStatementKind::Field { .. }
        | AstStatementKind::Source { .. }
        | AstStatementKind::Hold { field: Some(_), .. }
        | AstStatementKind::List { .. }
        | AstStatementKind::Block
        | AstStatementKind::Spread
        | AstStatementKind::Expression => None,
    })
}

fn public_declaration_flow_type(interface: &OwnerPublicInterface) -> FlowType {
    if interface.declaration_kind != Some(OwnerDeclarationKind::Function) {
        return interface.result.clone();
    }
    FlowType {
        mode: FlowMode::Continuous,
        ty: Type::Function {
            args: interface
                .parameters
                .iter()
                .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
                .map(|parameter| parameter.flow_type.ty.clone())
                .collect(),
            result: Box::new(interface.result.clone()),
        },
    }
}

fn statement_source(statement: &crate::OwnerStatementInput) -> OwnerSourceSite {
    OwnerSourceSite::Statement {
        statement: statement.stable_key.clone(),
    }
}

fn expression_source(expression: &crate::OwnerExpressionInput) -> OwnerSourceSite {
    OwnerSourceSite::Expression {
        expression: expression.stable_key.clone(),
    }
}

fn statement_body_container<'a>(
    syntax: &'a OwnerSyntaxInput,
    statement: &crate::OwnerStatementInput,
) -> Option<(usize, &'a crate::OwnerExpressionInput)> {
    fn is_container(expression: &crate::OwnerExpressionInput) -> bool {
        matches!(
            expression.kind,
            AstExprKind::Block { .. }
                | AstExprKind::Object(_)
                | AstExprKind::ListLiteral { .. }
                | AstExprKind::MapLiteral { .. }
                | AstExprKind::SetLiteral { .. }
        )
    }
    let expression_index = statement.expression? as usize;
    let expression = syntax.expressions.get(expression_index)?;
    if is_container(expression) {
        return Some((expression_index, expression));
    }
    let output = match &expression.kind {
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        }
        | AstExprKind::Then {
            output: Some(output),
            ..
        } => *output,
        _ => return None,
    };
    syntax
        .expressions
        .get(output)
        .filter(|output| is_container(output))
        .map(|expression| (output, expression))
}

fn lexical_scope_depths(
    scopes: &[crate::OwnerLexicalScopePlan],
) -> Result<Vec<usize>, CheckedOwnerBuildError> {
    let mut depths = vec![None::<usize>; scopes.len()];
    let mut visiting = vec![false; scopes.len()];
    for start in 0..scopes.len() {
        if depths[start].is_some() {
            continue;
        }
        let mut path = Vec::new();
        let mut current = start;
        loop {
            if let Some(mut depth) = depths[current] {
                for scope in path.into_iter().rev() {
                    depth = depth.saturating_add(1);
                    depths[scope] = Some(depth);
                    visiting[scope] = false;
                }
                break;
            }
            if visiting[current] {
                return Err(CheckedOwnerBuildError::new(
                    "owner lexical scope ancestry contains a cycle",
                ));
            }
            visiting[current] = true;
            path.push(current);
            let Some(parent) = scopes[current].parent else {
                let mut depth = 0usize;
                for scope in path.into_iter().rev() {
                    depths[scope] = Some(depth);
                    visiting[scope] = false;
                    depth = depth.saturating_add(1);
                }
                break;
            };
            current = usize::try_from(parent)
                .ok()
                .filter(|parent| *parent < scopes.len())
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new("owner lexical scope references a missing parent")
                })?;
        }
    }
    Ok(depths.into_iter().map(Option::unwrap).collect())
}

fn owner_expression_ref(
    syntax: &OwnerSyntaxInput,
    reference: usize,
) -> Result<OwnerExpressionRef, CheckedOwnerBuildError> {
    if reference < syntax.expressions.len() {
        return Ok(OwnerExpressionRef::Local {
            expression: OwnerExpressionId(checked_u32(reference, "owner expression reference")?),
        });
    }
    let external = syntax.external_expression(reference).ok_or_else(|| {
        CheckedOwnerBuildError::new(format!(
            "owner {:?} expression reference {reference} is out of bounds",
            syntax.owner
        ))
    })?;
    Ok(OwnerExpressionRef::Child {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn exact_linked_input(
    syntax: &OwnerSyntaxInput,
    expression: &crate::OwnerExpressionInput,
    fallback: usize,
) -> Result<OwnerExpressionRef, CheckedOwnerBuildError> {
    owner_expression_ref(
        syntax,
        expression
            .linked_input
            .map_or(fallback, |input| input as usize),
    )
}

fn pattern_variable_names(pattern: &AstMatchPattern) -> Vec<String> {
    match pattern {
        AstMatchPattern::Binding { name } => vec![name.clone()],
        AstMatchPattern::Tag { fields, .. } => fields.clone(),
        AstMatchPattern::Wildcard
        | AstMatchPattern::Number { .. }
        | AstMatchPattern::Text { .. }
        | AstMatchPattern::Bits { .. } => Vec::new(),
        AstMatchPattern::Invalid { .. } => Vec::new(),
    }
}

fn pattern_binding_type(selector: &Type, pattern: &AstMatchPattern, name: &str) -> Type {
    match pattern {
        AstMatchPattern::Binding { name: binding } if binding == name => selector.clone(),
        AstMatchPattern::Tag { name: tag, fields } if fields.iter().any(|field| field == name) => {
            let Type::VariantSet(variants) = selector else {
                return Type::Unknown;
            };
            variants
                .iter()
                .find_map(|variant| match variant {
                    Variant::Tagged {
                        tag: candidate,
                        fields,
                    } if candidate == tag => fields.fields.get(name).cloned(),
                    _ => None,
                })
                .unwrap_or(Type::Unknown)
        }
        _ => Type::Unknown,
    }
}

fn checked_match_pattern(
    pattern: &AstMatchPattern,
) -> Result<boon_checked::CheckedMatchPattern, CheckedOwnerBuildError> {
    Ok(match pattern {
        AstMatchPattern::Wildcard => boon_checked::CheckedMatchPattern::Wildcard,
        AstMatchPattern::Number { value } => boon_checked::CheckedMatchPattern::Number {
            value: ExactNumber::parse_strict(value, None).map_err(|error| {
                CheckedOwnerBuildError::new(format!("invalid exact number pattern: {error}"))
            })?,
        },
        AstMatchPattern::Text { value } => boon_checked::CheckedMatchPattern::Text {
            value: value.clone(),
        },
        AstMatchPattern::Tag { name, fields } => boon_checked::CheckedMatchPattern::Tag {
            name: name.clone(),
            fields: fields.clone(),
        },
        AstMatchPattern::Binding { name } => {
            boon_checked::CheckedMatchPattern::Binding { name: name.clone() }
        }
        AstMatchPattern::Bits {
            width,
            radix,
            digits,
        } => boon_checked::CheckedMatchPattern::Bits {
            value: Bits::parse_encoded(*width, *radix, digits).map_err(|error| {
                CheckedOwnerBuildError::new(format!("invalid bits pattern: {error}"))
            })?,
        },
        AstMatchPattern::Invalid { .. } => {
            return Err(CheckedOwnerBuildError::new("invalid match pattern"));
        }
    })
}

/// Build one complete owner shard without opening a project-wide checker.
///
/// Row construction is intentionally implemented below this validation seam;
/// callers cannot publish a partially validated shard.  The first production
/// consumer is added in the same flag-day tranche as the compatibility
/// assembler and `ProjectState.checked` deletion.
pub fn build_checked_owner_shard<'a>(
    syntax: &OwnerSyntaxInput,
    lexical_plan: &crate::OwnerLexicalPlan,
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    body: &OwnerBodyInferenceShard,
    body_currentness: &OwnerBodyInferenceCurrentnessReceipt,
    inference_abi: &OwnerInferenceAbiEnvironment,
    construction_abi: &OwnerConstructionAbiEnvironment,
    own_scc: &'a OwnerInterfaceSccResult,
    imported_sccs: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<CheckedOwnerShard, CheckedOwnerBuildError> {
    validate_inputs(
        syntax,
        lexical_plan,
        seed,
        summary,
        body,
        body_currentness,
        inference_abi,
        construction_abi,
        own_scc,
    )?;

    let mut interfaces =
        validated_frozen_interfaces(body, body_currentness, own_scc, imported_sccs)?;
    let own_interface = interfaces.remove(&syntax.owner).ok_or_else(|| {
        CheckedOwnerBuildError::new(format!(
            "checked owner {:?} has no frozen public interface",
            syntax.owner
        ))
    })?;
    let basis = CheckedOwnerShardBasis {
        owner: syntax.owner.clone(),
        syntax_fingerprint_v1: syntax.fingerprint_v1(),
        lexical_plan_fingerprint_v1: lexical_plan.fingerprint_v1(),
        signature_lexical_plan_fingerprint_v1: body.signature_lexical_plan.fingerprint_v1(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        summary_fingerprint_v1: summary.fingerprint_v1(),
        body_fingerprint_v1: body.fingerprint_v1(),
        body_currentness_fingerprint_v1: body_currentness.fingerprint_v1(),
        own_interface_scc_fingerprint_v1: own_scc.fingerprint_v1(),
        construction_abi_fingerprint_v1: construction_abi.fingerprint_v1(),
    };

    let (rows, receipts, diagnostics) = OwnerRowConstruction::new(
        syntax,
        lexical_plan,
        seed,
        summary,
        body,
        own_interface,
        construction_abi,
        interfaces,
    )?
    .build_base_rows()?;
    // The construction receipt commits every normalized row, diagnostic, and
    // relocation. Bind that compact seal to the exact current basis instead of
    // serializing the complete rich row tables for a second time.
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        CHECKED_OWNER_SHARD_DOMAIN_V7,
        &(&basis, &receipts.construction),
    )
    .map_err(|error| {
        CheckedOwnerBuildError::new(format!(
            "cannot fingerprint checked owner {:?}: {error}",
            syntax.owner
        ))
    })?;
    Ok(CheckedOwnerShard {
        basis,
        rows,
        diagnostics,
        receipts,
        fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        OwnerConstraintEdgeRole, OwnerPatternConstraint, OwnerSignatureCallLexicalError,
        OwnerSourceAnchorRole, build_owner_callable_scope_topology, build_owner_interface_topology,
        evaluate_owner_body_with_signature_plan, evaluate_owner_callable_scope_scc,
        evaluate_owner_interface_scc_with_signature_scopes, plan_owner_body_interfaces,
        project_owner_abi_environment, project_owner_callable_resolution_plan,
        project_owner_constraint_seed_with_lexical_plan, project_owner_lexical_plan,
        project_owner_syntax_input, resolve_owner_constraint_seed,
        resolve_owner_constraint_seed_with_signature_plan, solve_owner_interface_scc,
    };
    use boon_checked::{ExternalTypeEnvironment, OwnerAbiMemberRef};
    use boon_parser::{ProjectSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys};
    use std::sync::Arc;

    struct Fixture {
        syntax: OwnerSyntaxInput,
        lexical_plan: crate::OwnerLexicalPlan,
        seed: OwnerConstraintSeed,
        summary: OwnerConstraintSummary,
        inference_abi: OwnerInferenceAbiEnvironment,
        construction_abi: OwnerConstructionAbiEnvironment,
        interface: OwnerInterfaceSccResult,
        imported_interfaces: BTreeMap<StableCheckOwnerKey, OwnerPublicInterface>,
        body: OwnerBodyInferenceShard,
        body_currentness: OwnerBodyInferenceCurrentnessReceipt,
    }

    fn fixture(source: &str, name: &str) -> Fixture {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let source_unit_id = parsed.source_unit_id.clone();
        let link_key = project_unit_link_keys(
            "app/RUN.bn",
            [(source_unit_id.clone(), parsed.declared_functions.clone())],
        )
        .unwrap()
        .remove(&source_unit_id)
        .unwrap();
        let unit = Arc::new(parsed.into_unit_syntax_snapshot(link_key).unwrap());
        let project =
            ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::clone(&unit)])
                .unwrap();
        let owners = unit.stable_check_owner_keys().collect::<Vec<_>>();
        let owner = owners
            .iter()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if name == "<list>"
                            && owner.item_route.segments().last().is_some_and(|segment| {
                                segment.kind == boon_syntax::UnitItemKind::List
                            })
                )
            })
            .or_else(|| {
                owners.iter().find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(owner)
                            if owner.item_route.segments().last().is_some_and(|segment| {
                                segment.names.as_ref() == [name]
                            })
                    )
                })
            })
            .or_else(|| {
                owners.iter().find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(owner)
                            if owner.item_route.segments().last().is_some_and(|segment| {
                                segment.names.iter().any(|candidate| candidate == name)
                            })
                    )
                })
            })
            .cloned()
            .unwrap_or_else(|| panic!("fixture `{name}` has no owner in {owners:#?}"));
        let abi = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let syntaxes = owners
            .iter()
            .map(|owner| {
                (
                    owner.clone(),
                    project_owner_syntax_input(unit.owner_view_for_key(owner).unwrap()).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let lexical_plans = syntaxes
            .iter()
            .map(|(owner, syntax)| (owner.clone(), project_owner_lexical_plan(syntax).unwrap()))
            .collect::<BTreeMap<_, _>>();
        let seeds = syntaxes
            .iter()
            .map(|(owner, syntax)| {
                (
                    owner.clone(),
                    project_owner_constraint_seed_with_lexical_plan(syntax, &lexical_plans[owner])
                        .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let callable_surfaces = seeds
            .iter()
            .filter_map(|(owner, seed)| {
                seed.declarations
                    .iter()
                    .find(|declaration| {
                        declaration.public && declaration.kind == OwnerDeclarationKind::Function
                    })
                    .map(|declaration| (owner, declaration))
            })
            .collect::<Vec<_>>();
        let base_summaries = seeds
            .iter()
            .map(|(owner, seed)| {
                let resolved = seed
                    .references
                    .iter()
                    .filter(|reference| reference.kind == OwnerReferenceKind::Callable)
                    .filter_map(|reference| {
                        let matches = callable_surfaces
                            .iter()
                            .filter(|(_, declaration)| {
                                declaration.names.as_ref() == reference.parts.as_ref()
                            })
                            .collect::<Vec<_>>();
                        (matches.len() == 1).then(|| {
                            let (target, declaration) = matches[0];
                            crate::ResolvedOwnerSymbolReference {
                                reference: reference.clone(),
                                owner: (*target).clone(),
                                projection: Box::new([]),
                                parameters: declaration.parameters.clone(),
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                (
                    owner.clone(),
                    resolve_owner_constraint_seed(seed, resolved).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let callable_plans = seeds
            .iter()
            .map(|(owner, seed)| {
                (
                    owner.clone(),
                    project_owner_callable_resolution_plan(
                        seed,
                        base_summaries[owner]
                            .symbol_resolutions
                            .iter()
                            .filter(|resolution| {
                                resolution.reference().kind == OwnerReferenceKind::Callable
                            })
                            .cloned(),
                    )
                    .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let callable_topology =
            build_owner_callable_scope_topology(callable_plans.values()).unwrap();
        let callable_provider = abi.callable_environment().unwrap();
        let mut callable_results = BTreeMap::<
            crate::OwnerCallableScopeSccKey,
            Arc<crate::OwnerCallableScopeSccResult>,
        >::new();
        for scc in &callable_topology.sccs {
            let callable_abi = callable_provider
                .inference_environment(
                    scc.key.members.iter().cloned(),
                    scc.key.members.iter().flat_map(|member| {
                        callable_plans[member].authoritative_abi_names().into_vec()
                    }),
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| callable_results[dependency].as_ref())
                .collect::<Vec<_>>();
            let evaluation = evaluate_owner_callable_scope_scc(
                scc,
                scc.key.members.iter().map(|member| &seeds[member]),
                scc.key.members.iter().map(|member| &callable_plans[member]),
                &callable_abi,
                dependencies,
            )
            .unwrap();
            callable_results.insert(scc.key.clone(), evaluation.result);
        }
        let callable_scopes = callable_results
            .values()
            .flat_map(|result| result.owners.iter())
            .map(|scope| (scope.owner().clone(), scope))
            .collect::<BTreeMap<_, _>>();
        let summaries = seeds
            .iter()
            .map(|(owner, seed)| {
                let plan = callable_scopes[owner].lexical_plan();
                let resolutions = plan.external_candidates().iter().map(|reference| {
                    base_summaries[owner]
                        .symbol_resolutions
                        .iter()
                        .find(|resolution| resolution.reference() == reference)
                        .cloned()
                        .unwrap()
                });
                (
                    owner.clone(),
                    resolve_owner_constraint_seed_with_signature_plan(seed, plan, resolutions)
                        .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let topology = build_owner_interface_topology(summaries.values()).unwrap();
        let mut interface_results =
            BTreeMap::<crate::OwnerInterfaceSccKey, Arc<OwnerInterfaceSccResult>>::new();
        for scc in &topology.sccs {
            let parameter_requirements = scc
                .key
                .members
                .iter()
                .flat_map(|member| {
                    seeds[member]
                        .parameter_requirement_keys()
                        .into_vec()
                        .into_iter()
                        .map(|key| {
                            let (function, parameter) = seeds[member]
                                .parameter_requirement_names(key.parameter_ordinal())
                                .unwrap();
                            abi.parameter_requirement_lookup(key, function, parameter)
                                .unwrap()
                        })
                })
                .collect::<Vec<_>>();
            let interface_abi = abi
                .complete_inference_environment_with_requirements(
                    scc.key.members.iter().cloned(),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|member| summaries[member].authoritative_abi_names().into_vec()),
                    scc.key.members.iter().flat_map(|member| {
                        summaries[member].authoritative_value_abi_paths().into_vec()
                    }),
                    scc.key
                        .members
                        .iter()
                        .flat_map(|member| seeds[member].source_payload_abi_paths().into_vec()),
                    parameter_requirements,
                )
                .unwrap();
            let dependencies = scc
                .dependencies
                .iter()
                .map(|dependency| interface_results[dependency].as_ref())
                .collect::<Vec<_>>();
            let evaluation = evaluate_owner_interface_scc_with_signature_scopes(
                scc,
                &interface_abi,
                scc.key.members.iter().map(|member| &seeds[member]),
                scc.key.members.iter().map(|member| &summaries[member]),
                dependencies,
                scc.key.members.iter().map(|member| callable_scopes[member]),
            )
            .unwrap();
            interface_results.insert(scc.key.clone(), evaluation.result);
        }
        let syntax = syntaxes[&owner].clone();
        let lexical_plan = lexical_plans[&owner].clone();
        let seed = seeds[&owner].clone();
        let summary = summaries[&owner].clone();
        let parameter_requirements = seed
            .parameter_requirement_keys()
            .into_vec()
            .into_iter()
            .map(|key| {
                let (function, parameter) = seed
                    .parameter_requirement_names(key.parameter_ordinal())
                    .unwrap();
                abi.parameter_requirement_lookup(key, function, parameter)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let inference_abi = abi
            .complete_inference_environment_with_requirements(
                [seed.owner.clone()],
                summary.authoritative_abi_names().into_vec(),
                summary.authoritative_value_abi_paths().into_vec(),
                seed.source_payload_abi_paths().into_vec(),
                parameter_requirements,
            )
            .unwrap();
        let own_scc = topology.scc_for_owner(&owner).unwrap();
        let interface = interface_results[&own_scc.key].clone();
        let interface_plan = plan_owner_body_interfaces(
            &seed,
            &summary,
            interface_results.values().map(AsRef::as_ref),
        )
        .unwrap();
        let imported = interface_results
            .values()
            .map(AsRef::as_ref)
            .filter(|result| {
                interface_plan
                    .imports()
                    .iter()
                    .any(|import| import.key() == &result.key)
            })
            .collect::<Vec<_>>();
        let imported_interfaces = imported
            .iter()
            .flat_map(|result| result.owners.iter())
            .map(|interface| (interface.owner.clone(), interface.clone()))
            .collect::<BTreeMap<_, _>>();
        let body_evaluation = evaluate_owner_body_with_signature_plan(
            &syntax,
            &lexical_plan,
            &seed,
            &summary,
            &inference_abi,
            &interface_plan,
            callable_scopes[&owner].lexical_plan(),
        )
        .unwrap();
        let body = Arc::unwrap_or_clone(body_evaluation.result);
        let construction_abi = abi
            .construction_environment(
                seed.owner.clone(),
                summary.authoritative_abi_names().into_vec(),
                summary.authoritative_value_abi_paths().into_vec(),
            )
            .unwrap();
        Fixture {
            syntax,
            lexical_plan,
            seed,
            summary,
            inference_abi,
            construction_abi,
            interface: Arc::unwrap_or_clone(interface),
            imported_interfaces,
            body,
            body_currentness: body_evaluation.currentness,
        }
    }

    fn built_rows(fixture: &Fixture) -> (CheckedOwnerRows, OwnerCheckedReceiptSet) {
        let own = fixture.interface.owner(&fixture.syntax.owner).unwrap();
        let imported_interfaces = fixture
            .imported_interfaces
            .iter()
            .map(|(owner, interface)| (owner.clone(), interface))
            .collect();
        let (rows, receipts, _) = OwnerRowConstruction::new(
            &fixture.syntax,
            &fixture.lexical_plan,
            &fixture.seed,
            &fixture.summary,
            &fixture.body,
            own,
            &fixture.construction_abi,
            imported_interfaces,
        )
        .unwrap()
        .build_base_rows()
        .unwrap();
        (rows, receipts)
    }

    fn rows(fixture: &Fixture) -> CheckedOwnerRows {
        built_rows(fixture).0
    }

    fn fixture_with_symbol_resolution(base: Fixture, resolution: OwnerSymbolResolution) -> Fixture {
        let signature_plan = base.body.signature_lexical_plan.clone();
        let summary = resolve_owner_constraint_seed_with_signature_plan(
            &base.seed,
            &signature_plan,
            [resolution],
        )
        .unwrap();
        let topology = build_owner_interface_topology([&summary]).unwrap();
        let interface = solve_owner_interface_scc(
            topology.sccs.first().unwrap(),
            &base.inference_abi,
            [&base.seed],
            [&summary],
            [],
        )
        .unwrap();
        let interface_plan =
            plan_owner_body_interfaces(&base.seed, &summary, [&interface]).unwrap();
        let body_evaluation = evaluate_owner_body_with_signature_plan(
            &base.syntax,
            &base.lexical_plan,
            &base.seed,
            &summary,
            &base.inference_abi,
            &interface_plan,
            &signature_plan,
        )
        .unwrap();
        Fixture {
            summary,
            interface,
            body: Arc::unwrap_or_clone(body_evaluation.result),
            body_currentness: body_evaluation.currentness,
            ..base
        }
    }

    #[test]
    fn complete_checked_owner_shard_closes_rows_receipts_and_currentness() {
        let fixture = fixture("record: [value: 1]\n", "record");
        let shard = build_checked_owner_shard(
            &fixture.syntax,
            &fixture.lexical_plan,
            &fixture.seed,
            &fixture.summary,
            &fixture.body,
            &fixture.body_currentness,
            &fixture.inference_abi,
            &fixture.construction_abi,
            &fixture.interface,
            [],
        )
        .unwrap();

        assert_eq!(shard.owner(), &fixture.syntax.owner);
        assert_eq!(
            shard.basis.body_currentness_fingerprint_v1,
            fixture.body_currentness.fingerprint_v1()
        );
        assert_eq!(
            shard.basis.construction_abi_fingerprint_v1,
            fixture.construction_abi.fingerprint_v1()
        );
        assert_eq!(
            shard.receipts.construction.row_receipt_count as usize,
            shard.receipts.row_receipts.len()
        );
        assert_eq!(
            shard.receipts.construction.relocation_count as usize,
            shard.receipts.relocations.len()
        );
        crate::owner_checked::validate_owner_checked_receipts(&shard.receipts).unwrap();
        assert_ne!(shard.fingerprint_v1(), [0; 32]);
    }

    #[test]
    fn compact_checked_owner_seal_changes_with_row_content() {
        let build = |fixture: &Fixture| {
            build_checked_owner_shard(
                &fixture.syntax,
                &fixture.lexical_plan,
                &fixture.seed,
                &fixture.summary,
                &fixture.body,
                &fixture.body_currentness,
                &fixture.inference_abi,
                &fixture.construction_abi,
                &fixture.interface,
                [],
            )
            .unwrap()
        };
        let original = fixture("record: [value: 1]\n", "record");
        let changed = fixture("record: [value: 2]\n", "record");
        let original = build(&original);
        let changed = build(&changed);

        assert_ne!(original.fingerprint_v1(), changed.fingerprint_v1());
        assert_ne!(
            original.receipts.construction.local_content_digest_v1,
            changed.receipts.construction.local_content_digest_v1
        );
    }

    #[test]
    fn checked_owner_seal_rejects_stale_proof_or_construction_abi() {
        let fixture = fixture("record: [value: 1]\n", "record");
        let build = || {
            build_checked_owner_shard(
                &fixture.syntax,
                &fixture.lexical_plan,
                &fixture.seed,
                &fixture.summary,
                &fixture.body,
                &fixture.body_currentness,
                &fixture.inference_abi,
                &fixture.construction_abi,
                &fixture.interface,
                [],
            )
            .unwrap()
        };

        let mut shard = build();
        assert!(shard.validate_seal(&fixture.construction_abi).is_ok());
        let wrong_abi = OwnerConstructionAbiEnvironment::new(
            fixture.syntax.owner.clone(),
            ProgramRole::Server,
            [],
            [],
        )
        .unwrap();
        assert!(
            shard
                .validate_seal(&wrong_abi)
                .unwrap_err()
                .to_string()
                .contains("exact construction ABI")
        );

        shard.fingerprint_v1[0] ^= 1;
        assert!(
            shard
                .validate_seal(&fixture.construction_abi)
                .unwrap_err()
                .to_string()
                .contains("stale construction seal")
        );

        let mut shard = build();
        shard.receipts.construction.domain_counts[0].rows += 1;
        assert!(shard.validate_seal(&fixture.construction_abi).is_err());
    }

    #[test]
    fn checked_owner_shard_rejects_an_incomplete_construction_abi() {
        let fixture = fixture(
            "FUNCTION keep(input) {\n    Number/to_text(value: input)\n}\n",
            "keep",
        );
        let incomplete = OwnerConstructionAbiEnvironment::new(
            fixture.syntax.owner.clone(),
            ProgramRole::Client,
            [],
            [],
        )
        .unwrap();
        let error = build_checked_owner_shard(
            &fixture.syntax,
            &fixture.lexical_plan,
            &fixture.seed,
            &fixture.summary,
            &fixture.body,
            &fixture.body_currentness,
            &fixture.inference_abi,
            &incomplete,
            &fixture.interface,
            [],
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match its exact callable lookup set")
        );
    }

    #[test]
    fn base_rows_preserve_stable_object_structure() {
        let fixture = fixture("record: [value: 1]\n", "record");
        let rows = rows(&fixture);
        assert_eq!(rows.declarations.len(), 2);
        assert!(rows.declarations.iter().any(|declaration| {
            matches!(
                declaration.stable_key,
                OwnerDeclarationStableKey::RecordField { ref name, .. } if name == "value"
            )
        }));
        assert_eq!(rows.statements.len(), 1);
        assert_eq!(rows.expressions.len(), 2);
        assert!(matches!(
            rows.expressions.last().unwrap().kind,
            OwnerExpressionKind::Object { ref fields }
                if fields.len() == 1 && fields[0].name == "value"
        ));
        assert_eq!(
            rows.declarations[0].flow_type,
            fixture.body.expressions.last().unwrap().flow_type
        );
    }

    #[test]
    fn callable_rows_own_function_scope_parameters_and_result() {
        let fixture = fixture("FUNCTION identity(input) {\n    input\n}\n", "identity");
        let rows = rows(&fixture);
        assert_eq!(rows.scopes.len(), 1);
        assert_eq!(rows.declarations.len(), 2);
        assert_eq!(rows.callables.len(), 1);
        assert_eq!(rows.callables[0].parameters.len(), 1);
        assert_eq!(rows.callables[0].result, fixture.interface.owners[0].result);
        assert!(rows.callables[0].result_expression.is_some());
    }

    #[test]
    fn authoritative_calls_bind_exact_abi_members() {
        let fixture = fixture("value: Number/to_text(value: 1)\n", "value");
        let (rows, receipts) = built_rows(&fixture);
        assert_eq!(rows.calls.len(), 1);
        assert!(matches!(
            rows.expressions.last().unwrap().kind,
            OwnerExpressionKind::Call {
                call: OwnerCallId(0)
            }
        ));
        assert!(matches!(
            rows.calls[0].callable,
            OwnerDeclarationRef::Abi {
                member: OwnerAbiMemberRef::Declaration,
                ..
            }
        ));
        assert!(matches!(
            rows.calls[0].entries.as_slice(),
            [OwnerCallEntry::Input {
                formal: OwnerDeclarationRef::Abi {
                    member: OwnerAbiMemberRef::Parameter { ordinal: 0 },
                    ..
                },
                ..
            }]
        ));
        let authoritative_relocations = receipts
            .relocations
            .iter()
            .filter(|relocation| {
                matches!(
                    relocation.target,
                    OwnerRelocationTarget::AbiDeclaration { .. }
                )
            })
            .count();
        assert!(authoritative_relocations >= 2);
    }

    #[test]
    fn render_call_contexts_bind_dependent_argument_reads() {
        let fixture = fixture(
            concat!(
                "view: Element/text(\n",
                "    element: []\n",
                "    style: [active: element.hovered]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        let rows = rows(&fixture);
        let call = rows
            .calls
            .iter()
            .find(|call| call.function == "Element/text")
            .expect("render call must be published");
        let context = call
            .contexts
            .first()
            .expect("element constructor must materialize its call context");
        let context_read = rows
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ref projection,
                        ..
                    } if declaration == context.declaration && projection == &["hovered"]
                )
            })
            .expect("dependent context read must target the planned declaration");
        assert_eq!(context_read.flow_type.ty, crate::true_false_type());
        assert!(rows.occurrences.iter().any(|occurrence| {
            occurrence.kind == SemanticOccurrenceKind::Read
                && occurrence.target == local_declaration_ref(context.declaration)
        }));
    }

    #[test]
    fn dynamic_producer_types_are_preserved_before_nested_user_call_validation() {
        let fresh = fixture(
            concat!(
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "FUNCTION caller() {\n",
                "    LIST { TEXT { wrong } }\n",
                "    |> List/find(item, if: plus_one(value: item) == 1)\n",
                "}\n",
            ),
            "caller",
        );
        assert!(
            fresh.body.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "user_call_argument_type"
                    && diagnostic.message.contains("expected: NUMBER")
            }),
            "{:#?}",
            fresh.body.diagnostics
        );

        let context = fixture(
            concat!(
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "view: Element/text(\n",
                "    element: []\n",
                "    style: [value: plus_one(value: element)]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        assert!(
            context.body.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "user_call_argument_type"
                    && diagnostic.message.contains("expected: NUMBER")
            }),
            "{:#?}",
            context.body.diagnostics
        );
    }

    #[test]
    fn one_dynamic_producer_preserves_exact_types_for_every_consumer() {
        let fresh = fixture(
            concat!(
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "FUNCTION caller() {\n",
                "    LIST { TEXT { wrong } }\n",
                "    |> List/find(\n",
                "        item,\n",
                "        if: plus_one(value: item) == plus_one(value: item),\n",
                "    )\n",
                "}\n",
            ),
            "caller",
        );
        assert_eq!(
            fresh
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            2,
            "{:#?}",
            fresh.body.diagnostics
        );

        let context = fixture(
            concat!(
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "view: Element/text(\n",
                "    element: []\n",
                "    style: [\n",
                "        first: plus_one(value: element)\n",
                "        second: plus_one(value: element)\n",
                "    ]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        assert_eq!(
            context
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            2,
            "{:#?}",
            context.body.diagnostics
        );
    }

    #[test]
    fn user_call_dynamic_providers_are_frozen_before_every_consumer() {
        let fresh = fixture(
            concat!(
                "FUNCTION text_items() {\n",
                "    LIST { TEXT { wrong } }\n",
                "}\n",
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "FUNCTION caller() {\n",
                "    text_items()\n",
                "    |> List/find(\n",
                "        item,\n",
                "        if: plus_one(value: item) == plus_one(value: item),\n",
                "    )\n",
                "}\n",
            ),
            "caller",
        );
        assert_eq!(
            fresh
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            2,
            "{:#?}",
            fresh.body.diagnostics
        );

        let context = fixture(
            concat!(
                "FUNCTION make_element() {\n",
                "    [hovered: True]\n",
                "}\n",
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "view: Element/text(\n",
                "    element: make_element()\n",
                "    style: [\n",
                "        first: plus_one(value: element)\n",
                "        second: plus_one(value: element)\n",
                "    ]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        assert_eq!(
            context
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            2,
            "{:#?}",
            context.body.diagnostics
        );
    }

    #[test]
    fn composite_dynamic_arguments_are_staged_by_their_evaluation_scope() {
        let fresh = fixture(
            concat!(
                "FUNCTION text_items() {\n",
                "    LIST { TEXT { wrong } }\n",
                "}\n",
                "FUNCTION wants_number_record(value) {\n",
                "    value.x + 1\n",
                "}\n",
                "FUNCTION caller() {\n",
                "    text_items()\n",
                "    |> List/find(\n",
                "        item,\n",
                "        if: wants_number_record(value: [x: item]) == 1,\n",
                "    )\n",
                "}\n",
            ),
            "caller",
        );
        assert_eq!(
            fresh
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            1,
            "diagnostics: {:#?}\nplan: {:#?}",
            fresh.body.diagnostics,
            fresh.body.signature_lexical_plan
        );

        let context = fixture(
            concat!(
                "FUNCTION make_element() {\n",
                "    [hovered: True]\n",
                "}\n",
                "FUNCTION wants_number_record(value) {\n",
                "    value.x + 1\n",
                "}\n",
                "view: Element/text(\n",
                "    element: make_element()\n",
                "    style: [value: wants_number_record(value: [x: element])]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        assert_eq!(
            context
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            1,
            "diagnostics: {:#?}\nplan: {:#?}",
            context.body.diagnostics,
            context.body.signature_lexical_plan
        );
    }

    #[test]
    fn ancestor_consumers_cannot_pollute_nested_dynamic_results() {
        let fresh = fixture(
            concat!(
                "FUNCTION text_items() {\n",
                "    LIST { TEXT { wrong } }\n",
                "}\n",
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "FUNCTION caller() {\n",
                "    plus_one(\n",
                "        value: text_items()\n",
                "            |> List/find(item, if: item == item),\n",
                "    )\n",
                "}\n",
            ),
            "caller",
        );
        assert_eq!(
            fresh
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            1,
            "{:#?}",
            fresh.body.diagnostics
        );

        let context = fixture(
            concat!(
                "FUNCTION make_element() {\n",
                "    [hovered: True]\n",
                "}\n",
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "value: plus_one(\n",
                "    value: Element/text(\n",
                "        element: make_element()\n",
                "        style: [active: element.hovered]\n",
                "        text: TEXT { Hello }\n",
                "    ),\n",
                ")\n",
            ),
            "value",
        );
        assert_eq!(
            context
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            1,
            "{:#?}",
            context.body.diagnostics
        );
    }

    #[test]
    fn dynamic_pass_values_are_frozen_before_context_constraints() {
        let fresh = fixture(
            concat!(
                "FUNCTION text_items() {\n",
                "    LIST { TEXT { wrong } }\n",
                "}\n",
                "FUNCTION needs_number_pass() {\n",
                "    PASSED.value + 1\n",
                "}\n",
                "FUNCTION caller() {\n",
                "    text_items()\n",
                "    |> List/find(\n",
                "        item,\n",
                "        if: needs_number_pass(PASS: [value: item]) == 1,\n",
                "    )\n",
                "}\n",
            ),
            "caller",
        );
        assert_eq!(
            fresh
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "pass_context_type")
                .count(),
            1,
            "{:#?}",
            fresh.body.diagnostics
        );

        let context = fixture(
            concat!(
                "FUNCTION make_element() {\n",
                "    [hovered: True]\n",
                "}\n",
                "FUNCTION needs_number_pass() {\n",
                "    PASSED.value + 1\n",
                "}\n",
                "view: Element/text(\n",
                "    element: make_element()\n",
                "    style: [value: needs_number_pass(PASS: [value: element])]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        assert_eq!(
            context
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "pass_context_type")
                .count(),
            1,
            "{:#?}",
            context.body.diagnostics
        );
    }

    #[test]
    fn dynamic_selector_narrowing_uses_exact_fresh_out_and_context_targets() {
        let fresh = fixture(
            concat!(
                "FUNCTION choose() {\n",
                "    LIST { Found[payload: 1], NotFound }\n",
                "    |> List/find(\n",
                "        item,\n",
                "        if: item |> WHEN {\n",
                "            Found[payload] => item.payload == payload\n",
                "            NotFound => False\n",
                "        },\n",
                "    )\n",
                "}\n",
            ),
            "choose",
        );
        let assert_target =
            |fixture: &Fixture, expected: fn(&OwnerEffectiveLexicalTarget) -> bool| {
                let (selector, narrowed, projection) = fixture
                    .seed
                    .expressions
                    .iter()
                    .find_map(|expression| {
                        let selector = expression.inputs.iter().find_map(|input| {
                            matches!(input.role, OwnerConstraintEdgeRole::MatchSelector)
                                .then_some(input.expression)
                        })?;
                        let narrowed = expression.inputs.iter().find_map(|input| {
                            let OwnerConstraintEdgeRole::MatchNarrowedSelector { projection } =
                                &input.role
                            else {
                                return None;
                            };
                            Some((input.expression, projection.clone()))
                        })?;
                        Some((selector, narrowed.0, narrowed.1))
                    })
                    .expect("fixture must contain a narrowed selector read");
                let selector = fixture.body.signature_lexical_plan.reads()[selector as usize]
                    .as_ref()
                    .expect("dynamic selector must have an effective read");
                let narrowed = fixture.body.signature_lexical_plan.reads()[narrowed as usize]
                    .as_ref()
                    .expect("dynamic narrowed read must have an effective read");
                assert!(expected(&selector.target));
                assert_eq!(narrowed.target, selector.target);
                assert_eq!(
                    narrowed.projection[selector.projection.len()..],
                    *projection
                );
            };
        assert_target(&fresh, |target| {
            matches!(target, OwnerEffectiveLexicalTarget::FreshOut { .. })
        });

        let context = fixture(
            concat!(
                "view: Element/text(\n",
                "    element: []\n",
                "    style: [active: element.hovered |> WHEN {\n",
                "        True => element.hovered\n",
                "        False => False\n",
                "    }]\n",
                "    text: TEXT { Hello }\n",
                ")\n",
            ),
            "view",
        );
        assert_target(&context, |target| {
            matches!(target, OwnerEffectiveLexicalTarget::CallContext { .. })
        });
    }

    #[test]
    fn broad_outer_narrowing_does_not_hide_a_nested_same_named_fresh_out_read() {
        let fixture = fixture(
            concat!(
                "FUNCTION plus_one(value) {\n",
                "    value + 1\n",
                "}\n",
                "FUNCTION choose() {\n",
                "    LIST { Found[payload: 1] }\n",
                "    |> List/find(\n",
                "        item,\n",
                "        if: item |> WHEN {\n",
                "            Found[payload] =>\n",
                "                LIST { TEXT { wrong } }\n",
                "                |> List/find(\n",
                "                    item,\n",
                "                    if: plus_one(value: item) == 1,\n",
                "                )\n",
                "        },\n",
                "    )\n",
                "}\n",
            ),
            "choose",
        );
        assert_eq!(
            fixture
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "user_call_argument_type")
                .count(),
            1,
            "{:#?}",
            fixture.body.diagnostics
        );

        let plus_one = fixture
            .body
            .signature_lexical_plan
            .calls()
            .iter()
            .find(|call| call.function == "plus_one")
            .expect("nested plus_one call");
        let actual = plus_one
            .matched_inputs
            .iter()
            .find(|input| input.formal_name == "value")
            .expect("plus_one value input");
        let read = fixture.body.signature_lexical_plan.reads()[actual.expression as usize]
            .as_ref()
            .expect("nested item read");
        let OwnerEffectiveLexicalTarget::FreshOut {
            call,
            formal_ordinal,
        } = &read.target
        else {
            panic!("nested item must target its exact FreshOut: {read:#?}");
        };
        let rows = rows(&fixture);
        let expression = &fixture.seed.expressions[actual.expression as usize].expression;
        let declaration = rows
            .expressions
            .iter()
            .find(|row| &row.stable_key == expression)
            .and_then(|row| match row.kind {
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration },
                    ..
                } => Some(declaration),
                _ => None,
            })
            .expect("checked nested item read");
        assert!(rows.declarations.iter().any(|row| {
            row.id == declaration
                && row.stable_key
                    == OwnerDeclarationStableKey::FreshOut {
                        call: call.clone(),
                        formal_ordinal: *formal_ordinal,
                    }
        }));
    }

    #[test]
    fn invalid_call_shapes_emit_diagnostics_without_checked_call_rows() {
        let invalid = fixture("value: Number/to_text(radix: 10, value: 1)\n", "value");
        let (invalid_rows, receipts) = built_rows(&invalid);
        assert!(invalid_rows.calls.is_empty());
        assert!(matches!(
            invalid_rows.expressions.last().unwrap().kind,
            OwnerExpressionKind::Invalid { .. }
        ));
        assert!(
            invalid
                .body
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "misordered_call_entry")
        );
        assert!(
            receipts.construction.domain_counts.iter().any(|count| {
                count.domain == OwnerCheckedRowDomain::Diagnostic && count.rows > 0
            })
        );

        let host = fixture("value: Clock/wall(PASS: [])\n", "value");
        let host_rows = rows(&host);
        assert!(host_rows.calls.is_empty());
        let host_call = host_rows.expressions.last().unwrap();
        assert!(matches!(
            host_call.kind,
            OwnerExpressionKind::Invalid { .. }
        ));
        assert!(host_call.effect.invokes_host);
    }

    #[test]
    fn construction_receipts_cover_every_emitted_row_and_diagnostic() {
        let fixture = fixture("record: [value: 1]\n", "record");
        let (rows, receipts) = built_rows(&fixture);
        let expected = rows.scopes.len()
            + rows.declarations.len()
            + rows.statements.len()
            + rows.expressions.len()
            + rows.callables.len()
            + rows.context_formals.len()
            + rows.calls.len()
            + rows.call_result_paths.len()
            + rows.pattern_bindings.len()
            + rows.resource_projection_seeds.len()
            + rows.sources.len()
            + rows.states.len()
            + rows.lists.len()
            + rows.occurrences.len()
            + fixture.body.diagnostics.len();
        assert_eq!(receipts.row_receipts.len(), expected);
        assert_eq!(
            receipts.construction.row_receipt_count as usize,
            receipts.row_receipts.len()
        );
    }

    #[test]
    fn pattern_bindings_own_arm_scope_type_and_local_reads() {
        let fixture = fixture(
            "value: Found[item: 1] |> WHEN { Found[item] => item }\n",
            "value",
        );
        let rows = rows(&fixture);
        assert_eq!(rows.pattern_bindings.len(), 1);
        let binding = &rows.pattern_bindings[0];
        let declaration = &rows.declarations[binding.declaration.0 as usize];
        assert_eq!(declaration.kind, CheckedDeclarationKind::PatternBinding);
        assert_eq!(declaration.flow_type.ty, Type::Number);
        assert_eq!(binding.projection, ["item"]);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration: target },
                    ref projection,
                    ..
                } if target == binding.declaration && projection.is_empty()
            )
        }));
    }

    #[test]
    fn nested_inline_pattern_scopes_and_outer_reads_follow_structural_ancestry() {
        let fixture = fixture(
            concat!(
                "value:\n",
                "    Outer[a: Inner[b: 1]]\n",
                "    |> WHEN { Outer[a] => a |> WHEN { Inner[b] => a } }\n",
            ),
            "value",
        );
        let rows = rows(&fixture);
        let arm_key = |tag: &str| {
            fixture
                .syntax
                .expressions
                .iter()
                .find_map(|expression| match &expression.kind {
                    AstExprKind::MatchArm {
                        pattern: AstMatchPattern::Tag { name, .. },
                        ..
                    } if name == tag => Some(expression.stable_key.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let scope = |key| {
            rows.scopes
                .iter()
                .find(|scope| {
                    matches!(
                        &scope.stable_key,
                        OwnerScopeStableKey::Expression {
                            expression,
                            role: boon_checked::OwnerExpressionScopeRole::MatchArm,
                        } if expression == key
                    )
                })
                .unwrap()
                .id
        };
        let outer_key = arm_key("Outer");
        let inner_key = arm_key("Inner");
        let outer_scope = scope(&outer_key);
        let inner_scope = scope(&inner_key);
        assert_eq!(
            rows.scopes[inner_scope.0 as usize].parent,
            Some(local_scope_ref(outer_scope))
        );
        let outer_binding = rows
            .declarations
            .iter()
            .find(|declaration| {
                declaration.kind == CheckedDeclarationKind::PatternBinding
                    && declaration.name == "a"
            })
            .unwrap();
        assert!(rows.expressions.iter().any(|expression| {
            expression.scope == local_scope_ref(inner_scope)
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ..
                    } if declaration == outer_binding.id
                )
        }));
    }

    #[test]
    fn nested_pattern_inside_arm_block_keeps_the_body_scope_in_its_parent_chain() {
        let fixture = fixture(
            concat!(
                "value:\n",
                "    Outer[outer: Inner[inner: 1]]\n",
                "    |> WHEN {\n",
                "        Outer[outer] => BLOCK {\n",
                "            outer |> WHEN { Inner[inner] => outer }\n",
                "        }\n",
                "    }\n",
            ),
            "value",
        );
        let rows = rows(&fixture);
        let arm_scope = |tag: &str| {
            let key = fixture
                .syntax
                .expressions
                .iter()
                .find_map(|expression| match &expression.kind {
                    AstExprKind::MatchArm {
                        pattern: AstMatchPattern::Tag { name, .. },
                        ..
                    } if name == tag => Some(&expression.stable_key),
                    _ => None,
                })
                .unwrap();
            rows.scopes
                .iter()
                .find(|scope| {
                    matches!(
                        &scope.stable_key,
                        OwnerScopeStableKey::Expression {
                            expression,
                            role: boon_checked::OwnerExpressionScopeRole::MatchArm,
                        } if expression == key
                    )
                })
                .unwrap()
                .id
        };
        let outer = arm_scope("Outer");
        let inner = arm_scope("Inner");
        let body = match rows.scopes[inner.0 as usize].parent.clone() {
            Some(OwnerScopeRef::Local { scope }) => scope,
            parent => panic!("inner arm must have a local body parent: {parent:?}"),
        };
        assert_ne!(body, outer);
        assert_eq!(
            rows.scopes[body.0 as usize].parent,
            Some(local_scope_ref(outer))
        );
        let outer_binding = rows
            .declarations
            .iter()
            .find(|declaration| {
                declaration.kind == CheckedDeclarationKind::PatternBinding
                    && declaration.name == "outer"
            })
            .unwrap();
        assert!(rows.expressions.iter().any(|expression| {
            expression.scope == local_scope_ref(inner)
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ..
                    } if declaration == outer_binding.id
                )
        }));
    }

    #[test]
    fn pattern_binding_wrapping_a_child_list_owner_is_retained() {
        let fixture = fixture(
            concat!(
                "value:\n",
                "    Found[item: 1]\n",
                "    |> WHEN {\n",
                "        Found[item] => LIST { item }\n",
                "    }\n",
            ),
            "<list>",
        );
        let rows = rows(&fixture);
        assert_eq!(rows.pattern_bindings.len(), 1);
        assert!(matches!(
            rows.pattern_bindings[0].selector,
            OwnerExpressionRef::Child { .. }
        ));
    }

    #[test]
    fn child_owner_reads_the_exact_forward_block_sibling() {
        let fixture = fixture(
            concat!(
                "item: TEXT { global }\n",
                "container: BLOCK {\n",
                "    item: 1\n",
                "    copy: item\n",
                "    copy\n",
                "}\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::ImportedStable {
                        declaration: OwnerDeclarationStableKey::Public,
                        ..
                    },
                    projection,
                    ..
                } if projection.is_empty()
            )
        }));
    }

    #[test]
    fn child_owner_reads_the_exact_forward_record_sibling() {
        let fixture = fixture(
            concat!(
                "item: TEXT { global }\n",
                "container: [\n",
                "    item: 1\n",
                "    copy: item\n",
                "]\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(
            rows.expressions.iter().any(|expression| {
                expression.flow_type.ty == Type::Number
                    && matches!(
                        &expression.kind,
                        OwnerExpressionKind::Read {
                            target: OwnerDeclarationRef::ImportedStable {
                                declaration: OwnerDeclarationStableKey::Public,
                                ..
                            },
                            projection,
                            ..
                        } if projection.is_empty()
                    )
            }),
            "child rows: {rows:#?}"
        );
    }

    #[test]
    fn wide_block_signature_children_share_one_effective_binding_environment() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    first: 1\n",
                "    second: 2\n",
                "    third: 3\n",
                "    third\n",
                "}\n",
            ),
            "container",
        );
        let environments = fixture.body.signature_lexical_plan.child_environments();

        assert!(environments.len() >= 3);
        assert!(environments[0].bindings.len() >= 3);
        assert!(environments.iter().skip(1).all(|environment| {
            crate::OwnerLexicalBoundaryBindings::ptr_eq(
                &environments[0].bindings,
                &environment.bindings,
            )
        }));
    }

    #[test]
    fn child_owner_nearest_nested_block_shadow_wins() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    item: 1\n",
                "    nested: BLOCK {\n",
                "        item: 2\n",
                "        copy: item\n",
                "        copy\n",
                "    }\n",
                "    nested\n",
                "}\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::ImportedStable {
                        owner: StableCheckOwnerKey::Item(owner),
                        declaration: OwnerDeclarationStableKey::Public,
                    },
                    projection,
                    ..
                } if projection.is_empty() && owner.item_route.segments().len() == 3
            )
        }));
    }

    #[test]
    fn child_owner_reads_the_exact_parent_pattern_binding() {
        let fixture = fixture(
            concat!(
                "item: TEXT { global }\n",
                "value: Found[item: 1]\n",
                "    |> WHEN {\n",
                "        Found[item] => BLOCK {\n",
                "            copy: item\n",
                "            copy\n",
                "        }\n",
                "    }\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::ImportedStable {
                        declaration: OwnerDeclarationStableKey::PatternBinding { .. },
                        ..
                    },
                    ref projection,
                    ..
                } if projection.is_empty()
            )
        }));
    }

    #[test]
    fn grandchild_owner_keeps_the_exact_parent_pattern_binding_and_type() {
        let fixture = fixture(
            concat!(
                "value: Found[item: 1]\n",
                "    |> WHEN {\n",
                "        Found[item] => BLOCK {\n",
                "            middle: BLOCK {\n",
                "                leaf: item\n",
                "                0\n",
                "            }\n",
                "            0\n",
                "        }\n",
                "    }\n",
            ),
            "leaf",
        );
        let rows = rows(&fixture);
        assert!(
            rows.expressions.iter().any(|expression| {
                expression.flow_type.ty == Type::Number
                    && matches!(
                        expression.kind,
                        OwnerExpressionKind::Read {
                            target: OwnerDeclarationRef::ImportedStable {
                                declaration: OwnerDeclarationStableKey::PatternBinding { .. },
                                ..
                            },
                            ref projection,
                            ..
                        } if projection.is_empty()
                    )
            }),
            "grandchild rows: {rows:#?}"
        );
    }

    #[test]
    fn public_reactive_capture_preserves_its_flow_mode() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    event: 1 |> THEN { 2 }\n",
                "    copy: event\n",
                "    copy\n",
                "}\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            expression.flow_type.mode == FlowMode::PresentOrAbsent
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::ImportedStable {
                            declaration: OwnerDeclarationStableKey::Public,
                            ..
                        },
                        ..
                    }
                )
        }));
    }

    #[test]
    fn imported_selector_projection_remains_lazy_until_pattern_narrowing() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    choice: Found[value: 1]\n",
                "    copy: choice |> WHEN { Found[value] => choice.value }\n",
                "    copy\n",
                "}\n",
            ),
            "copy",
        );
        let choice = fixture
            .interface
            .owner(&fixture.syntax.owner)
            .expect("copy interface")
            .lexical_captures
            .iter()
            .find(|capture| {
                matches!(
                    &capture.target,
                    OwnerLexicalTargetRef::Declaration {
                        declaration: OwnerDeclarationStableKey::Public,
                        ..
                    }
                )
            })
            .expect("choice lexical capture");
        assert_eq!(
            choice.demand_paths.as_ref(),
            &[Box::<[String]>::default()],
            "the active arm observes the complete selector before projecting its payload",
        );
        assert!(matches!(
            &choice.flow_type.ty,
            Type::VariantSet(variants)
                if variants.iter().any(|variant| matches!(
                    variant,
                    boon_checked::Variant::Tagged { tag, fields }
                        if tag == "Found"
                            && fields.fields.get("value") == Some(&Type::Number)
                            && !fields.open
                ))
        ));
        assert!(rows(&fixture).expressions.iter().any(|expression| {
            expression.flow_type.ty == Type::Number
                && matches!(
                    &expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::ImportedStable {
                            declaration: OwnerDeclarationStableKey::Public,
                            ..
                        },
                        projection,
                        ..
                    } if projection.as_ref() == ["value"]
                )
        }));
    }

    #[test]
    fn tagged_union_selector_projection_closes_a_generic_list_call_input() {
        fn assert_rows_are_exact(fixture: &Fixture) {
            let rows = rows(fixture);
            let projected = rows
                .expressions
                .iter()
                .find(|expression| {
                    matches!(
                        &expression.kind,
                        OwnerExpressionKind::Read { projection, .. }
                            if projection.as_ref() == ["rows"]
                    )
                })
                .expect("tagged arm must retain its projected selector read");
            assert!(
                matches!(
                    &projected.flow_type.ty,
                    Type::List(item)
                        if matches!(
                            item.as_ref(),
                            Type::Object(shape)
                                if shape.fields.get("signal_id") == Some(&Type::Text)
                                    && shape.fields.get("value") == Some(&Type::Number)
                        )
                ),
                "generic list input must receive the complete exact arm-local item type, got {:?}",
                projected.flow_type.ty
            );
        }

        let local = fixture(
            concat!(
                "FUNCTION probe() {\n",
                "    choice: LATEST {\n",
                "        NotStarted\n",
                "        Page[rows: LIST { [signal_id: TEXT { id }, value: 1] }]\n",
                "    }\n",
                "    result:\n",
                "        choice |> WHEN {\n",
                "            Page =>\n",
                "                choice.rows\n",
                "                |> List/find(item, if: item.signal_id == TEXT { id })\n",
                "            __ => NotFound\n",
                "        }\n",
                "    result\n",
                "}\n",
            ),
            "probe",
        );
        assert_rows_are_exact(&local);

        let imported = fixture(
            concat!(
                "store: [\n",
                "    choice: LATEST {\n",
                "        NotStarted\n",
                "        Page[rows: LIST { [signal_id: TEXT { id }, value: 1] }]\n",
                "    }\n",
                "    result:\n",
                "        choice |> WHEN {\n",
                "            Page =>\n",
                "                choice.rows\n",
                "                |> List/find(item, if: item.signal_id == TEXT { id })\n",
                "            __ => NotFound\n",
                "        }\n",
                "]\n",
            ),
            "result",
        );
        assert_rows_are_exact(&imported);

        let nested_source = concat!(
            "FUNCTION identity(value) { value }\n",
            "store: [\n",
            "    choice: LATEST {\n",
            "        NotStarted\n",
            "        Page[rows: LIST { [signal_id: TEXT { id }, value: 1] }]\n",
            "    }\n",
            "    result:\n",
            "        choice |> WHEN {\n",
            "            Page => identity(\n",
            "                value:\n",
            "                    choice.rows\n",
            "                    |> List/find(item, if: item.signal_id == TEXT { id })\n",
            "                    |> WHEN {\n",
            "                        Found[value] => value.value\n",
            "                        NotFound => 0\n",
            "                    }\n",
            "            )\n",
            "            __ => NotFound\n",
            "        }\n",
            "]\n",
        );
        let nested = fixture(nested_source, "result");
        assert_rows_are_exact(&nested);

        let child_source = concat!(
            "store: [\n",
            "    choice: LATEST {\n",
            "        NotStarted\n",
            "        Page[rows: LIST { [signal_id: TEXT { id }, value: 1] }]\n",
            "    }\n",
            "    result:\n",
            "        choice |> WHEN {\n",
            "            Page => [\n",
            "                outer: [\n",
            "                    nested:\n",
            "                        choice.rows\n",
            "                        |> List/find(item, if: item.signal_id == TEXT { id })\n",
            "                ]\n",
            "            ]\n",
            "            __ => []\n",
            "        }\n",
            "]\n",
        );
        let child = fixture(child_source, "nested");
        let environment = child
            .body
            .signature_lexical_plan
            .inherited_environment()
            .expect("nested grandchild field must inherit its provider environment");
        assert_eq!(environment.pattern_narrowings().len(), 1);
        assert!(matches!(
            &environment.pattern_narrowings()[0].pattern,
            OwnerPatternConstraint::Tag { name, .. } if name == "Page"
        ));
        assert_rows_are_exact(&child);
    }

    #[test]
    fn projected_pattern_binding_keeps_only_the_projected_payload_type() {
        let consumed = fixture(
            concat!(
                "FUNCTION probe() {\n",
                "    choice: LATEST {\n",
                "        Missing\n",
                "        Found[value: [transitions: LIST { [time: 1] }]]\n",
                "    }\n",
                "    result:\n",
                "        choice |> WHEN {\n",
                "            Found[value] => value.transitions\n",
                "            Missing => LIST {}\n",
                "        }\n",
                "        |> List/map(item, new: item.time)\n",
                "    result\n",
                "}\n",
            ),
            "probe",
        );
        let consumed_rows = rows(&consumed);
        let projected = consumed_rows
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    OwnerExpressionKind::Read { projection, .. }
                        if projection.as_ref() == ["transitions"]
                )
            })
            .expect("pattern binding projection must remain an exact read");
        assert!(
            matches!(
                &projected.flow_type.ty,
                Type::List(item)
                    if matches!(
                        item.as_ref(),
                        Type::Object(shape)
                            if shape.fields.get("time") == Some(&Type::Number)
                    )
            ),
            "projected pattern binding must be the transitions list, got {:?}",
            projected.flow_type.ty,
        );
        assert!(
            !format!("{:?}", projected.flow_type.ty).contains("Var("),
            "projected pattern binding must be closed: {:?}",
            projected.flow_type.ty,
        );
        let when = consumed_rows
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, OwnerExpressionKind::When { .. }))
            .expect("fixture must retain the inner WHEN");
        assert!(
            matches!(&when.flow_type.ty, Type::List(item) if !format!("{item:?}").contains("Var(")),
            "WHEN result must be one concrete list: {:?}",
            when.flow_type.ty,
        );
        let map = consumed
            .body
            .calls
            .iter()
            .find(|call| call.function == "List/map")
            .expect("fixture must retain List/map");
        let list_input = map
            .inputs
            .iter()
            .find(|input| matches!(input.role, OwnerConstraintEdgeRole::PipeInput))
            .expect("List/map must retain its pipeline list input");
        assert!(
            !format!("{:?}", list_input.actual_type).contains("Var("),
            "List/map list input must be concrete: {:#?}",
            list_input,
        );
        assert!(
            matches!(map.result.ty, Type::List(_)),
            "List/map must retain a list result: {:?}",
            map.result.ty,
        );

        let unconsumed = fixture(
            concat!(
                "FUNCTION probe() {\n",
                "    choice: LATEST {\n",
                "        Missing\n",
                "        Found[value: [transitions: LIST { [time: 1] }]]\n",
                "    }\n",
                "    result: choice |> WHEN {\n",
                "        Found[value] => value.transitions\n",
                "        Missing => LIST {}\n",
                "    }\n",
                "    result\n",
                "}\n",
            ),
            "probe",
        );
        let unconsumed_rows = rows(&unconsumed);
        let unconsumed_when = unconsumed_rows
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, OwnerExpressionKind::When { .. }))
            .expect("fixture must retain the unconsumed WHEN");
        assert!(
            matches!(
                &unconsumed_when.flow_type.ty,
                Type::List(item)
                    if matches!(
                        item.as_ref(),
                        Type::Object(shape)
                            if shape.fields.get("time") == Some(&Type::Number)
                    )
            ),
            "unconsumed empty branch must inherit its sibling List item: {:?}",
            unconsumed_when.flow_type.ty,
        );
    }

    #[test]
    fn inherited_pattern_narrowings_compose_outer_to_inner_for_a_grandchild_owner() {
        let fixture = fixture(
            concat!(
                "store: [\n",
                "    choice: LATEST {\n",
                "        Other[inner: Leaf[value: TEXT { wrong }]]\n",
                "        Outer[inner: Leaf[value: 1]]\n",
                "    }\n",
                "    result:\n",
                "        choice |> WHEN {\n",
                "            Outer => choice.inner |> WHEN {\n",
                "                Leaf => [\n",
                "                    outer: [\n",
                "                        nested: choice.inner.value\n",
                "                    ]\n",
                "                ]\n",
                "                __ => []\n",
                "            }\n",
                "            __ => []\n",
                "        }\n",
                "]\n",
            ),
            "nested",
        );
        let environment = fixture
            .body
            .signature_lexical_plan
            .inherited_environment()
            .expect("nested grandchild must inherit both active match arms");
        assert_eq!(environment.pattern_narrowings().len(), 2);
        assert!(matches!(
            &environment.pattern_narrowings()[0].pattern,
            OwnerPatternConstraint::Tag { name, .. } if name == "Outer"
        ));
        assert!(matches!(
            &environment.pattern_narrowings()[1].pattern,
            OwnerPatternConstraint::Tag { name, .. } if name == "Leaf"
        ));
        let capture = fixture
            .interface
            .owner(&fixture.syntax.owner)
            .expect("nested grandchild interface")
            .lexical_captures
            .first()
            .expect("nested grandchild must capture its selector provider");
        assert_eq!(
            capture.demand_paths.as_ref(),
            &[Box::<[String]>::default()],
            "the outer selector root dominates deeper inherited selector and read paths",
        );
        assert!(rows(&fixture).expressions.iter().any(|expression| {
            expression.flow_type.ty == Type::Number
                && matches!(
                    &expression.kind,
                    OwnerExpressionKind::Read { projection, .. }
                        if projection.as_ref() == ["inner", "value"]
                )
        }));
    }

    #[test]
    fn inherited_pattern_narrowing_does_not_materialize_an_impossible_closed_arm() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    choice: Other\n",
                "    result: choice |> WHEN {\n",
                "        Found => BLOCK {\n",
                "            child: choice\n",
                "            child\n",
                "        }\n",
                "        __ => []\n",
                "    }\n",
                "    result\n",
                "}\n",
            ),
            "child",
        );
        let interface = fixture
            .interface
            .owner(&fixture.syntax.owner)
            .expect("impossible-arm child interface");
        let other = Type::VariantSet(vec![boon_checked::Variant::Tag("Other".to_owned())].into());
        let capture = interface
            .lexical_captures
            .first()
            .expect("impossible-arm child must retain its exact selector capture");
        assert_eq!(capture.demand_paths.as_ref(), &[Box::<[String]>::default()]);
        assert_eq!(capture.flow_type.ty, other);
        assert!(
            matches!(
                &interface.result.ty,
                Type::VariantSet(variants)
                    if variants.as_ref().as_slice()
                        == [boon_checked::Variant::Tag("Other".to_owned())]
            ),
            "closed selector must not synthesize the impossible Found arm: {:#?}",
            interface.result.ty
        );
        assert!(!format!("{:?}", interface.result.ty).contains("Found"));
    }

    #[test]
    fn child_owner_ambiguous_block_binding_fails_closed() {
        let fixture = fixture(
            concat!(
                "item: TEXT { global }\n",
                "container: BLOCK {\n",
                "    item: 1\n",
                "    item: 2\n",
                "    copy: item\n",
                "    copy\n",
                "}\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "ambiguous_lexical_read")
            )
        }));
    }

    #[test]
    fn child_owner_drain_preserves_the_imported_static_target() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    item: 1\n",
                "    copy: DRAIN { item }\n",
                "    copy\n",
                "}\n",
            ),
            "copy",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Drain {
                    target: OwnerDeclarationRef::ImportedStable {
                        declaration: OwnerDeclarationStableKey::Public,
                        ..
                    },
                    ref projection,
                } if projection.is_empty()
            )
        }));
    }

    #[test]
    fn imported_store_capture_materializes_only_the_demanded_object_path() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    store: [\n",
                "        selected: [value: 1, omitted: TEXT { sibling }]\n",
                "        unrelated: [payload: TEXT { large }]\n",
                "    ]\n",
                "    observed: store.selected.value\n",
                "    observed\n",
                "}\n",
            ),
            "observed",
        );
        let interface = fixture
            .interface
            .owner(&fixture.syntax.owner)
            .expect("observed interface");
        let capture = interface
            .lexical_captures
            .iter()
            .find(|capture| {
                matches!(
                    &capture.target,
                    OwnerLexicalTargetRef::Declaration {
                        declaration: OwnerDeclarationStableKey::Public,
                        ..
                    }
                )
            })
            .expect("observed child must capture the enclosing store value");
        assert_eq!(
            capture.demand_paths.as_ref(),
            &[Box::<[String]>::from([
                "selected".to_owned(),
                "value".to_owned(),
            ])],
        );
        assert!(
            matches!(
                &capture.flow_type.ty,
                Type::Object(store)
                    if !store.open
                        && !store.fields.contains_key("unrelated")
                        && matches!(
                            store.fields.get("selected"),
                            Some(Type::Object(selected))
                                if !selected.open
                                    && selected.fields.get("value") == Some(&Type::Number)
                                    && !selected.fields.contains_key("omitted")
                        )
            ),
            "capture must contain only the selected.value surface: {:#?}",
            capture.flow_type.ty
        );
        assert!(rows(&fixture).expressions.iter().any(|expression| {
            expression.flow_type.ty == Type::Number
                && matches!(
                    &expression.kind,
                    OwnerExpressionKind::Read { projection, .. }
                        if projection.as_ref() == ["selected", "value"]
                )
        }));
    }

    #[test]
    fn child_owner_keeps_callable_only_bindings_non_readable() {
        let fixture = fixture(
            concat!(
                "container: BLOCK {\n",
                "    FUNCTION helper() { 1 }\n",
                "    copy: helper\n",
                "    copy\n",
                "}\n",
            ),
            "copy",
        );
        assert!(
            fixture
                .body
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "function_must_be_called" })
        );
        assert!(rows(&fixture).expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "function_must_be_called")
            )
        }));
    }

    #[test]
    fn lexical_statement_reads_do_not_fall_through_to_external_values() {
        let fixture = fixture(
            "FUNCTION identity(input) {\n    local: input\n    local\n}\n",
            "identity",
        );
        let rows = rows(&fixture);
        let local = rows
            .declarations
            .iter()
            .find(|declaration| declaration.name == "local")
            .unwrap();
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration },
                    ..
                } if declaration == local.id
            )
        }));
    }

    #[test]
    fn inline_record_fields_own_self_reads_while_spreads_only_read() {
        let fixture = fixture(
            "FUNCTION merge(base) {\n    [...base, item: item]\n}\n",
            "merge",
        );
        let rows = rows(&fixture);
        let field = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { ref name, .. } if name == "item"
                )
            })
            .expect("inline record field declaration");
        let parameter = rows
            .declarations
            .iter()
            .find(|declaration| declaration.kind == CheckedDeclarationKind::ValueParameter)
            .expect("spread input parameter");

        assert!(
            rows.scopes
                .iter()
                .any(|scope| scope.kind == CheckedScopeKind::Record)
        );
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration },
                    ref projection,
                    ..
                } if declaration == field.id && projection.is_empty()
            )
        }));
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration },
                    ref projection,
                    ..
                } if declaration == parameter.id && projection.is_empty()
            )
        }));
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                OwnerExpressionKind::Object { ref fields }
                    if fields.iter().any(|record| {
                        record.name == "item"
                            && record.declaration
                                == Some(OwnerDeclarationRef::Local {
                                    declaration: field.id,
                                })
                    })
            )
        }));
    }

    #[test]
    fn record_field_reads_use_forward_sibling_types_and_shadow_parameters() {
        let fixture = fixture(
            concat!(
                "FUNCTION make(item) {\n",
                "    [copy: item, item: 1]\n",
                "}\n",
            ),
            "make",
        );
        let rows = rows(&fixture);
        let item = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { ref name, .. } if name == "item"
                )
            })
            .expect("item record declaration");
        let copy = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { ref name, .. } if name == "copy"
                )
            })
            .expect("copy record declaration");
        assert_eq!(item.flow_type.ty, Type::Number);
        assert_eq!(copy.flow_type.ty, Type::Number);
        assert!(rows.expressions.iter().any(|expression| {
            expression.flow_type.ty == Type::Number
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ref projection,
                        ..
                    } if declaration == item.id && projection.is_empty()
                )
        }));
        let parameter = rows
            .declarations
            .iter()
            .find(|declaration| declaration.kind == CheckedDeclarationKind::ValueParameter)
            .unwrap();
        assert_ne!(parameter.id, item.id);
    }

    #[test]
    fn record_field_shadowing_inside_pattern_arms_controls_inference_and_lowering() {
        let fixture = fixture(
            concat!(
                "value:\n",
                "    Found[item: TEXT { outer }]\n",
                "    |> WHEN { Found[item] => [copy: item, item: 1] }\n",
            ),
            "value",
        );
        let rows = rows(&fixture);
        let copy = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    &declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { name, .. } if name == "copy"
                )
            })
            .unwrap();
        let OwnerDeclarationStableKey::RecordField {
            object: record_object,
            ..
        } = &copy.stable_key
        else {
            unreachable!()
        };
        let item = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    &declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { object, name, .. }
                        if object == record_object && name == "item"
                )
            })
            .unwrap();
        assert_eq!(item.flow_type.ty, Type::Number);
        assert_eq!(copy.flow_type.ty, Type::Number);
        assert!(rows.expressions.iter().any(|expression| {
            expression.flow_type.ty == Type::Number
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ref projection,
                        ..
                    } if declaration == item.id && projection.is_empty()
                )
        }));
        assert!(rows.pattern_bindings.iter().any(|binding| {
            rows.declarations[binding.declaration.0 as usize]
                .flow_type
                .ty
                == Type::Text
        }));
    }

    #[test]
    fn record_field_shadowing_the_match_selector_is_not_branch_narrowed() {
        let fixture = fixture(
            concat!(
                "FUNCTION shadow(value) {\n",
                "    value |> WHEN {\n",
                "        Found[item] => [copy: value, value: 1]\n",
                "    }\n",
                "}\n",
            ),
            "shadow",
        );
        let rows = rows(&fixture);
        let copy = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    &declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { name, .. } if name == "copy"
                )
            })
            .unwrap();
        let OwnerDeclarationStableKey::RecordField {
            object: record_object,
            ..
        } = &copy.stable_key
        else {
            unreachable!()
        };
        let value = rows
            .declarations
            .iter()
            .find(|declaration| {
                matches!(
                    &declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { object, name, .. }
                        if object == record_object && name == "value"
                )
            })
            .unwrap();
        assert_eq!(value.flow_type.ty, Type::Number);
        assert_eq!(copy.flow_type.ty, Type::Number);
        assert!(rows.expressions.iter().any(|expression| {
            expression.flow_type.ty == Type::Number
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ref projection,
                        ..
                    } if declaration == value.id && projection.is_empty()
                )
        }));
    }

    #[test]
    fn duplicate_record_fields_do_not_select_an_arbitrary_lexical_target() {
        let fixture = fixture(
            "FUNCTION make() {\n    [item: 1, item: 2, copy: item]\n}\n",
            "make",
        );
        let rows = rows(&fixture);
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "ambiguous_lexical_read")
            )
        }));
        let item_declarations = rows
            .declarations
            .iter()
            .filter(|declaration| {
                matches!(
                    declaration.stable_key,
                    OwnerDeclarationStableKey::RecordField { ref name, .. } if name == "item"
                )
            })
            .map(|declaration| declaration.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(item_declarations.len(), 2);
        assert!(rows.expressions.iter().all(|expression| {
            !matches!(
                expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration },
                    ..
                } if item_declarations.contains(&declaration)
            )
        }));
        assert!(fixture.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "duplicate_record_field"
                && diagnostic.message == "duplicate explicit record field `item`"
        }));
    }

    #[test]
    fn duplicate_record_fields_with_child_owned_values_keep_parent_diagnostic() {
        let fixture = fixture(
            "record: [\n    items: LIST { 1 }\n    items: LIST { 2 }\n]\n",
            "record",
        );
        assert!(fixture.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "duplicate_record_field"
                && diagnostic.message == "duplicate explicit record field `items`"
                && diagnostic.role == Some(OwnerSourceAnchorRole::RecordField { ordinal: 1 })
        }));
    }

    #[test]
    fn bare_function_value_reports_a_user_diagnostic_instead_of_failing_inference() {
        let fixture = fixture("FUNCTION repeat(input) {\n    repeat\n}\n", "repeat");
        assert!(fixture.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "function_must_be_called"
                && diagnostic
                    .message
                    .contains("function `repeat` must be called with parentheses")
        }));
        assert!(rows(&fixture).expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "function_must_be_called")
            )
        }));
    }

    #[test]
    fn resolved_cross_owner_function_value_is_diagnosed_and_lowered_fail_closed() {
        let base = fixture("value: helper\n", "value");
        let reference = base.seed.references.first().cloned().unwrap();
        let fixture = fixture_with_symbol_resolution(
            base,
            OwnerSymbolResolution::CallableAsValue { reference },
        );

        assert!(fixture.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "function_must_be_called"
                && diagnostic
                    .message
                    .contains("function `helper` must be called")
        }));
        assert!(rows(&fixture).expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "function_must_be_called")
            )
        }));
    }

    #[test]
    fn unresolved_and_ambiguous_values_are_diagnosed_and_lowered_fail_closed() {
        let unresolved = fixture("value: typo\n", "value");
        assert!(unresolved.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "unknown_identifier"
                && diagnostic.message == "unknown identifier `typo`"
        }));
        assert!(rows(&unresolved).expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "unknown_identifier")
            )
        }));

        let base = fixture("value: item\n", "value");
        let reference = base.seed.references.first().cloned().unwrap();
        let candidate = crate::AmbiguousOwnerSymbolCandidate {
            owner: base.seed.owner.clone(),
            parameters: Box::new([]),
        };
        let ambiguous = fixture_with_symbol_resolution(
            base,
            OwnerSymbolResolution::Ambiguous {
                reference,
                candidates: vec![candidate.clone(), candidate].into_boxed_slice(),
            },
        );
        assert!(ambiguous.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "ambiguous_value"
                && diagnostic
                    .message
                    .contains("2 equally ranked project targets")
        }));
        assert!(rows(&ambiguous).expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "ambiguous_value")
            )
        }));
    }

    #[test]
    fn passed_outside_a_callable_is_unbound_and_owns_no_context_formal() {
        let fixture = fixture("value: PASSED.store\n", "value");
        assert!(
            fixture
                .interface
                .owner(&fixture.syntax.owner)
                .is_some_and(|interface| interface.context.is_none())
        );
        assert!(
            fixture
                .body
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "unbound_passed_context" })
        );
        let rows = rows(&fixture);
        assert!(rows.context_formals.is_empty());
        assert!(rows.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Invalid { tokens }
                    if tokens.first().is_some_and(|token| token == "unbound_passed_context")
            )
        }));
    }

    #[test]
    fn non_callable_roots_cannot_inherit_a_callee_pass_context() {
        let missing = fixture(
            concat!(
                "FUNCTION leaf() {\n",
                "    PASSED.store.count\n",
                "}\n",
                "value: leaf()\n",
            ),
            "value",
        );
        assert!(
            missing
                .interface
                .owner(&missing.syntax.owner)
                .is_some_and(|interface| interface.context.is_none())
        );
        assert!(missing.body.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "missing_pass_context"
                && diagnostic
                    .message
                    .contains("requires a final `PASS:` clause")
        }));
        assert!(matches!(missing.body.calls.as_ref(), [call] if !call.valid));
        let missing_rows = rows(&missing);
        assert!(missing_rows.context_formals.is_empty());
        assert!(missing_rows.calls.is_empty());

        let explicit = fixture(
            concat!(
                "FUNCTION leaf() {\n",
                "    PASSED.store.count\n",
                "}\n",
                "value: leaf(PASS: [store: [count: 1]])\n",
            ),
            "value",
        );
        assert!(
            !explicit
                .body
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "missing_pass_context")
        );
        assert!(matches!(explicit.body.calls.as_ref(), [call] if call.valid));
        assert_eq!(rows(&explicit).calls.len(), 1);

        let inherited = fixture(
            concat!(
                "FUNCTION leaf() {\n",
                "    PASSED.store.count\n",
                "}\n",
                "FUNCTION wrapper() {\n",
                "    leaf()\n",
                "}\n",
            ),
            "wrapper",
        );
        assert!(
            inherited
                .interface
                .owner(&inherited.syntax.owner)
                .is_some_and(|interface| interface.context.is_some())
        );
        assert!(
            !inherited
                .body
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "missing_pass_context")
        );
        assert!(matches!(inherited.body.calls.as_ref(), [call] if call.valid));
        let inherited_rows = rows(&inherited);
        assert_eq!(inherited_rows.context_formals.len(), 1);
        assert_eq!(inherited_rows.calls.len(), 1);
    }

    #[test]
    fn unread_record_field_keeps_its_state_anchor_when_a_sibling_starts_reading_it() {
        let before = fixture("record: [state: 0 |> HOLD state {}, copy: 0]\n", "record");
        let after = fixture(
            "record: [state: 0 |> HOLD state {}, copy: state]\n",
            "record",
        );

        let anchor = |fixture: &Fixture| {
            let rows = rows(fixture);
            let state = rows.states.first().expect("record field state row");
            let OwnerDeclarationRef::Local { declaration } = &state.path.anchor else {
                panic!("record field state must have a local declaration anchor");
            };
            rows.declarations[declaration.0 as usize].stable_key.clone()
        };
        let before_anchor = anchor(&before);
        let after_anchor = anchor(&after);
        assert_eq!(before_anchor, after_anchor);
        assert!(matches!(
            before_anchor,
            OwnerDeclarationStableKey::RecordField { ref name, .. } if name == "state"
        ));
    }

    #[test]
    fn owner_occurrences_use_source_owner_rows_and_exact_relocations() {
        let local = fixture("FUNCTION identity(input) {\n    input\n}\n", "identity");
        let local_rows = rows(&local);
        assert_eq!(
            local_rows
                .occurrences
                .iter()
                .filter(|occurrence| occurrence.kind == SemanticOccurrenceKind::Declaration)
                .count(),
            local_rows.declarations.len()
        );
        let parameter = local_rows.callables[0].parameters[0].declaration;
        assert!(local_rows.occurrences.iter().any(|occurrence| {
            occurrence.kind == SemanticOccurrenceKind::Read
                && occurrence.target == local_declaration_ref(parameter)
                && matches!(occurrence.source, OwnerSourceSite::Expression { .. })
        }));

        let authoritative = fixture("value: Number/to_text(value: 1)\n", "value");
        let (authoritative_rows, receipts) = built_rows(&authoritative);
        assert!(authoritative_rows.occurrences.iter().any(|occurrence| {
            occurrence.kind == SemanticOccurrenceKind::Call
                && matches!(occurrence.target, OwnerDeclarationRef::Abi { .. })
                && matches!(occurrence.source, OwnerSourceSite::Expression { .. })
        }));
        let occurrence_receipt = receipts
            .row_receipts
            .iter()
            .find(|receipt| receipt.domain == OwnerCheckedRowDomain::Occurrence)
            .unwrap();
        assert!(
            occurrence_receipt.relocations.len > 0
                || receipts.row_receipts.iter().any(|receipt| {
                    receipt.domain == OwnerCheckedRowDomain::Occurrence
                        && receipt.relocations.len > 0
                })
        );

        let forwarded = fixture(
            concat!(
                "FUNCTION wrap(list, row: OUT) {\n",
                "    list |> List/sort_by(item: row, key: row.rank, direction: Ascending)\n",
                "}\n",
            ),
            "wrap",
        );
        let forwarded_rows = rows(&forwarded);
        let row = forwarded_rows.callables[0].parameters[1].declaration;
        assert!(forwarded_rows.occurrences.iter().any(|occurrence| {
            occurrence.kind == SemanticOccurrenceKind::ForwardOut
                && occurrence.target == local_declaration_ref(row)
                && matches!(
                    occurrence.source,
                    OwnerSourceSite::PipeArgument { ordinal: 0, .. }
                )
        }));

        let nested = fixture(
            concat!(
                "ordered: LIST { [rank: 1] }\n",
                "    |> List/sort_by(\n",
                "        item,\n",
                "        key: LIST { [rank: 1] }\n",
                "            |> List/find(item: item, if: True),\n",
                "        direction: Ascending,\n",
                "    )\n",
            ),
            "ordered",
        );
        let nested_rows = rows(&nested);
        let outer_output = nested_rows
            .calls
            .iter()
            .find(|call| call.function == "List/sort_by")
            .and_then(|call| {
                call.entries.iter().find_map(|entry| match entry {
                    OwnerCallEntry::FreshOut { output, .. } => Some(*output),
                    _ => None,
                })
            })
            .unwrap();
        assert!(nested_rows.calls.iter().any(|call| {
            call.function == "List/find"
                && call.entries.iter().any(|entry| {
                    matches!(
                        entry,
                        OwnerCallEntry::ForwardOut {
                            target: OwnerDeclarationRef::Local { declaration },
                            ..
                        } if *declaration == outer_output
                    )
                })
        }));
        assert!(nested_rows.occurrences.iter().any(|occurrence| {
            occurrence.kind == SemanticOccurrenceKind::ForwardOut
                && occurrence.target == local_declaration_ref(outer_output)
                && matches!(
                    occurrence.source,
                    OwnerSourceSite::PipeArgument { ordinal: 0, .. }
                )
        }));
    }

    #[test]
    fn output_scope_reparents_nested_record_without_flattening_authored_scope() {
        let fixture = fixture(
            concat!(
                "FUNCTION wrap(list) {\n",
                "    list |> List/sort_by(\n",
                "        item,\n",
                "        key: [\n",
                "            rank: LIST { [rank: 1] }\n",
                "                |> List/find(item, if: item.rank == 1),\n",
                "        ],\n",
                "        direction: Ascending,\n",
                "    )\n",
                "}\n",
            ),
            "wrap",
        );
        let rows = rows(&fixture);
        let (_, item_scope) = rows
            .calls
            .iter()
            .find(|call| call.function == "List/sort_by")
            .and_then(|call| {
                call.entries.iter().find_map(|entry| match entry {
                    OwnerCallEntry::FreshOut {
                        name,
                        output,
                        scope_id,
                        ..
                    } if name == "item" => Some((*output, *scope_id)),
                    _ => None,
                })
            })
            .expect("sort call must publish its item OUT");
        let (inner_item, inner_scope) = rows
            .calls
            .iter()
            .find(|call| call.function == "List/find")
            .and_then(|call| {
                call.entries.iter().find_map(|entry| match entry {
                    OwnerCallEntry::FreshOut {
                        name,
                        output,
                        scope_id,
                        ..
                    } if name == "item" => Some((*output, *scope_id)),
                    _ => None,
                })
            })
            .expect("nested find call must publish its own item OUT");
        let OwnerScopeRef::Local {
            scope: object_scope,
        } = rows.scopes[inner_scope.0 as usize]
            .parent
            .as_ref()
            .expect("inner output must retain its authored parent")
        else {
            panic!("inner output must be nested under a local authored scope");
        };
        let object_scope = *object_scope;
        assert_eq!(
            rows.scopes[object_scope.0 as usize].kind,
            CheckedScopeKind::Record
        );
        assert_eq!(
            rows.scopes[object_scope.0 as usize].parent,
            Some(local_scope_ref(item_scope))
        );
        assert!(rows.expressions.iter().any(|expression| {
            expression.scope == local_scope_ref(inner_scope)
                && matches!(
                    expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ref projection,
                        ..
                    } if declaration == inner_item && projection == &["rank"]
                )
        }));
    }

    #[test]
    fn invalid_recursive_call_does_not_poison_contextual_scope_projection() {
        let fixture = fixture(
            concat!(
                "FUNCTION recur(items, item: OUT, value) {\n",
                "    BLOCK {\n",
                "        ignored: recur(wrong: value)\n",
                "        result: items |> List/sort_by(\n",
                "            item: item,\n",
                "            key: value,\n",
                "            direction: Ascending,\n",
                "        )\n",
                "        result\n",
                "    }\n",
                "}\n",
            ),
            "recur",
        );
        let interface = fixture.interface.owner(&fixture.syntax.owner).unwrap();
        assert!(interface.parameters.iter().any(|parameter| {
            parameter.name == "value"
                && parameter.evaluation_scope
                    == OwnerInterfaceEvaluationScope::Output {
                        parameter_ordinal: 1,
                    }
        }));
        assert!(fixture.body.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "unexpected_call_entry" | "misordered_call_entry" | "missing_call_entry"
            )
        }));
    }

    #[test]
    fn invalid_late_forward_out_clears_earlier_fresh_binding_and_keeps_diagnostic() {
        let fixture = fixture(
            concat!(
                "FUNCTION fill_both(first: OUT, second: OUT) {\n",
                "    first\n",
                "}\n",
                "FUNCTION caller(existing) {\n",
                "    values: LIST { existing }\n",
                "    fill_both(first, second: existing)\n",
                "}\n",
            ),
            "caller",
        );
        assert!(
            fixture
                .body
                .signature_lexical_plan
                .declarations()
                .is_empty()
        );
        let inferred_call = fixture
            .body
            .calls
            .iter()
            .find(|call| call.function == "fill_both")
            .expect("invalid lexical call remains an inference fact");
        assert!(!inferred_call.valid);
        assert_eq!(
            inferred_call.effect,
            boon_checked::CheckedEffectSummary::default()
        );
        assert!(
            fixture
                .body
                .signature_lexical_plan
                .calls()
                .iter()
                .any(|call| {
                    call.function == "fill_both"
                        && !call.valid
                        && matches!(
                            call.lexical_errors.as_ref(),
                            [OwnerSignatureCallLexicalError::MissingEnclosingOut {
                                target_name,
                                ..
                            }] if target_name == "existing"
                        )
                }),
            "{:#?}",
            fixture.body.signature_lexical_plan.calls()
        );
        let rows = rows(&fixture);
        assert!(rows.calls.is_empty());
        assert!(
            rows.declarations.iter().all(|declaration| {
                !matches!(declaration.kind, CheckedDeclarationKind::FreshOut)
            })
        );
        assert_eq!(
            fixture
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "missing_enclosing_out")
                .count(),
            1
        );
        let construction = OwnerRowConstruction::new(
            &fixture.syntax,
            &fixture.lexical_plan,
            &fixture.seed,
            &fixture.summary,
            &fixture.body,
            fixture.interface.owner(&fixture.syntax.owner).unwrap(),
            &fixture.construction_abi,
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(
            construction
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "missing_enclosing_out")
                .count(),
            1
        );
    }

    #[test]
    fn drain_cannot_be_used_as_a_forward_out_binding() {
        let fixture = fixture(
            concat!(
                "FUNCTION sink(item: OUT) {\n",
                "    item\n",
                "}\n",
                "FUNCTION wrapper(row: OUT) {\n",
                "    sink(item: DRAIN { row })\n",
                "}\n",
            ),
            "wrapper",
        );
        let planned = fixture
            .body
            .signature_lexical_plan
            .calls()
            .iter()
            .find(|call| call.function == "sink")
            .expect("sink call must have a lexical plan");
        assert!(!planned.valid);
        assert!(matches!(
            planned.lexical_errors.as_ref(),
            [OwnerSignatureCallLexicalError::InvalidForwardOutTarget { .. }]
        ));
        let inferred = fixture
            .body
            .calls
            .iter()
            .find(|call| call.function == "sink")
            .expect("invalid sink call remains an inference fact");
        assert!(!inferred.valid);
        assert_eq!(
            inferred.effect,
            boon_checked::CheckedEffectSummary::default()
        );
        assert!(rows(&fixture).calls.is_empty());
        assert_eq!(
            fixture
                .body
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "invalid_forward_out_target")
                .count(),
            1
        );
    }

    #[test]
    fn flush_control_type_is_emitted_by_owner_body_inference() {
        let fixture = fixture("value: FLUSH { Error }\n", "value");
        let rows = rows(&fixture);
        let flush = rows
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, OwnerExpressionKind::Flush { .. }))
            .unwrap();
        assert_eq!(
            flush.flush_type,
            Some(Type::VariantSet(
                vec![Variant::Tag("Error".to_owned())].into()
            ))
        );
        let declaration = rows
            .declarations
            .iter()
            .find(|declaration| declaration.name == "value")
            .unwrap();
        assert_eq!(
            declaration.flow_type.ty,
            crate::union_structural_type(&flush.flow_type.ty, flush.flush_type.as_ref().unwrap())
        );
        assert_eq!(declaration.flow_type.mode, FlowMode::Continuous);
        assert!(fixture.body.diagnostics.is_empty());
    }

    #[test]
    fn invalid_flush_payload_is_a_stable_owner_diagnostic() {
        let fixture = fixture("value: FLUSH { 1 }\n", "value");
        assert!(
            fixture
                .body
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "invalid_flush_payload")
        );
        let (_, receipts) = built_rows(&fixture);
        assert!(
            receipts
                .construction
                .domain_counts
                .iter()
                .any(|count| count.domain == OwnerCheckedRowDomain::Diagnostic && count.rows == 1)
        );
    }

    #[test]
    fn render_slot_use_survives_the_owner_boundary() {
        let render = fixture("document: [\n    root: 1\n]\n", "root");
        assert_eq!(
            rows(&render).statements[0].value_use,
            boon_checked::CheckedValueUse::RenderSlot
        );

        let ordinary = fixture("root: 1\n", "root");
        assert_eq!(
            rows(&ordinary).statements[0].value_use,
            boon_checked::CheckedValueUse::RuntimeValue
        );
    }

    #[test]
    fn owner_resources_are_emitted_with_statement_bindings_and_receipts() {
        let source = fixture("events: SOURCE\n", "events");
        let (source_rows, source_receipts) = built_rows(&source);
        assert_eq!(source_rows.sources.len(), 1);
        assert!(matches!(
            source_rows.statements[0].resources.as_slice(),
            [OwnerResourceBinding::Source {
                source: boon_checked::OwnerSourceRef::Local {
                    source: OwnerSourceId(0)
                }
            }]
        ));
        assert!(
            source_receipts
                .construction
                .domain_counts
                .iter()
                .any(|count| count.domain == OwnerCheckedRowDomain::Source && count.rows == 1)
        );

        let state = fixture("state: 1 |> HOLD state {}\n", "state");
        let state_rows = rows(&state);
        assert_eq!(state_rows.states.len(), 1);
        assert_eq!(state_rows.states[0].kind, CheckedStateKind::Hold);

        let list = fixture("items: LIST { 1, 2 }\n", "items");
        let list_rows = rows(&list);
        assert_eq!(list_rows.lists.len(), 1);
        assert_eq!(list_rows.lists[0].item_type, Type::Number);

        let update_source = "container:\n    1 |> HOLD state {\n        LATEST {\n            2\n            3\n        }\n    }\n";
        let update_authority = fixture(update_source, "container");
        let update_rows = rows(&update_authority);
        let update_body = fixture(update_source, "state");
        let (update_body_rows, update_body_receipts) = built_rows(&update_body);
        let latest = update_body_rows
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, OwnerExpressionKind::Latest { .. }))
            .unwrap();
        assert!(latest.effect.reads_state && latest.effect.writes_state);
        assert!(update_rows.states.is_empty());
        assert_eq!(update_body_rows.states.len(), 1);
        assert_eq!(update_body_rows.states[0].kind, CheckedStateKind::Hold);
        assert!(matches!(
            update_body_rows.states[0].declaration,
            OwnerDeclarationRef::ScopeOwner {
                scope: OwnerScopeRef::Imported { .. }
            }
        ));
        assert!(
            update_body_receipts
                .row_receipts
                .iter()
                .any(|receipt| receipt.domain == OwnerCheckedRowDomain::State
                    && receipt.relocations.len > 0),
            "a child-owner state must receipt its lexical declaration relocation"
        );

        let inline = fixture("FUNCTION make() {\n    [items: LIST { 1 }]\n}\n", "make");
        let inline_rows = rows(&inline);
        assert_eq!(inline_rows.lists.len(), 1);
        assert_ne!(
            inline_rows.lists[0].statement,
            inline_rows.callables[0].body.unwrap(),
            "an inline list belongs to its containing expression statement"
        );
    }

    #[test]
    fn fieldless_hold_aliases_materialize_exactly_one_declaration_authority() {
        let local_source = concat!(
            "FUNCTION update() {\n",
            "    0 |> HOLD state {\n",
            "        state + 1\n",
            "    }\n",
            "}\n",
        );
        let local = fixture(local_source, "update");
        let local_rows = rows(&local);
        let local_hold = local_rows
            .statements
            .iter()
            .find_map(|statement| match &statement.kind {
                OwnerStatementKind::Hold {
                    declaration: Some(declaration),
                    name: Some(name),
                } if name == "state" => Some(*declaration),
                _ => None,
            })
            .expect("a function-local fieldless HOLD must own one declaration");
        assert!(matches!(
            local_rows.declarations[local_hold.0 as usize].stable_key,
            OwnerDeclarationStableKey::Statement { .. }
        ));
        assert_eq!(local_rows.declarations[local_hold.0 as usize].name, "state");
        assert!(local_rows.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                OwnerExpressionKind::Read {
                    target: OwnerDeclarationRef::Local { declaration },
                    ..
                } if *declaration == local_hold
            )
        }));
        assert!(local_rows.states.iter().any(|state| {
            state.declaration
                == (OwnerDeclarationRef::Local {
                    declaration: local_hold,
                })
        }));

        let nested_local_source = concat!(
            "FUNCTION update() {\n",
            "    0 |> HOLD outer {\n",
            "        1 |> HOLD inner {\n",
            "            inner + outer\n",
            "        }\n",
            "    }\n",
            "}\n",
        );
        let nested_local = fixture(nested_local_source, "update");
        let nested_local_rows = rows(&nested_local);
        let hold_statements = nested_local_rows
            .statements
            .iter()
            .filter_map(|statement| match &statement.kind {
                OwnerStatementKind::Hold { declaration, name } => {
                    Some((name.as_deref(), *declaration))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(hold_statements.len(), 2);
        let outer = hold_statements
            .iter()
            .find_map(|(name, declaration)| (*name == Some("outer")).then_some(*declaration))
            .flatten()
            .expect("the outer fieldless HOLD must own the declaration");
        assert!(
            hold_statements
                .iter()
                .any(|(name, declaration)| *name == Some("inner") && declaration.is_none())
        );
        for (syntax, expression) in nested_local
            .syntax
            .expressions
            .iter()
            .zip(&nested_local_rows.expressions)
        {
            if matches!(&syntax.kind, AstExprKind::Identifier(name) if name == "outer" || name == "inner")
            {
                assert!(matches!(
                    &expression.kind,
                    OwnerExpressionKind::Read {
                        target: OwnerDeclarationRef::Local { declaration },
                        ..
                    } if *declaration == outer
                ));
            }
        }
        let mut local_state_paths = nested_local_rows
            .states
            .iter()
            .filter(|state| {
                state.declaration == (OwnerDeclarationRef::Local { declaration: outer })
            })
            .map(|state| state.path.projection.clone())
            .collect::<Vec<_>>();
        local_state_paths.sort();
        assert_eq!(
            local_state_paths,
            vec![Vec::<String>::new(), vec!["state_1".to_owned()]],
            "the declaration result and its nested HOLD need distinct dense-compatible paths"
        );

        let imported_source = concat!(
            "container:\n",
            "    0 |> HOLD outer {\n",
            "        1 |> HOLD inner {\n",
            "            inner + outer\n",
            "        }\n",
            "    }\n",
        );
        let container = fixture(imported_source, "container");
        for name in ["outer", "inner"] {
            let update = fixture(imported_source, name);
            let update_rows = rows(&update);
            assert!(update_rows.statements.iter().all(|statement| {
                !matches!(
                    statement.kind,
                    OwnerStatementKind::Hold {
                        declaration: Some(_),
                        ..
                    }
                )
            }));
            assert!(update_rows.declarations.is_empty());
            let [state] = update_rows.states.as_slice() else {
                panic!("fieldless HOLD owner `{name}` must emit exactly one state row");
            };
            if name == "outer" {
                assert!(
                    state.path.projection.is_empty(),
                    "the outer HOLD is the enclosing declaration's result authority"
                );
            } else {
                assert_eq!(
                    state.path.projection,
                    ["state_1".to_owned()],
                    "the nested child owner must include its stable inherited state ordinal"
                );
            }
            for (syntax, expression) in update
                .syntax
                .expressions
                .iter()
                .zip(&update_rows.expressions)
            {
                if matches!(&syntax.kind, AstExprKind::Identifier(alias) if alias == "outer" || alias == "inner")
                {
                    assert!(matches!(
                        &expression.kind,
                        OwnerExpressionKind::Read {
                            target:
                                OwnerDeclarationRef::ImportedStable {
                                    owner,
                                    declaration: OwnerDeclarationStableKey::Public,
                                },
                            ..
                        } if owner == &container.syntax.owner
                    ));
                }
            }
        }

        let sibling_source = concat!(
            "container:\n",
            "    LATEST {\n",
            "        0 |> HOLD first { first + 1 }\n",
            "        0 |> HOLD second { second + 1 }\n",
            "    }\n",
        );
        for (name, expected) in [("first", "state_0"), ("second", "state_1")] {
            let update = fixture(sibling_source, name);
            let update_rows = rows(&update);
            let [state] = update_rows.states.as_slice() else {
                panic!("sibling HOLD owner `{name}` must emit exactly one state row");
            };
            assert_eq!(
                state.path.projection,
                [expected.to_owned()],
                "sibling HOLD ordinals must follow authored source order even when aliases differ"
            );
        }
    }

    #[test]
    fn owner_projection_seeds_and_call_result_paths_use_stable_local_anchors() {
        let projection = fixture("FUNCTION pick(input) {\n    input.value\n}\n", "pick");
        let projection_rows = rows(&projection);
        assert_eq!(projection_rows.resource_projection_seeds.len(), 1);
        assert_eq!(
            projection_rows.resource_projection_seeds[0].projection,
            ["value"]
        );

        let call = fixture("value: [text: Number/to_text(value: 1)]\n", "value");
        let call_rows = rows(&call);
        assert_eq!(call_rows.call_result_paths.len(), 1);
        let path = &call_rows.call_result_paths[0];
        assert!(path.projection.is_empty());
        let OwnerDeclarationRef::Local { declaration } = &path.anchor else {
            panic!("inline record call result must use its field declaration anchor");
        };
        assert!(matches!(
            &call_rows.declarations[declaration.0 as usize].stable_key,
            OwnerDeclarationStableKey::RecordField { name, .. } if name == "text"
        ));
    }

    #[test]
    fn user_calls_do_not_duplicate_sources_owned_by_the_callee() {
        let source = concat!(
            "FUNCTION source_factory() {\n",
            "    event: SOURCE\n",
            "    event\n",
            "}\n",
            "FUNCTION wrapper() {\n",
            "    source_factory()\n",
            "}\n",
            "value: wrapper()\n",
        );
        let source_factory = rows(&fixture(source, "source_factory"));
        let wrapper = rows(&fixture(source, "wrapper"));

        assert_eq!(source_factory.sources.len(), 1);
        assert!(wrapper.sources.is_empty());
    }
}
