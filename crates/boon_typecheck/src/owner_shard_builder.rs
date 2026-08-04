//! Construction of immutable checked rows for one authored owner.
//!
//! The builder consumes only span-free owner requests plus frozen interfaces
//! and the authoritative ABI.  It never opens `CheckedProgramDatabase` or a
//! project-wide checked image.  Linked dense identities and source spans are
//! deliberately deferred to the non-checking compatibility assembler.

use crate::{
    InferredOwnerCall, InferredOwnerCallableTarget, OwnerAbiCallableContract, OwnerAbiEnvironment,
    OwnerAbiEvaluationScope, OwnerArgumentKind, OwnerBodyInferenceShard, OwnerCheckedReceiptSink,
    OwnerConstraintEdgeRole, OwnerConstraintSeed, OwnerConstraintSummary,
    OwnerContainingScopeInput, OwnerDeclarationKind, OwnerInferenceExpressionRef,
    OwnerInterfaceEvaluationScope, OwnerInterfaceSccResult, OwnerParameterKind,
    OwnerPublicInterface, OwnerSourceAnchorSite, OwnerSyntaxGraph, OwnerSyntaxInput,
};
use boon_checked::{
    CheckedCallContextKind, CheckedCallableKind, CheckedDeclarationKind, CheckedIntrinsicV1,
    CheckedOwnerRows, CheckedParameterKind, CheckedParameterRequirement, CheckedScopeKind,
    CheckedValueUse, FlowMode, FlowType, OwnerAbiDeclarationKey, OwnerAbiDeclarationKind,
    OwnerAbiMemberRef, OwnerBlockBinding, OwnerCallContextRow, OwnerCallEntry, OwnerCallId,
    OwnerCallRow, OwnerCallableContextRow, OwnerCallableRow, OwnerCheckedReceiptSet,
    OwnerCheckedRowDomain, OwnerContextBinding, OwnerContextFormalId, OwnerContextFormalRef,
    OwnerContextFormalRow, OwnerDeclarationId, OwnerDeclarationRef, OwnerDeclarationRow,
    OwnerDeclarationStableKey, OwnerEvaluationScope, OwnerExpressionId, OwnerExpressionKind,
    OwnerExpressionRef, OwnerExpressionRow, OwnerInterfaceMemberRef, OwnerParameterRow,
    OwnerRecordField, OwnerRelocationTarget, OwnerScopeId, OwnerScopeRef, OwnerScopeRow,
    OwnerScopeStableKey, OwnerSourceSite, OwnerStatementId, OwnerStatementKind, OwnerStatementRow,
    OwnerStatementScopeRole, OwnerTextSegment, OwnerTypeSubstitution, ProgramRole, Type,
};
use boon_data::{Bits, ExactNumber};
use boon_syntax::{
    AstDrainPath, AstExprKind, AstMatchPattern, AstStatementKind, AstTextSegment, BytesSizeSyntax,
    StableCheckOwnerKey, StableExpressionKey,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

const CHECKED_OWNER_SHARD_DOMAIN_V1: &[u8] = b"boon.checked-owner-shard.v1\0";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckedOwnerShardBasis {
    pub owner: StableCheckOwnerKey,
    pub syntax_fingerprint_v1: [u8; 32],
    pub seed_fingerprint_v1: [u8; 32],
    pub summary_fingerprint_v1: [u8; 32],
    pub body_fingerprint_v1: [u8; 32],
    pub own_interface_scc_fingerprint_v1: [u8; 32],
    pub authoritative_abi_fingerprint_v1: [u8; 32],
}

/// Complete span-free checked result for one stable authored owner.
///
/// Diagnostics retain owner source-anchor templates and are materialized
/// against the independently current `OwnerSourceMap`.  `receipts` are
/// construction-owned proof material; the compatibility linker consumes them
/// without rescanning `rows`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedOwnerShard {
    pub basis: CheckedOwnerShardBasis,
    pub rows: CheckedOwnerRows,
    pub diagnostics: Box<[crate::OwnerDiagnosticTemplate]>,
    pub receipts: OwnerCheckedReceiptSet,
    fingerprint_v1: [u8; 32],
}

impl CheckedOwnerShard {
    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.basis.owner
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
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
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    body: &OwnerBodyInferenceShard,
    abi: &OwnerAbiEnvironment,
    own_scc: &OwnerInterfaceSccResult,
) -> Result<(), CheckedOwnerBuildError> {
    let owner = &syntax.owner;
    if &seed.owner != owner
        || &summary.owner != owner
        || body.owner() != owner
        || !own_scc.key.members.contains(owner)
    {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner inputs disagree on stable owner {owner:?}"
        )));
    }
    if seed.fingerprint_v1() != body.basis.seed_fingerprint_v1
        || summary.fingerprint_v1() != body.basis.summary_fingerprint_v1
        || syntax.fingerprint_v1() != body.basis.syntax_fingerprint_v1
        || abi.fingerprint_v1() != body.basis.authoritative_abi_fingerprint_v1
        || own_scc.fingerprint_v1() != body.basis.own_scc.result_fingerprint_v1
    {
        return Err(CheckedOwnerBuildError::new(format!(
            "checked owner inputs for {owner:?} do not match the frozen body basis"
        )));
    }
    OwnerSyntaxGraph::build(syntax).map_err(|error| {
        CheckedOwnerBuildError::new(format!(
            "checked owner {owner:?} has invalid syntax graph: {error}"
        ))
    })?;
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
            && !body.interface_imports.iter().any(|import| {
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
    requirement: CheckedParameterRequirement,
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
    requires_pass: bool,
    role: ProgramRole,
}

struct OwnerRowConstruction<'a> {
    syntax: &'a OwnerSyntaxInput,
    seed: &'a OwnerConstraintSeed,
    summary: &'a OwnerConstraintSummary,
    body: &'a OwnerBodyInferenceShard,
    own_interface: &'a OwnerPublicInterface,
    abi: &'a OwnerAbiEnvironment,
    graph: OwnerSyntaxGraph,
    imported_interfaces: BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    containing_scope: OwnerScopeRef,
    scope_ids: BTreeMap<OwnerScopeStableKey, OwnerScopeId>,
    scope_specs: Vec<Option<ScopeSpec>>,
    declaration_ids: BTreeMap<OwnerDeclarationStableKey, OwnerDeclarationId>,
    declaration_specs: Vec<Option<DeclarationSpec>>,
    statement_declarations: BTreeMap<OwnerStatementId, OwnerDeclarationId>,
    parameter_declarations: BTreeMap<u32, OwnerDeclarationId>,
    statement_scopes: Vec<OwnerScopeRef>,
    statement_body_scopes: BTreeMap<OwnerStatementId, OwnerScopeId>,
    expression_scopes: Vec<OwnerScopeRef>,
    expression_declarations: Vec<Option<OwnerDeclarationId>>,
    call_ids: BTreeMap<StableExpressionKey, OwnerCallId>,
    call_rows: Vec<OwnerCallRow>,
    call_context_rows: Vec<OwnerCallContextRow>,
}

impl<'a> OwnerRowConstruction<'a> {
    fn new(
        syntax: &'a OwnerSyntaxInput,
        seed: &'a OwnerConstraintSeed,
        summary: &'a OwnerConstraintSummary,
        body: &'a OwnerBodyInferenceShard,
        own_interface: &'a OwnerPublicInterface,
        abi: &'a OwnerAbiEnvironment,
        imported_interfaces: BTreeMap<StableCheckOwnerKey, &'a OwnerPublicInterface>,
    ) -> Result<Self, CheckedOwnerBuildError> {
        let graph = OwnerSyntaxGraph::build(syntax)
            .map_err(|error| CheckedOwnerBuildError::new(error.to_string()))?;
        let containing_scope = match &syntax.containing_scope {
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
        };
        let mut construction = Self {
            syntax,
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
            statement_scopes: vec![containing_scope; syntax.statements.len()],
            statement_body_scopes: BTreeMap::new(),
            expression_scopes: vec![OwnerScopeRef::ProjectRoot; syntax.expressions.len()],
            expression_declarations: vec![None; syntax.expressions.len()],
            call_ids: BTreeMap::new(),
            call_rows: Vec::new(),
            call_context_rows: Vec::new(),
        };
        construction.reserve_authored_declarations()?;
        construction.reserve_lexical_scopes()?;
        construction.define_authored_declarations()?;
        construction.assign_expression_ownership()?;
        construction.prepare_calls()?;
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
            if declaration_name(&statement_input.kind).is_none() {
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
                .is_some_and(|expression| matches!(expression.kind, AstExprKind::Object(_)))
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
            let flow_type = if declaration.public {
                public_declaration_flow_type(self.own_interface)
            } else {
                value
                    .as_ref()
                    .and_then(|value| self.expression_flow_type(value))
                    .unwrap_or_else(unknown_flow_type)
            };
            self.define_declaration(
                id,
                DeclarationSpec {
                    stable_key,
                    scope: self.statement_scopes[statement_id.0 as usize].clone(),
                    name: declaration_name(&statement.kind)
                        .expect("reserved declarations have a lexical name")
                        .to_owned(),
                    kind: checked_declaration_kind(declaration.kind),
                    flow_type,
                    value,
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

    fn assign_expression_ownership(&mut self) -> Result<(), CheckedOwnerBuildError> {
        let mut assigned = vec![false; self.syntax.expressions.len()];
        for statement_index in 0..self.syntax.statements.len() {
            let statement_id =
                OwnerStatementId(checked_u32(statement_index, "owner statement id")?);
            let statement = &self.syntax.statements[statement_index];
            let scope = self.statement_scopes[statement_index].clone();
            let declaration = self.statement_declarations.get(&statement_id).copied();
            if let Some(expression) = statement.expression {
                self.assign_expression_tree(expression, scope, declaration, false, &mut assigned)?;
            }
            if let Some(container) = statement_body_container(self.syntax, statement)
                && let Some(body_scope) = self.statement_body_scopes.get(&statement_id).copied()
            {
                self.assign_expression_tree(
                    checked_u32(
                        self.syntax
                            .expressions
                            .iter()
                            .position(|candidate| candidate.stable_key == container.stable_key)
                            .ok_or_else(|| {
                                CheckedOwnerBuildError::new("statement body container is missing")
                            })?,
                        "owner body container expression",
                    )?,
                    local_scope_ref(body_scope),
                    declaration,
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
        Ok(())
    }

    fn assign_expression_tree(
        &mut self,
        expression: u32,
        scope: OwnerScopeRef,
        declaration: Option<OwnerDeclarationId>,
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
        assigned[index] = true;
        self.expression_scopes[index] = scope.clone();
        if declaration.is_some() || override_existing {
            self.expression_declarations[index] = declaration;
        }
        let inputs = self
            .graph
            .expression_inputs(OwnerExpressionId(expression))
            .ok_or_else(|| CheckedOwnerBuildError::new("owner expression graph is missing"))?
            .to_vec();
        for input in inputs {
            if let OwnerExpressionRef::Local { expression } = input {
                self.assign_expression_tree(
                    expression.0,
                    scope.clone(),
                    declaration,
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
                            requirement: parameter.requirement.clone(),
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
                    requires_pass: interface.context.is_some(),
                    role: self.abi.role,
                }))
            }
            InferredOwnerCallableTarget::Authoritative => {
                let contract = self.abi.callable(&call.function).ok_or_else(|| {
                    CheckedOwnerBuildError::new(format!(
                        "authoritative owner call `{}` has no ABI contract",
                        call.function
                    ))
                })?;
                let key = abi_callable_key(contract)?;
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
                        requirement: parameter.requirement.clone(),
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

    fn prepare_calls(&mut self) -> Result<(), CheckedOwnerBuildError> {
        for call in self.body.calls.iter().cloned() {
            let Some(target) = self.prepare_call_target(&call)? else {
                continue;
            };
            let expression = self
                .syntax
                .expressions
                .iter()
                .position(|expression| expression.stable_key == call.expression)
                .ok_or_else(|| {
                    CheckedOwnerBuildError::new(format!(
                        "owner call `{}` has no local expression",
                        call.function
                    ))
                })?;
            let expression_id =
                OwnerExpressionId(checked_u32(expression, "owner call expression")?);
            let call_id = OwnerCallId(checked_u32(self.call_rows.len(), "owner call id")?);
            if self
                .call_ids
                .insert(call.expression.clone(), call_id)
                .is_some()
            {
                return Err(CheckedOwnerBuildError::new(
                    "owner body contains duplicate call expression identity",
                ));
            }

            let mut entries = Vec::new();
            let mut values_by_formal = BTreeMap::<u32, OwnerExpressionRef>::new();
            let mut output_bindings = BTreeMap::<u32, (OwnerDeclarationId, OwnerScopeId)>::new();
            for parameter in &target.parameters {
                let Some((input, from_pipe, argument_kind)) =
                    call_input_for_parameter(&call, parameter, &target.parameters)
                else {
                    if parameter.requirement.is_optional() {
                        continue;
                    }
                    return Err(CheckedOwnerBuildError::new(format!(
                        "owner call `{}` is missing required input `{}` after body inference",
                        call.function, parameter.name
                    )));
                };
                let value = owner_inference_expression_ref(input);
                match (parameter.kind, argument_kind) {
                    (CheckedParameterKind::Value, OwnerArgumentKind::Named)
                    | (CheckedParameterKind::Value, OwnerArgumentKind::BareBinding)
                        if from_pipe =>
                    {
                        values_by_formal.insert(parameter.ordinal, value.clone());
                        entries.push(OwnerCallEntry::Input {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            value,
                            from_pipe,
                            evaluation_scope: parameter
                                .output_evaluation_ordinal
                                .and_then(|ordinal| {
                                    target
                                        .parameters
                                        .iter()
                                        .find(|candidate| candidate.ordinal == ordinal)
                                })
                                .map_or(OwnerEvaluationScope::Parent, |formal| {
                                    OwnerEvaluationScope::Output {
                                        formal: formal.formal.clone(),
                                    }
                                }),
                        });
                    }
                    (CheckedParameterKind::Value, OwnerArgumentKind::Named) => {
                        values_by_formal.insert(parameter.ordinal, value.clone());
                        entries.push(OwnerCallEntry::Input {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            value,
                            from_pipe,
                            evaluation_scope: parameter
                                .output_evaluation_ordinal
                                .and_then(|ordinal| {
                                    target
                                        .parameters
                                        .iter()
                                        .find(|candidate| candidate.ordinal == ordinal)
                                })
                                .map_or(OwnerEvaluationScope::Parent, |formal| {
                                    OwnerEvaluationScope::Output {
                                        formal: formal.formal.clone(),
                                    }
                                }),
                        });
                    }
                    (CheckedParameterKind::Value, OwnerArgumentKind::BareBinding) => {
                        return Err(CheckedOwnerBuildError::new(format!(
                            "ordinary input `{}` is bound as a bare OUT",
                            parameter.name
                        )));
                    }
                    (CheckedParameterKind::Out, OwnerArgumentKind::BareBinding) => {
                        let declaration_key = OwnerDeclarationStableKey::FreshOut {
                            call: call.expression.clone(),
                            formal_ordinal: parameter.ordinal,
                        };
                        let declaration = self.reserve_declaration(declaration_key.clone())?;
                        let scope_key = OwnerScopeStableKey::GeneratedOut {
                            call: call.expression.clone(),
                            formal_ordinal: parameter.ordinal,
                        };
                        let scope = self.reserve_scope(scope_key.clone())?;
                        let parent = self.expression_scopes[expression].clone();
                        self.define_scope(
                            scope,
                            ScopeSpec {
                                stable_key: scope_key,
                                parent: Some(parent.clone()),
                                owner: Some(local_declaration_ref(declaration)),
                                kind: CheckedScopeKind::RepeatedOutput,
                                source: Some(expression_source(
                                    &self.syntax.expressions[expression],
                                )),
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
                                source: expression_source(&self.syntax.expressions[expression]),
                            },
                        )?;
                        output_bindings.insert(parameter.ordinal, (declaration, scope));
                        entries.push(OwnerCallEntry::FreshOut {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            output: declaration,
                            scope_id: scope,
                        });
                        if let OwnerExpressionRef::Local {
                            expression: output_expression,
                        } = value
                        {
                            self.rebase_expression_tree(output_expression, local_scope_ref(scope))?;
                        }
                    }
                    (CheckedParameterKind::Out, OwnerArgumentKind::Named) => {
                        let target_name = self.single_expression_name(&value).ok_or_else(|| {
                            CheckedOwnerBuildError::new(format!(
                                "forwarded OUT `{}` does not name one binding",
                                parameter.name
                            ))
                        })?;
                        let target_declaration = self
                            .own_interface
                            .parameters
                            .iter()
                            .find(|candidate| {
                                candidate.kind == OwnerParameterKind::Out
                                    && candidate.name == target_name
                            })
                            .and_then(|candidate| {
                                self.parameter_declarations.get(&candidate.ordinal).copied()
                            })
                            .ok_or_else(|| {
                                CheckedOwnerBuildError::new(format!(
                                    "forwarded OUT `{target_name}` has no enclosing formal"
                                ))
                            })?;
                        entries.push(OwnerCallEntry::ForwardOut {
                            formal: parameter.formal.clone(),
                            name: parameter.name.clone(),
                            target: local_declaration_ref(target_declaration),
                            target_name,
                        });
                    }
                }
            }

            for entry in &entries {
                let OwnerCallEntry::Input {
                    value,
                    evaluation_scope: OwnerEvaluationScope::Output { formal },
                    ..
                } = entry
                else {
                    continue;
                };
                let output_ordinal = target
                    .parameters
                    .iter()
                    .find(|parameter| &parameter.formal == formal)
                    .map(|parameter| parameter.ordinal);
                let scope = output_ordinal
                    .and_then(|ordinal| output_bindings.get(&ordinal).map(|(_, scope)| *scope))
                    .or_else(|| {
                        let OwnerDeclarationRef::Local { declaration } = formal else {
                            return None;
                        };
                        self.declaration_specs
                            .get(declaration.0 as usize)
                            .and_then(Option::as_ref)
                            .and_then(|declaration| declaration.body_scope)
                    });
                if let Some(scope) = scope
                    && let OwnerExpressionRef::Local { expression } = value
                {
                    self.rebase_expression_tree(*expression, local_scope_ref(scope))?;
                }
            }

            let mut contexts = Vec::new();
            for (context_ordinal, context) in target.contexts.iter().enumerate() {
                if !values_by_formal.contains_key(&context.provider_ordinal) {
                    continue;
                }
                let ordinal = checked_u32(context_ordinal, "owner call context ordinal")?;
                let declaration_key = OwnerDeclarationStableKey::CallContext {
                    call: call.expression.clone(),
                    ordinal,
                };
                let declaration = self.reserve_declaration(declaration_key.clone())?;
                let scope_key = OwnerScopeStableKey::CallContext {
                    call: call.expression.clone(),
                    ordinal,
                };
                let scope = self.reserve_scope(scope_key.clone())?;
                self.define_scope(
                    scope,
                    ScopeSpec {
                        stable_key: scope_key,
                        parent: Some(self.expression_scopes[expression].clone()),
                        owner: Some(local_declaration_ref(declaration)),
                        kind: CheckedScopeKind::CallContext,
                        source: Some(expression_source(&self.syntax.expressions[expression])),
                    },
                )?;
                self.define_declaration(
                    declaration,
                    DeclarationSpec {
                        stable_key: declaration_key,
                        scope: local_scope_ref(scope),
                        name: context.name.clone(),
                        kind: match context.kind {
                            CheckedCallContextKind::ElementState => {
                                CheckedDeclarationKind::ElementState
                            }
                        },
                        flow_type: context.flow_type.clone(),
                        value: None,
                        body_scope: None,
                        source: expression_source(&self.syntax.expressions[expression]),
                    },
                )?;
                contexts.push(OwnerCallContextRow {
                    declaration,
                    context_ordinal: ordinal,
                    scope_id: scope,
                });
            }

            let explicit_pass = call.inputs.iter().find_map(|input| {
                matches!(
                    input.role,
                    OwnerConstraintEdgeRole::CallPass { .. }
                        | OwnerConstraintEdgeRole::PipePass { .. }
                )
                .then(|| owner_inference_expression_ref(&input.expression))
            });
            let context_binding = explicit_pass.map_or_else(
                || {
                    if target.requires_pass && self.own_interface.context.is_some() {
                        OwnerContextBinding::Inherited {
                            formal: OwnerContextFormalRef::Local {
                                formal: OwnerContextFormalId(0),
                            },
                        }
                    } else {
                        OwnerContextBinding::None
                    }
                },
                |value| OwnerContextBinding::Explicit {
                    value,
                    source: OwnerSourceSite::CallPass {
                        expression: call.expression.clone(),
                    },
                },
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
            self.call_context_rows.extend(contexts.iter().cloned());
            self.call_rows.push(OwnerCallRow {
                id: call_id,
                stable_key: call.expression.clone(),
                expression: expression_id,
                callable: target.callable,
                owner_callable,
                function: call.function,
                intrinsic: target.intrinsic,
                entries,
                contexts,
                context_binding,
                contextual_substitutions: Vec::new(),
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
                source: expression_source(&self.syntax.expressions[expression]),
            });
        }
        Ok(())
    }

    fn rebase_expression_tree(
        &mut self,
        expression: OwnerExpressionId,
        scope: OwnerScopeRef,
    ) -> Result<(), CheckedOwnerBuildError> {
        let mut assigned = vec![true; self.syntax.expressions.len()];
        let declaration = self.expression_declarations[expression.0 as usize];
        self.assign_expression_tree(expression.0, scope, declaration, true, &mut assigned)
    }

    fn single_expression_name(&self, expression: &OwnerExpressionRef) -> Option<String> {
        let OwnerExpressionRef::Local { expression } = expression else {
            return None;
        };
        match &self.syntax.expressions.get(expression.0 as usize)?.kind {
            AstExprKind::Identifier(name) => Some(name.clone()),
            AstExprKind::Path(parts) if parts.len() == 1 => parts.first().cloned(),
            _ => None,
        }
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
                        .map(|declaration| self.local_declaration_key(declaration))
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
                "source_seed": source_seed,
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
                        "declaration": self.local_declaration_key(binding.declaration)?,
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
                boon_checked::OwnerStatementChild::Owner { owner } => Ok(json!({
                    "kind": "owner",
                    "owner": owner,
                })),
            })
            .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()?;
        Ok(json!({
            "scope": self.normalize_scope_ref(&row.scope, relocations)?,
            "kind": self.normalize_statement_kind(&row.kind)?,
            "resources": row.resources,
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
        relocations: &mut Vec<OwnerRelocationTarget>,
    ) -> Result<Value, CheckedOwnerBuildError> {
        Ok(json!({
            "scope": self.normalize_scope_ref(&row.scope, relocations)?,
            "declaration": row
                .declaration
                .map(|declaration| self.local_declaration_key(declaration))
                .transpose()?,
            "flow_type": row.flow_type,
            "flush_type": row.flush_type,
            "effect": row.effect,
            "kind": self.normalize_expression_kind(&row.kind, relocations)?,
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
            "contextual_substitutions": row.contextual_substitutions,
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
    ) -> Result<(CheckedOwnerRows, OwnerCheckedReceiptSet), CheckedOwnerBuildError> {
        let statements = self.build_statement_rows()?;
        let expressions = self.build_expression_rows()?;
        let callables = self.build_callable_rows()?;
        let context_formals = self.build_context_formal_rows()?;
        let calls = self.call_rows.clone();
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
            let payload = self.normalized_statement_payload(row, &mut relocations)?;
            sink.record(
                OwnerCheckedRowDomain::Statement,
                &row.stable_key,
                &payload,
                relocations,
            )?;
        }
        for row in &rows.expressions {
            let mut relocations = Vec::new();
            let payload = self.normalized_expression_payload(row, &mut relocations)?;
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
        for diagnostic in &self.body.diagnostics {
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
        rows.relocations = receipts.relocations.to_vec();
        rows.receipts = receipts.row_receipts.to_vec();
        Ok((rows, receipts))
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
            role: self.abi.role,
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
                    value_use: CheckedValueUse::RuntimeValue,
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
                Ok(OwnerExpressionRow {
                    id,
                    stable_key: expression.stable_key.clone(),
                    scope: self.expression_scopes[index].clone(),
                    declaration: self.expression_declarations[index],
                    flow_type: inferred.flow_type.clone(),
                    flush_type: None,
                    effect: inferred.direct_effect,
                    kind: self.lower_expression_kind(id, &expression.kind)?,
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
        let expression = &self.syntax.expressions[id.0 as usize];
        let expression_ref = |reference| owner_expression_ref(self.syntax, reference);
        let fields = |fields: &[boon_syntax::AstRecordField]| {
            fields
                .iter()
                .enumerate()
                .map(|(ordinal, field)| {
                    Ok(OwnerRecordField {
                        declaration: (field.value < self.syntax.expressions.len())
                            .then(|| self.expression_declarations[field.value])
                            .flatten(),
                        name: field.name.clone(),
                        value: expression_ref(field.value)?,
                        spread: field.spread,
                        source: OwnerSourceSite::CallArgument {
                            expression: expression.stable_key.clone(),
                            ordinal: checked_u32(ordinal, "record field ordinal")?,
                        },
                    })
                })
                .collect::<Result<Vec<_>, CheckedOwnerBuildError>>()
        };
        Ok(match kind {
            AstExprKind::Identifier(name) => self.lower_read(expression, &[name.clone()], false),
            AstExprKind::Path(parts) => self.lower_read(expression, parts, false),
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
                self.lower_read(expression, &parts, true)
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
                bindings: Vec::new(),
                output: output.map(expression_ref).transpose()?,
            },
            AstExprKind::Block { bindings, result } => OwnerExpressionKind::Block {
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        let statement = OwnerStatementId(checked_u32(
                            binding.statement,
                            "block binding statement",
                        )?);
                        Ok(OwnerBlockBinding {
                            declaration: *self.statement_declarations.get(&statement).ok_or_else(
                                || {
                                    CheckedOwnerBuildError::new(
                                        "block binding declaration is missing",
                                    )
                                },
                            )?,
                            value: expression_ref(binding.value)?,
                            source: OwnerSourceSite::Statement {
                                statement: self.syntax.statements[statement.0 as usize]
                                    .stable_key
                                    .clone(),
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
        expression: &crate::OwnerExpressionInput,
        parts: &[String],
        drain: bool,
    ) -> OwnerExpressionKind {
        if let Some(fields) = parts.strip_prefix(&["PASSED".to_owned()]) {
            return OwnerExpressionKind::Invalid {
                tokens: std::iter::once(if drain {
                    "unbound_passed_drain"
                } else {
                    "unbound_passed_context"
                })
                .chain(fields.iter().map(String::as_str))
                .map(str::to_owned)
                .collect(),
            };
        }
        if let Some(resolved) = self
            .summary
            .resolved_references
            .iter()
            .find(|resolved| resolved.reference.expression == expression.stable_key)
        {
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
                return if drain {
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
                };
            }
        }
        if let Some((root, projection)) = parts.split_first() {
            if let Some((_, declaration)) =
                self.parameter_declarations.iter().find(|(ordinal, _)| {
                    self.own_interface.parameters[**ordinal as usize].name == *root
                })
            {
                let target = local_declaration_ref(*declaration);
                return if drain {
                    OwnerExpressionKind::Drain {
                        target,
                        projection: projection.to_vec(),
                    }
                } else {
                    OwnerExpressionKind::Read {
                        target,
                        projection: projection.to_vec(),
                        source_seed: None,
                    }
                };
            }
        }
        OwnerExpressionKind::ExternalRead {
            canonical_path: parts.join("/"),
            declaration: None,
        }
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

fn owner_inference_expression_ref(reference: &OwnerInferenceExpressionRef) -> OwnerExpressionRef {
    match reference {
        OwnerInferenceExpressionRef::Local { expression } => OwnerExpressionRef::Local {
            expression: OwnerExpressionId(expression.0),
        },
        OwnerInferenceExpressionRef::External { owner, expression } => OwnerExpressionRef::Child {
            owner: owner.clone(),
            expression: expression.clone(),
        },
    }
}

fn call_input_for_parameter<'a>(
    call: &'a InferredOwnerCall,
    parameter: &PreparedCallParameter,
    parameters: &[PreparedCallParameter],
) -> Option<(&'a OwnerInferenceExpressionRef, bool, OwnerArgumentKind)> {
    call.inputs.iter().find_map(|input| {
        let matched = match &input.role {
            OwnerConstraintEdgeRole::CallArgument { kind, name } => {
                (name == &parameter.name).then_some((false, *kind))
            }
            OwnerConstraintEdgeRole::PipeArgument { kind, name } => {
                (name == &parameter.name).then_some((false, *kind))
            }
            OwnerConstraintEdgeRole::PipeInput
                if parameter.kind == CheckedParameterKind::Value
                    && parameters
                        .iter()
                        .filter(|candidate| candidate.kind == CheckedParameterKind::Value)
                        .min_by_key(|candidate| candidate.ordinal)
                        .is_some_and(|candidate| candidate.ordinal == parameter.ordinal) =>
            {
                Some((true, OwnerArgumentKind::Named))
            }
            _ => None,
        }?;
        Some((&input.expression, matched.0, matched.1))
    })
}

fn abi_callable_key(
    contract: &OwnerAbiCallableContract,
) -> Result<OwnerAbiDeclarationKey, CheckedOwnerBuildError> {
    let contract_fingerprint_v1 =
        boon_contract::canonical_serde_hash_v1(b"boon.owner-checked-abi-callable.v1\0", contract)
            .map_err(|error| {
            CheckedOwnerBuildError::new(format!("cannot fingerprint owner ABI callable: {error}"))
        })?;
    Ok(OwnerAbiDeclarationKey {
        role: contract.role,
        kind: match contract.kind {
            CheckedCallableKind::Builtin => OwnerAbiDeclarationKind::BuiltinCallable,
            CheckedCallableKind::External => OwnerAbiDeclarationKind::ExternalCallable,
            CheckedCallableKind::User => {
                return Err(CheckedOwnerBuildError::new(
                    "authoritative ABI cannot contain a user callable",
                ));
            }
        },
        contract_fingerprint_v1,
        external_identity: contract.external_identity,
    })
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
) -> Option<&'a crate::OwnerExpressionInput> {
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
    let expression = syntax.expressions.get(statement.expression? as usize)?;
    if is_container(expression) {
        return Some(expression);
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
    seed: &OwnerConstraintSeed,
    summary: &OwnerConstraintSummary,
    body: &OwnerBodyInferenceShard,
    abi: &OwnerAbiEnvironment,
    own_scc: &OwnerInterfaceSccResult,
    imported_sccs: impl IntoIterator<Item = &'a OwnerInterfaceSccResult>,
) -> Result<CheckedOwnerShard, CheckedOwnerBuildError> {
    validate_inputs(syntax, seed, summary, body, abi, own_scc)?;

    let mut interfaces = own_scc
        .owners
        .iter()
        .chain(
            imported_sccs
                .into_iter()
                .flat_map(|result| result.owners.iter()),
        )
        .map(|interface| (interface.owner.clone(), interface))
        .collect::<std::collections::BTreeMap<_, &OwnerPublicInterface>>();
    let own_interface = interfaces.remove(&syntax.owner).ok_or_else(|| {
        CheckedOwnerBuildError::new(format!(
            "checked owner {:?} has no frozen public interface",
            syntax.owner
        ))
    })?;
    let basis = CheckedOwnerShardBasis {
        owner: syntax.owner.clone(),
        syntax_fingerprint_v1: syntax.fingerprint_v1(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        summary_fingerprint_v1: summary.fingerprint_v1(),
        body_fingerprint_v1: body.fingerprint_v1(),
        own_interface_scc_fingerprint_v1: own_scc.fingerprint_v1(),
        authoritative_abi_fingerprint_v1: abi.fingerprint_v1(),
    };

    let (rows, receipts) =
        OwnerRowConstruction::new(syntax, seed, summary, body, own_interface, abi, interfaces)?
            .build_base_rows()?;

    // The row builder lands immediately after this typed validation/basis
    // boundary.  Keep this fail-closed while it is incomplete: returning an
    // empty shard would let a session accidentally publish a semantically
    // partial checked owner.
    let _ = (basis, rows, receipts, CHECKED_OWNER_SHARD_DOMAIN_V1);
    Err(CheckedOwnerBuildError::new(
        "checked owner row construction is not complete",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        build_owner_interface_topology, infer_owner_body, project_owner_abi_environment,
        project_owner_constraint_seed, project_owner_syntax_input, resolve_owner_constraint_seed,
        solve_owner_interface_scc,
    };
    use boon_checked::{ExternalTypeEnvironment, OwnerAbiMemberRef};
    use boon_parser::{ProjectSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys};
    use std::sync::Arc;

    struct Fixture {
        syntax: OwnerSyntaxInput,
        seed: OwnerConstraintSeed,
        summary: OwnerConstraintSummary,
        abi: OwnerAbiEnvironment,
        interface: OwnerInterfaceSccResult,
        body: OwnerBodyInferenceShard,
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
        let owner = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == [name])
                )
            })
            .unwrap();
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let seed = project_owner_constraint_seed(&syntax).unwrap();
        let summary = resolve_owner_constraint_seed(&seed, []).unwrap();
        let abi = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let callable_abi = abi.callable_environment().unwrap();
        let topology = build_owner_interface_topology([&summary]).unwrap();
        let interface = solve_owner_interface_scc(
            topology.sccs.first().unwrap(),
            &callable_abi,
            [&seed],
            [&summary],
            [],
        )
        .unwrap();
        let body =
            infer_owner_body(&syntax, &seed, &summary, &callable_abi, &interface, []).unwrap();
        Fixture {
            syntax,
            seed,
            summary,
            abi,
            interface,
            body,
        }
    }

    fn built_rows(fixture: &Fixture) -> (CheckedOwnerRows, OwnerCheckedReceiptSet) {
        let own = fixture.interface.owner(&fixture.syntax.owner).unwrap();
        OwnerRowConstruction::new(
            &fixture.syntax,
            &fixture.seed,
            &fixture.summary,
            &fixture.body,
            own,
            &fixture.abi,
            BTreeMap::new(),
        )
        .unwrap()
        .build_base_rows()
        .unwrap()
    }

    fn rows(fixture: &Fixture) -> CheckedOwnerRows {
        built_rows(fixture).0
    }

    #[test]
    fn base_rows_preserve_stable_object_structure() {
        let fixture = fixture("record: [value: 1]\n", "record");
        let rows = rows(&fixture);
        assert_eq!(rows.declarations.len(), 1);
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
            + fixture.body.diagnostics.len();
        assert_eq!(receipts.row_receipts.len(), expected);
        assert_eq!(rows.receipts.as_slice(), receipts.row_receipts.as_ref());
        assert_eq!(rows.relocations.as_slice(), receipts.relocations.as_ref());
        assert_eq!(
            receipts.construction.row_receipt_count as usize,
            receipts.row_receipts.len()
        );
    }
}
