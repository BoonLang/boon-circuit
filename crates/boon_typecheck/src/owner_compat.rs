//! Non-checking dense compatibility assembly for immutable checked owners.
//!
//! This module is deliberately downstream of owner inference and checked-row
//! construction. It assigns revision-local dense IDs, resolves stable
//! relocations, and reattaches current source positions; it never opens the
//! monolithic [`crate::CheckedProgramDatabase`] or performs type inference.

use crate::{
    CheckedOwnerShard, OwnerAbiCallableContract, OwnerAbiContextualOperation,
    OwnerAbiEnvironmentError, OwnerAbiEvaluationScope, OwnerAbiValueContract,
    OwnerConstructionAbiEnvironment, OwnerConstructionCallableAbiLookupOutcome,
    OwnerConstructionValueAbiLookupOutcome, OwnerSourceAnchorRole, OwnerSourceAnchorSite,
    OwnerSourceMap, ProjectDiagnosticFacts, owner_abi_value_declaration_key,
};
use boon_checked::*;
use boon_parser::ProjectSyntaxSnapshot;
use boon_syntax::{AstStatement, StableCheckOwnerKey, StableExpressionKey, StableStatementKey};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::time::Instant;

fn trace_compat_phase(enabled: bool, phase: &str, started: &mut Instant, items: usize) {
    if enabled {
        eprintln!(
            "boon owner compatibility phase={phase} items={items} phase_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    *started = Instant::now();
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCompatibilityAssemblyError {
    message: String,
}

impl OwnerCompatibilityAssemblyError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerCompatibilityAssemblyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerCompatibilityAssemblyError {}

impl From<OwnerAbiEnvironmentError> for OwnerCompatibilityAssemblyError {
    fn from(error: OwnerAbiEnvironmentError) -> Self {
        Self::new(error.to_string())
    }
}

/// Dense legacy DTO plus source-positioned diagnostics assembled from checked
/// owner results. Runtime sealing is a distinct request so diagnostics can
/// retain these exact fields without constructing a second checked graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedOwnerProjectAssembly {
    fields: CheckedProgramFields,
    diagnostics: Vec<TypeDiagnostic>,
    fingerprint_v1: [u8; 32],
}

impl CheckedOwnerProjectAssembly {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub const fn fields(&self) -> &CheckedProgramFields {
        &self.fields
    }

    pub fn diagnostics(&self) -> &[TypeDiagnostic] {
        &self.diagnostics
    }
}

#[derive(Clone)]
struct AbiCallableEntry {
    name: String,
    key: OwnerAbiDeclarationKey,
    contract: OwnerAbiCallableContract,
}

#[derive(Clone)]
struct AbiValueEntry {
    path: String,
    key: OwnerAbiDeclarationKey,
    contract: OwnerAbiValueContract,
}

#[derive(Default)]
struct OwnerDenseLayout {
    scopes: Vec<LexicalScopeId>,
    declarations: Vec<DeclId>,
    statements: Vec<CheckedStatementId>,
    expressions: Vec<CheckedExprId>,
    calls: Vec<CheckedCallId>,
    context_formals: Vec<ContextFormalId>,
    sources: Vec<CheckedSourceId>,
    states: Vec<CheckedStateId>,
    lists: Vec<CheckedListId>,
}

struct CompatibilityLayout<'a> {
    project: &'a ProjectSyntaxSnapshot,
    shards: BTreeMap<StableCheckOwnerKey, &'a CheckedOwnerShard>,
    source_maps: BTreeMap<StableCheckOwnerKey, &'a OwnerSourceMap>,
    owners: BTreeMap<StableCheckOwnerKey, OwnerDenseLayout>,
    statement_by_key: BTreeMap<StableStatementKey, CheckedStatementId>,
    expression_by_key: BTreeMap<StableExpressionKey, CheckedExprId>,
    call_by_key: BTreeMap<(StableCheckOwnerKey, StableExpressionKey), CheckedCallId>,
    owner_root_statement: BTreeMap<StableCheckOwnerKey, CheckedStatementId>,
    owner_public_declaration: BTreeMap<StableCheckOwnerKey, DeclId>,
    owner_parameter_declaration: BTreeMap<(StableCheckOwnerKey, u32), DeclId>,
    declaration_by_key: BTreeMap<(StableCheckOwnerKey, OwnerDeclarationStableKey), DeclId>,
    owner_context_formal: BTreeMap<StableCheckOwnerKey, ContextFormalId>,
    scope_by_key: BTreeMap<(StableCheckOwnerKey, OwnerScopeStableKey), LexicalScopeId>,
    source_by_key: BTreeMap<OwnerSourceStableKey, CheckedSourceId>,
    abi_declarations: BTreeMap<(String, OwnerAbiDeclarationKey, OwnerAbiMemberRef), DeclId>,
    abi_callables: Vec<AbiCallableEntry>,
    abi_values: Vec<AbiValueEntry>,
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerCompatibilityAssemblyError> {
    u32::try_from(value)
        .map_err(|_| OwnerCompatibilityAssemblyError::new(format!("{context} exceeds u32")))
}

fn checked_usize(value: u64, context: &str) -> Result<usize, OwnerCompatibilityAssemblyError> {
    usize::try_from(value)
        .map_err(|_| OwnerCompatibilityAssemblyError::new(format!("{context} exceeds usize")))
}

fn allocate_decl(next: &mut u32) -> Result<DeclId, OwnerCompatibilityAssemblyError> {
    let id = DeclId(*next);
    *next = next.checked_add(1).ok_or_else(|| {
        OwnerCompatibilityAssemblyError::new("dense declaration identity overflow")
    })?;
    Ok(id)
}

impl<'a> CompatibilityLayout<'a> {
    fn new(
        project: &'a ProjectSyntaxSnapshot,
        role: ProgramRole,
        shard_inputs: impl IntoIterator<Item = &'a CheckedOwnerShard>,
        source_map_inputs: impl IntoIterator<Item = &'a OwnerSourceMap>,
        construction_abis: impl IntoIterator<Item = &'a OwnerConstructionAbiEnvironment>,
    ) -> Result<Self, OwnerCompatibilityAssemblyError> {
        let mut shards = BTreeMap::new();
        for shard in shard_inputs {
            if shards.insert(shard.owner().clone(), shard).is_some() {
                return Err(OwnerCompatibilityAssemblyError::new(format!(
                    "checked owner assembly has duplicate shard for {:?}",
                    shard.owner()
                )));
            }
        }
        let mut source_maps = BTreeMap::new();
        for map in source_map_inputs {
            if source_maps.insert(map.owner().clone(), map).is_some() {
                return Err(OwnerCompatibilityAssemblyError::new(format!(
                    "checked owner assembly has duplicate source map for {:?}",
                    map.owner()
                )));
            }
        }
        let mut construction_abis_by_owner = BTreeMap::new();
        for abi in construction_abis {
            if abi.role() != role {
                return Err(OwnerCompatibilityAssemblyError::new(format!(
                    "checked owner assembly construction ABI for {:?} has the wrong program role",
                    abi.owner()
                )));
            }
            if construction_abis_by_owner
                .insert(abi.owner().clone(), abi)
                .is_some()
            {
                return Err(OwnerCompatibilityAssemblyError::new(format!(
                    "checked owner assembly has duplicate construction ABI for {:?}",
                    abi.owner()
                )));
            }
        }
        let project_owners = project.stable_check_owner_keys().collect::<BTreeSet<_>>();
        if shards.keys().cloned().collect::<BTreeSet<_>>() != project_owners
            || source_maps.keys().cloned().collect::<BTreeSet<_>>() != project_owners
            || construction_abis_by_owner
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != project_owners
        {
            return Err(OwnerCompatibilityAssemblyError::new(
                "checked owner assembly inputs do not exactly cover the project owner set",
            ));
        }
        for (owner, shard) in &shards {
            shard
                .validate_seal(construction_abis_by_owner[owner])
                .map_err(|error| OwnerCompatibilityAssemblyError::new(error.to_string()))?;
        }

        let mut abi_callables =
            BTreeMap::<(String, OwnerAbiDeclarationKey), AbiCallableEntry>::new();
        let mut abi_values = BTreeMap::<(String, OwnerAbiDeclarationKey), AbiValueEntry>::new();
        for abi in construction_abis_by_owner.into_values() {
            for lookup in abi.callable_lookups() {
                let OwnerConstructionCallableAbiLookupOutcome::Found { contract } =
                    lookup.outcome()
                else {
                    continue;
                };
                let key = lookup.declaration_key().ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(
                        "found construction callable ABI has no declaration key",
                    )
                })?;
                let entry = AbiCallableEntry {
                    name: lookup.canonical_name().to_owned(),
                    key,
                    contract: contract.clone(),
                };
                match abi_callables.entry((entry.name.clone(), key)) {
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        slot.insert(entry);
                    }
                    std::collections::btree_map::Entry::Occupied(slot)
                        if slot.get().contract == entry.contract => {}
                    std::collections::btree_map::Entry::Occupied(_) => {
                        return Err(OwnerCompatibilityAssemblyError::new(
                            "checked owner construction ABIs disagree on a callable contract",
                        ));
                    }
                }
            }
            for lookup in abi.value_lookups() {
                let OwnerConstructionValueAbiLookupOutcome::Found { contract } = lookup.outcome()
                else {
                    continue;
                };
                let key = owner_abi_value_declaration_key(abi.role(), contract)?;
                let entry = AbiValueEntry {
                    path: lookup.canonical_path().to_owned(),
                    key,
                    contract: contract.clone(),
                };
                match abi_values.entry((entry.path.clone(), key)) {
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        slot.insert(entry);
                    }
                    std::collections::btree_map::Entry::Occupied(slot)
                        if slot.get().contract == entry.contract => {}
                    std::collections::btree_map::Entry::Occupied(_) => {
                        return Err(OwnerCompatibilityAssemblyError::new(
                            "checked owner construction ABIs disagree on a value contract",
                        ));
                    }
                }
            }
        }
        let mut statement_by_key = BTreeMap::new();
        for slot in 0..project.statement_count() {
            let syntax = project.statement_id_for_slot(slot).ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new("project statement slot is not reversible")
            })?;
            let key = project.stable_statement_key(syntax).ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new("project statement has no stable key")
            })?;
            if statement_by_key
                .insert(
                    key,
                    CheckedStatementId(checked_u32(slot, "statement slot")?),
                )
                .is_some()
            {
                return Err(OwnerCompatibilityAssemblyError::new(
                    "project has duplicate stable statement keys",
                ));
            }
        }
        let mut expression_by_key = BTreeMap::new();
        for slot in 0..project.expression_count() {
            let syntax = project.expression_id_for_slot(slot).ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new("project expression slot is not reversible")
            })?;
            let Some(key) = project.stable_expression_key(syntax) else {
                // Parser arenas retain some unreachable construction nodes.
                // They have no stable owner and therefore no checked shard;
                // the dense compatibility DTO installs an inert row later.
                continue;
            };
            if expression_by_key
                .insert(key, CheckedExprId(checked_u32(slot, "expression slot")?))
                .is_some()
            {
                return Err(OwnerCompatibilityAssemblyError::new(
                    "project has duplicate stable expression keys",
                ));
            }
        }

        let mut layout = Self {
            project,
            shards,
            source_maps,
            owners: BTreeMap::new(),
            statement_by_key,
            expression_by_key,
            call_by_key: BTreeMap::new(),
            owner_root_statement: BTreeMap::new(),
            owner_public_declaration: BTreeMap::new(),
            owner_parameter_declaration: BTreeMap::new(),
            declaration_by_key: BTreeMap::new(),
            owner_context_formal: BTreeMap::new(),
            scope_by_key: BTreeMap::new(),
            source_by_key: BTreeMap::new(),
            abi_declarations: BTreeMap::new(),
            abi_callables: abi_callables.into_values().collect(),
            abi_values: abi_values.into_values().collect(),
        };
        layout.assign_dense_ids()?;
        Ok(layout)
    }

    fn assign_dense_ids(&mut self) -> Result<(), OwnerCompatibilityAssemblyError> {
        for (owner, shard) in &self.shards {
            let statements = shard
                .rows()
                .statements
                .iter()
                .map(|row| {
                    self.statement_by_key
                        .get(&row.stable_key)
                        .copied()
                        .ok_or_else(|| {
                            OwnerCompatibilityAssemblyError::new(format!(
                                "owner {owner:?} statement has no project dense slot"
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let expressions = shard
                .rows()
                .expressions
                .iter()
                .map(|row| {
                    self.expression_by_key
                        .get(&row.stable_key)
                        .copied()
                        .ok_or_else(|| {
                            OwnerCompatibilityAssemblyError::new(format!(
                                "owner {owner:?} expression has no project dense slot"
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(root) = statements.first() {
                self.owner_root_statement.insert(owner.clone(), *root);
            }
            self.owners.insert(
                owner.clone(),
                OwnerDenseLayout {
                    statements,
                    expressions,
                    ..OwnerDenseLayout::default()
                },
            );
        }

        let mut next_declaration = 1u32;
        // User callable identities and formals precede authoritative ABI rows,
        // matching the legacy checker's stable high-level allocation shape.
        for (owner, shard) in &self.shards {
            for callable in &shard.rows().callables {
                if callable.kind != CheckedCallableKind::User {
                    continue;
                }
                let declaration = allocate_decl(&mut next_declaration)?;
                self.owners
                    .get_mut(owner)
                    .expect("owner layout exists")
                    .declarations
                    .resize(shard.rows().declarations.len(), DeclId(u32::MAX));
                self.owners.get_mut(owner).unwrap().declarations[callable.declaration.0 as usize] =
                    declaration;
                self.owner_public_declaration
                    .insert(owner.clone(), declaration);
                for parameter in &callable.parameters {
                    let dense = allocate_decl(&mut next_declaration)?;
                    self.owners.get_mut(owner).unwrap().declarations
                        [parameter.declaration.0 as usize] = dense;
                    self.owner_parameter_declaration
                        .insert((owner.clone(), parameter.ordinal), dense);
                }
            }
        }

        for entry in &self.abi_callables {
            let declaration = allocate_decl(&mut next_declaration)?;
            self.abi_declarations.insert(
                (
                    entry.name.clone(),
                    entry.key,
                    OwnerAbiMemberRef::Declaration,
                ),
                declaration,
            );
            for parameter in &entry.contract.parameters {
                let formal = allocate_decl(&mut next_declaration)?;
                self.abi_declarations.insert(
                    (
                        entry.name.clone(),
                        entry.key,
                        OwnerAbiMemberRef::Parameter {
                            ordinal: parameter.ordinal,
                        },
                    ),
                    formal,
                );
            }
        }
        for entry in &self.abi_values {
            let declaration = allocate_decl(&mut next_declaration)?;
            self.abi_declarations.insert(
                (
                    entry.path.clone(),
                    entry.key,
                    OwnerAbiMemberRef::Declaration,
                ),
                declaration,
            );
        }
        for (owner, shard) in &self.shards {
            let dense = &mut self.owners.get_mut(owner).unwrap().declarations;
            dense.resize(shard.rows().declarations.len(), DeclId(u32::MAX));
            for row in &shard.rows().declarations {
                let slot = &mut dense[row.id.0 as usize];
                if slot.0 == u32::MAX {
                    *slot = allocate_decl(&mut next_declaration)?;
                }
                if self
                    .declaration_by_key
                    .insert((owner.clone(), row.stable_key.clone()), *slot)
                    .is_some()
                {
                    return Err(OwnerCompatibilityAssemblyError::new(format!(
                        "owner {owner:?} repeats stable declaration key {:?}",
                        row.stable_key
                    )));
                }
                if row.stable_key == OwnerDeclarationStableKey::Public {
                    self.owner_public_declaration.insert(owner.clone(), *slot);
                }
                if let OwnerDeclarationStableKey::Parameter { ordinal } = row.stable_key {
                    self.owner_parameter_declaration
                        .insert((owner.clone(), ordinal), *slot);
                }
            }
        }

        let mut next_scope = 1u32;
        let mut next_formal = 0u32;
        for (owner, shard) in &self.shards {
            let dense = &mut self.owners.get_mut(owner).unwrap();
            dense.scopes = shard
                .rows()
                .scopes
                .iter()
                .map(|row| {
                    let id = LexicalScopeId(next_scope);
                    next_scope = next_scope.checked_add(1).ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new("dense scope identity overflow")
                    })?;
                    self.scope_by_key
                        .insert((owner.clone(), row.stable_key.clone()), id);
                    Ok(id)
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
            dense.context_formals = shard
                .rows()
                .context_formals
                .iter()
                .map(|_| {
                    let id = ContextFormalId(next_formal);
                    next_formal = next_formal.checked_add(1).ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(
                            "dense context-formal identity overflow",
                        )
                    })?;
                    Ok(id)
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
            if let Some(formal) = dense.context_formals.first() {
                self.owner_context_formal.insert(owner.clone(), *formal);
            }
        }

        let mut calls = Vec::new();
        for (owner, shard) in &self.shards {
            for row in &shard.rows().calls {
                let expression = self.owners[owner].expressions[row.expression.0 as usize];
                calls.push((expression, owner.clone(), row.id));
            }
        }
        calls.sort_by(|left, right| (left.0, &left.1, left.2).cmp(&(right.0, &right.1, right.2)));
        for (index, (_, owner, local)) in calls.into_iter().enumerate() {
            let row = &self.shards[&owner].rows().calls[local.0 as usize];
            let stable_key = row.stable_key.clone();
            let dense = &mut self.owners.get_mut(&owner).unwrap().calls;
            dense.resize(
                self.shards[&owner].rows().calls.len(),
                CheckedCallId(u32::MAX),
            );
            let id = CheckedCallId(checked_u32(index, "call identity")?);
            dense[local.0 as usize] = id;
            if self.call_by_key.insert((owner, stable_key), id).is_some() {
                return Err(OwnerCompatibilityAssemblyError::new(
                    "checked owner assembly repeats one stable call identity",
                ));
            }
        }

        let mut next_source = 0u32;
        let mut next_state = 0u32;
        let mut next_list = 0u32;
        for (owner, shard) in &self.shards {
            let dense = self.owners.get_mut(owner).unwrap();
            dense.sources = shard
                .rows()
                .sources
                .iter()
                .map(|row| {
                    let id = CheckedSourceId(next_source);
                    next_source = next_source.checked_add(1).ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new("dense source identity overflow")
                    })?;
                    self.source_by_key.insert(row.stable_key.clone(), id);
                    Ok(id)
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
            dense.states = shard
                .rows()
                .states
                .iter()
                .map(|_| {
                    let id = CheckedStateId(next_state);
                    next_state = next_state.checked_add(1).ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new("dense state identity overflow")
                    })?;
                    Ok(id)
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
            dense.lists = shard
                .rows()
                .lists
                .iter()
                .map(|_| {
                    let id = CheckedListId(next_list);
                    next_list = next_list.checked_add(1).ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new("dense list identity overflow")
                    })?;
                    Ok(id)
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
        }
        Ok(())
    }
}

impl CompatibilityLayout<'_> {
    fn local_declaration(
        &self,
        owner: &StableCheckOwnerKey,
        declaration: OwnerDeclarationId,
    ) -> Result<DeclId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .declarations
            .get(declaration.0 as usize)
            .copied()
            .filter(|id| id.0 != u32::MAX)
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense declaration {}",
                    declaration.0
                ))
            })
    }

    fn local_scope(
        &self,
        owner: &StableCheckOwnerKey,
        scope: OwnerScopeId,
    ) -> Result<LexicalScopeId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .scopes
            .get(scope.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense scope {}",
                    scope.0
                ))
            })
    }

    fn local_statement(
        &self,
        owner: &StableCheckOwnerKey,
        statement: OwnerStatementId,
    ) -> Result<CheckedStatementId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .statements
            .get(statement.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense statement {}",
                    statement.0
                ))
            })
    }

    fn local_expression(
        &self,
        owner: &StableCheckOwnerKey,
        expression: OwnerExpressionId,
    ) -> Result<CheckedExprId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .expressions
            .get(expression.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense expression {}",
                    expression.0
                ))
            })
    }

    fn local_call(
        &self,
        owner: &StableCheckOwnerKey,
        call: OwnerCallId,
    ) -> Result<CheckedCallId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .calls
            .get(call.0 as usize)
            .copied()
            .filter(|id| id.0 != u32::MAX)
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense call {}",
                    call.0
                ))
            })
    }

    fn stable_expression(
        &self,
        expression: &crate::ProjectOrderExpressionFact,
    ) -> Result<CheckedExprId, OwnerCompatibilityAssemblyError> {
        self.expression_by_key
            .get(&expression.expression)
            .copied()
            .filter(|_| self.shards.contains_key(&expression.owner))
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(
                    "project order fact references a missing stable expression",
                )
            })
    }

    fn stable_call(
        &self,
        call: &crate::ProjectOrderExpressionFact,
    ) -> Result<CheckedCallId, OwnerCompatibilityAssemblyError> {
        self.call_by_key
            .get(&(call.owner.clone(), call.expression.clone()))
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(
                    "project order fact references a missing stable call",
                )
            })
    }

    fn local_context_formal(
        &self,
        owner: &StableCheckOwnerKey,
        formal: OwnerContextFormalId,
    ) -> Result<ContextFormalId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .context_formals
            .get(formal.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense context formal {}",
                    formal.0
                ))
            })
    }

    fn local_source(
        &self,
        owner: &StableCheckOwnerKey,
        source: OwnerSourceId,
    ) -> Result<CheckedSourceId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .sources
            .get(source.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense source {}",
                    source.0
                ))
            })
    }

    fn local_state(
        &self,
        owner: &StableCheckOwnerKey,
        state: OwnerStateId,
    ) -> Result<CheckedStateId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .states
            .get(state.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense state {}",
                    state.0
                ))
            })
    }

    fn local_list(
        &self,
        owner: &StableCheckOwnerKey,
        list: OwnerListId,
    ) -> Result<CheckedListId, OwnerCompatibilityAssemblyError> {
        self.owner_layout(owner)?
            .lists
            .get(list.0 as usize)
            .copied()
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no dense list {}",
                    list.0
                ))
            })
    }

    fn evaluation_scope(
        &self,
        owner: &StableCheckOwnerKey,
        scope: &OwnerEvaluationScope,
    ) -> Result<CheckedEvaluationScope, OwnerCompatibilityAssemblyError> {
        Ok(match scope {
            OwnerEvaluationScope::Parent => CheckedEvaluationScope::Parent,
            OwnerEvaluationScope::Output { formal } => CheckedEvaluationScope::Output {
                formal: self.declaration(owner, formal)?,
            },
        })
    }

    fn semantic_path(
        &self,
        owner: &StableCheckOwnerKey,
        path: &OwnerSemanticPath,
    ) -> Result<CheckedSemanticPath, OwnerCompatibilityAssemblyError> {
        Ok(CheckedSemanticPath {
            anchor: self.declaration(owner, &path.anchor)?,
            projection: path.projection.clone(),
        })
    }

    fn abi_value_identity(
        &self,
        canonical_path: &str,
        declaration: Option<OwnerAbiDeclarationKey>,
    ) -> Result<Option<CheckedExternalDeclarationIdentityV1>, OwnerCompatibilityAssemblyError> {
        let Some(declaration) = declaration else {
            return Ok(None);
        };
        self.abi_values
            .iter()
            .find(|entry| entry.path == canonical_path && entry.key == declaration)
            .map(|entry| entry.contract.external_identity)
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "external value `{canonical_path}` has no exact construction ABI contract"
                ))
            })
    }

    fn owner_layout(
        &self,
        owner: &StableCheckOwnerKey,
    ) -> Result<&OwnerDenseLayout, OwnerCompatibilityAssemblyError> {
        self.owners.get(owner).ok_or_else(|| {
            OwnerCompatibilityAssemblyError::new(format!(
                "compatibility assembly has no dense layout for owner {owner:?}"
            ))
        })
    }

    fn scope(
        &self,
        current: &StableCheckOwnerKey,
        reference: &OwnerScopeRef,
    ) -> Result<LexicalScopeId, OwnerCompatibilityAssemblyError> {
        match reference {
            OwnerScopeRef::Local { scope } => self
                .owner_layout(current)?
                .scopes
                .get(scope.0 as usize)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing local scope {}",
                        scope.0
                    ))
                }),
            OwnerScopeRef::Imported { owner, scope } => self
                .scope_by_key
                .get(&(owner.clone(), scope.clone()))
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing imported scope {owner:?} {scope:?}"
                    ))
                }),
            OwnerScopeRef::ProjectRoot => Ok(LexicalScopeId(0)),
        }
    }

    fn declaration(
        &self,
        current: &StableCheckOwnerKey,
        reference: &OwnerDeclarationRef,
    ) -> Result<DeclId, OwnerCompatibilityAssemblyError> {
        match reference {
            OwnerDeclarationRef::Local { declaration } => self
                .owner_layout(current)?
                .declarations
                .get(declaration.0 as usize)
                .copied()
                .filter(|id| id.0 != u32::MAX)
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing local declaration {}",
                        declaration.0
                    ))
                }),
            OwnerDeclarationRef::Imported { owner, member } => match member {
                OwnerInterfaceMemberRef::PublicDeclaration => self
                    .owner_public_declaration
                    .get(owner)
                    .copied()
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(format!(
                            "owner {current:?} references missing public declaration for {owner:?}"
                        ))
                    }),
                OwnerInterfaceMemberRef::Parameter { ordinal } => self
                    .owner_parameter_declaration
                    .get(&(owner.clone(), *ordinal))
                    .copied()
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(format!(
                            "owner {current:?} references missing parameter {ordinal} for {owner:?}"
                        ))
                    }),
                OwnerInterfaceMemberRef::ContextFormal => Err(
                    OwnerCompatibilityAssemblyError::new(
                        "context formal cannot be used as a declaration reference",
                    ),
                ),
            },
            OwnerDeclarationRef::ImportedStable { owner, declaration } => self
                .declaration_by_key
                .get(&(owner.clone(), declaration.clone()))
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing stable declaration {owner:?} {declaration:?}"
                    ))
                }),
            OwnerDeclarationRef::Abi {
                canonical_name,
                declaration,
                member,
            } => self
                .abi_declarations
                .get(&(canonical_name.clone(), *declaration, *member))
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing ABI declaration `{canonical_name}` {member:?}"
                    ))
                }),
            OwnerDeclarationRef::ScopeOwner { scope } => {
                let dense = self.scope(current, scope)?;
                self.scope_owner(dense)
            }
        }
    }

    fn scope_owner(
        &self,
        dense: LexicalScopeId,
    ) -> Result<DeclId, OwnerCompatibilityAssemblyError> {
        for (owner, shard) in &self.shards {
            let owner_layout = &self.owners[owner];
            if let Some(local) = owner_layout.scopes.iter().position(|id| *id == dense) {
                let row = &shard.rows().scopes[local];
                if let Some(reference) = &row.owner {
                    return self.declaration(owner, reference);
                }
                if let Some(parent) = &row.parent {
                    return self.scope_owner(self.scope(owner, parent)?);
                }
            }
        }
        Err(OwnerCompatibilityAssemblyError::new(format!(
            "scope {} has no lexical declaration owner",
            dense.0
        )))
    }

    fn expression(
        &self,
        current: &StableCheckOwnerKey,
        reference: &OwnerExpressionRef,
    ) -> Result<CheckedExprId, OwnerCompatibilityAssemblyError> {
        match reference {
            OwnerExpressionRef::Local { expression } => self
                .owner_layout(current)?
                .expressions
                .get(expression.0 as usize)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing local expression {}",
                        expression.0
                    ))
                }),
            OwnerExpressionRef::Child { owner, expression } => self
                .expression_by_key
                .get(expression)
                .copied()
                .filter(|_| self.shards.contains_key(owner))
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing child expression in {owner:?}"
                    ))
                }),
        }
    }

    fn context_formal(
        &self,
        current: &StableCheckOwnerKey,
        reference: &OwnerContextFormalRef,
    ) -> Result<ContextFormalId, OwnerCompatibilityAssemblyError> {
        match reference {
            OwnerContextFormalRef::Local { formal } => self
                .owner_layout(current)?
                .context_formals
                .get(formal.0 as usize)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing local context formal {}",
                        formal.0
                    ))
                }),
            OwnerContextFormalRef::Imported { owner } => self
                .owner_context_formal
                .get(owner)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing context formal for {owner:?}"
                    ))
                }),
        }
    }

    fn source(
        &self,
        current: &StableCheckOwnerKey,
        reference: &OwnerSourceRef,
    ) -> Result<CheckedSourceId, OwnerCompatibilityAssemblyError> {
        match reference {
            OwnerSourceRef::Local { source } => self
                .owner_layout(current)?
                .sources
                .get(source.0 as usize)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing local source {}",
                        source.0
                    ))
                }),
            OwnerSourceRef::Imported { source } => {
                self.source_by_key.get(source).copied().ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing imported source {source:?}"
                    ))
                })
            }
        }
    }

    fn statement_child(
        &self,
        current: &StableCheckOwnerKey,
        child: &OwnerStatementChild,
    ) -> Result<CheckedStatementId, OwnerCompatibilityAssemblyError> {
        match child {
            OwnerStatementChild::Local { statement } => self
                .owner_layout(current)?
                .statements
                .get(statement.0 as usize)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references missing local child statement {}",
                        statement.0
                    ))
                }),
            OwnerStatementChild::Owner { owner } => self
                .owner_root_statement
                .get(owner)
                .copied()
                .ok_or_else(|| {
                    OwnerCompatibilityAssemblyError::new(format!(
                        "owner {current:?} references child owner {owner:?} without a root statement"
                    ))
                }),
        }
    }

    fn source_map(
        &self,
        owner: &StableCheckOwnerKey,
    ) -> Result<&OwnerSourceMap, OwnerCompatibilityAssemblyError> {
        self.source_maps.get(owner).copied().ok_or_else(|| {
            OwnerCompatibilityAssemblyError::new(format!(
                "compatibility assembly has no current source map for {owner:?}"
            ))
        })
    }

    fn source_span(
        &self,
        owner: &StableCheckOwnerKey,
        site: &OwnerSourceSite,
    ) -> Result<CheckedSpan, OwnerCompatibilityAssemblyError> {
        let map = self.source_map(owner)?;
        let (line, start, end) = match site {
            OwnerSourceSite::Statement { statement } => {
                let source = map
                    .statements()
                    .iter()
                    .find(|source| &source.stable_key == statement)
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(format!(
                            "owner {owner:?} source map has no statement {statement:?}"
                        ))
                    })?;
                (source.line, source.start, source.end)
            }
            OwnerSourceSite::Expression { expression } => {
                let source = map
                    .expressions()
                    .iter()
                    .find(|source| &source.expression == expression)
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(format!(
                            "owner {owner:?} source map has no expression {expression:?}"
                        ))
                    })?;
                (source.line, source.start, source.end)
            }
            OwnerSourceSite::FunctionParameter { statement, ordinal } => {
                let local = map
                    .statements()
                    .iter()
                    .find(|source| &source.stable_key == statement)
                    .map(|source| source.statement)
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(
                            "function parameter has no source statement",
                        )
                    })?;
                let anchor = map
                    .anchor(
                        &OwnerSourceAnchorSite::Statement { statement: local },
                        OwnerSourceAnchorRole::FunctionParameter { ordinal: *ordinal },
                    )
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(
                            "function parameter has no exact source anchor",
                        )
                    })?;
                (anchor.line, anchor.start, anchor.end)
            }
            OwnerSourceSite::CallArgument {
                expression,
                ordinal,
            } => self.anchor_span(
                map,
                expression,
                OwnerSourceAnchorRole::CallArgument { ordinal: *ordinal },
            )?,
            OwnerSourceSite::CallPass { expression } => {
                self.anchor_span(map, expression, OwnerSourceAnchorRole::CallPass)?
            }
            OwnerSourceSite::PipeArgument {
                expression,
                ordinal,
            } => self.anchor_span(
                map,
                expression,
                OwnerSourceAnchorRole::PipeArgument { ordinal: *ordinal },
            )?,
            OwnerSourceSite::PipePass { expression } => {
                self.anchor_span(map, expression, OwnerSourceAnchorRole::PipePass)?
            }
            OwnerSourceSite::RecordField {
                expression,
                ordinal,
            } => self.anchor_span(
                map,
                expression,
                OwnerSourceAnchorRole::RecordField { ordinal: *ordinal },
            )?,
            OwnerSourceSite::BlockBinding {
                expression,
                ordinal,
            } => self.anchor_span(
                map,
                expression,
                OwnerSourceAnchorRole::BlockBinding { ordinal: *ordinal },
            )?,
            OwnerSourceSite::PatternBinding {
                expression,
                ordinal: _,
            } => {
                let source = map
                    .expressions()
                    .iter()
                    .find(|source| &source.expression == expression)
                    .ok_or_else(|| {
                        OwnerCompatibilityAssemblyError::new(
                            "pattern binding has no source expression",
                        )
                    })?;
                (source.line, source.start, source.end)
            }
            OwnerSourceSite::Synthetic { .. } => return Ok(CheckedSpan::default()),
        };
        self.global_span(owner, line, start, end)
    }

    fn anchor_span(
        &self,
        map: &OwnerSourceMap,
        expression: &StableExpressionKey,
        role: OwnerSourceAnchorRole,
    ) -> Result<(u64, u64, u64), OwnerCompatibilityAssemblyError> {
        let anchor = map
            .anchor(
                &OwnerSourceAnchorSite::Expression {
                    expression: expression.clone(),
                },
                role,
            )
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {:?} source map has no exact expression anchor",
                    map.owner()
                ))
            })?;
        Ok((anchor.line, anchor.start, anchor.end))
    }

    fn global_span(
        &self,
        owner: &StableCheckOwnerKey,
        line: u64,
        start: u64,
        end: u64,
    ) -> Result<CheckedSpan, OwnerCompatibilityAssemblyError> {
        let layout = self
            .project
            .source_layouts()
            .iter()
            .find(|layout| &layout.source_unit_id == owner.source_unit_id())
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "owner {owner:?} has no project source layout"
                ))
            })?;
        let line = checked_usize(line, "source line")?;
        let start = checked_usize(start, "source start")?;
        let end = checked_usize(end, "source end")?;
        Ok(CheckedSpan {
            line: layout
                .start_line
                .checked_add(line.saturating_sub(1))
                .ok_or_else(|| OwnerCompatibilityAssemblyError::new("global line overflow"))?,
            start: layout
                .start_byte
                .checked_add(start)
                .ok_or_else(|| OwnerCompatibilityAssemblyError::new("global start overflow"))?,
            end: layout
                .start_byte
                .checked_add(end)
                .ok_or_else(|| OwnerCompatibilityAssemblyError::new("global end overflow"))?,
        })
    }

    fn record_field(
        &self,
        owner: &StableCheckOwnerKey,
        field: &OwnerRecordField,
    ) -> Result<CheckedRecordField, OwnerCompatibilityAssemblyError> {
        Ok(CheckedRecordField {
            declaration: field
                .declaration
                .as_ref()
                .map(|declaration| self.declaration(owner, declaration))
                .transpose()?,
            name: field.name.clone(),
            value: self.expression(owner, &field.value)?,
            spread: field.spread,
            span: self.source_span(owner, &field.source)?,
        })
    }

    fn expression_kind(
        &self,
        owner: &StableCheckOwnerKey,
        kind: &OwnerExpressionKind,
    ) -> Result<CheckedExpressionKind, OwnerCompatibilityAssemblyError> {
        let expressions = |values: &[OwnerExpressionRef]| {
            values
                .iter()
                .map(|value| self.expression(owner, value))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(match kind {
            OwnerExpressionKind::Read {
                target,
                projection,
                source_seed,
            } => CheckedExpressionKind::Read {
                target: self.declaration(owner, target)?,
                projection: projection.clone(),
                source: source_seed
                    .as_ref()
                    .map(|source| -> Result<_, OwnerCompatibilityAssemblyError> {
                        Ok(CheckedSourceRead {
                            source: self.source(owner, &source.source)?,
                            payload_projection: source.payload_projection.clone(),
                        })
                    })
                    .transpose()?,
            },
            OwnerExpressionKind::Passed {
                formal,
                projection,
                access,
            } => CheckedExpressionKind::Passed {
                formal: self.context_formal(owner, formal)?,
                projection: projection.clone(),
                access: *access,
            },
            OwnerExpressionKind::ExternalRead {
                canonical_path,
                declaration,
            } => CheckedExpressionKind::ExternalRead {
                canonical_path: canonical_path.clone(),
                external_identity: self.abi_value_identity(canonical_path, *declaration)?,
            },
            OwnerExpressionKind::Drain { target, projection } => CheckedExpressionKind::Drain {
                target: self.declaration(owner, target)?,
                projection: projection.clone(),
            },
            OwnerExpressionKind::Text { value } => CheckedExpressionKind::Text {
                value: value.clone(),
            },
            OwnerExpressionKind::TextTemplate { segments } => CheckedExpressionKind::TextTemplate {
                segments: segments
                    .iter()
                    .map(|segment| match segment {
                        OwnerTextSegment::Static { value } => Ok(CheckedTextSegment::Static {
                            value: value.clone(),
                        }),
                        OwnerTextSegment::Dynamic { value } => Ok(CheckedTextSegment::Dynamic {
                            value: self.expression(owner, value)?,
                        }),
                    })
                    .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
            },
            OwnerExpressionKind::Number { value } => CheckedExpressionKind::Number {
                value: value.clone(),
            },
            OwnerExpressionKind::BytesByte { value } => {
                CheckedExpressionKind::BytesByte { value: *value }
            }
            OwnerExpressionKind::Absent => CheckedExpressionKind::Absent,
            OwnerExpressionKind::Flush { payload } => CheckedExpressionKind::Flush {
                payload: self.expression(owner, payload)?,
            },
            OwnerExpressionKind::Tag { name } => CheckedExpressionKind::Tag { name: name.clone() },
            OwnerExpressionKind::TaggedObject { tag, fields } => {
                CheckedExpressionKind::TaggedObject {
                    tag: tag.clone(),
                    fields: fields
                        .iter()
                        .map(|field| self.record_field(owner, field))
                        .collect::<Result<Vec<_>, _>>()?,
                }
            }
            OwnerExpressionKind::Source => CheckedExpressionKind::Source,
            OwnerExpressionKind::Call { call } => CheckedExpressionKind::Call {
                call: self.local_call(owner, *call)?,
            },
            OwnerExpressionKind::Draining { input } => CheckedExpressionKind::Draining {
                input: self.expression(owner, input)?,
            },
            OwnerExpressionKind::Hold { initial, name } => CheckedExpressionKind::Hold {
                initial: self.expression(owner, initial)?,
                name: name.clone(),
            },
            OwnerExpressionKind::Latest { branches } => CheckedExpressionKind::Latest {
                branches: expressions(branches)?,
            },
            OwnerExpressionKind::When { input, arms } => CheckedExpressionKind::When {
                input: self.expression(owner, input)?,
                arms: expressions(arms)?,
            },
            OwnerExpressionKind::While { input, arms } => CheckedExpressionKind::While {
                input: self.expression(owner, input)?,
                arms: expressions(arms)?,
            },
            OwnerExpressionKind::Then { input, output } => CheckedExpressionKind::Then {
                input: self.expression(owner, input)?,
                output: output
                    .as_ref()
                    .map(|output| self.expression(owner, output))
                    .transpose()?,
            },
            OwnerExpressionKind::Infix { left, op, right } => CheckedExpressionKind::Infix {
                left: self.expression(owner, left)?,
                op: op.clone(),
                right: self.expression(owner, right)?,
            },
            OwnerExpressionKind::MatchArm {
                pattern,
                bindings,
                output,
            } => CheckedExpressionKind::MatchArm {
                pattern: pattern.clone(),
                bindings: bindings
                    .iter()
                    .map(|binding| self.local_declaration(owner, *binding))
                    .collect::<Result<Vec<_>, _>>()?,
                output: output
                    .as_ref()
                    .map(|output| self.expression(owner, output))
                    .transpose()?,
            },
            OwnerExpressionKind::Block { bindings, result } => CheckedExpressionKind::Block {
                bindings: bindings
                    .iter()
                    .map(|binding| {
                        Ok(CheckedBlockBinding {
                            declaration: self.declaration(owner, &binding.declaration)?,
                            value: self.expression(owner, &binding.value)?,
                            span: self.source_span(owner, &binding.source)?,
                        })
                    })
                    .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
                result: result
                    .as_ref()
                    .map(|result| self.expression(owner, result))
                    .transpose()?,
            },
            OwnerExpressionKind::Object { fields } => CheckedExpressionKind::Object {
                fields: fields
                    .iter()
                    .map(|field| self.record_field(owner, field))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            OwnerExpressionKind::List { capacity, items } => CheckedExpressionKind::List {
                capacity: *capacity,
                items: expressions(items)?,
            },
            OwnerExpressionKind::Bytes { fixed_size, items } => CheckedExpressionKind::Bytes {
                fixed_size: *fixed_size,
                items: expressions(items)?,
            },
            OwnerExpressionKind::Delimiter => CheckedExpressionKind::Delimiter,
            OwnerExpressionKind::Invalid { tokens } => CheckedExpressionKind::Invalid {
                tokens: tokens.clone(),
            },
            OwnerExpressionKind::MapEntry { key, value } => CheckedExpressionKind::MapEntry {
                key: self.expression(owner, key)?,
                value: self.expression(owner, value)?,
            },
            OwnerExpressionKind::Map { entries } => CheckedExpressionKind::Map {
                entries: expressions(entries)?,
            },
            OwnerExpressionKind::Set { items } => CheckedExpressionKind::Set {
                items: expressions(items)?,
            },
            OwnerExpressionKind::Bits { value } => CheckedExpressionKind::Bits {
                value: value.clone(),
            },
        })
    }

    fn statement_kind(
        &self,
        owner: &StableCheckOwnerKey,
        kind: &OwnerStatementKind,
    ) -> Result<CheckedStatementKind, OwnerCompatibilityAssemblyError> {
        Ok(match kind {
            OwnerStatementKind::Function { declaration } => CheckedStatementKind::Function {
                declaration: self.local_declaration(owner, *declaration)?,
            },
            OwnerStatementKind::Field { declaration } => CheckedStatementKind::Field {
                declaration: self.local_declaration(owner, *declaration)?,
            },
            OwnerStatementKind::Source { declaration, event } => CheckedStatementKind::Source {
                declaration: declaration
                    .map(|declaration| self.local_declaration(owner, declaration))
                    .transpose()?,
                event: event.clone(),
            },
            OwnerStatementKind::Hold { declaration, name } => CheckedStatementKind::Hold {
                declaration: declaration
                    .map(|declaration| self.local_declaration(owner, declaration))
                    .transpose()?,
                name: name.clone(),
            },
            OwnerStatementKind::List {
                declaration,
                capacity,
            } => CheckedStatementKind::List {
                declaration: declaration
                    .map(|declaration| self.local_declaration(owner, declaration))
                    .transpose()?,
                capacity: *capacity,
            },
            OwnerStatementKind::Block => CheckedStatementKind::Block,
            OwnerStatementKind::Spread => CheckedStatementKind::Spread,
            OwnerStatementKind::Expression => CheckedStatementKind::Expression,
        })
    }

    fn resource(
        &self,
        owner: &StableCheckOwnerKey,
        resource: &OwnerResourceBinding,
    ) -> Result<CheckedResourceBinding, OwnerCompatibilityAssemblyError> {
        Ok(match resource {
            OwnerResourceBinding::Source { source } => CheckedResourceBinding::Source {
                source: self.source(owner, source)?,
            },
            OwnerResourceBinding::State { state } => CheckedResourceBinding::State {
                state: self.local_state(owner, *state)?,
            },
            OwnerResourceBinding::ListAuthority { list } => CheckedResourceBinding::ListAuthority {
                list: self.local_list(owner, *list)?,
            },
            OwnerResourceBinding::ListAlias { target } => CheckedResourceBinding::ListAlias {
                target: self.declaration(owner, target)?,
            },
        })
    }

    fn contextual_operation(
        &self,
        owner: &StableCheckOwnerKey,
        operation: &OwnerContextualOperation,
    ) -> Result<CheckedContextualOperation, OwnerCompatibilityAssemblyError> {
        macro_rules! declaration {
            ($value:expr) => {
                self.declaration(owner, $value)?
            };
        }
        Ok(match operation {
            OwnerContextualOperation::Map { list, row, body } => CheckedContextualOperation::Map {
                list: declaration!(list),
                row: declaration!(row),
                body: declaration!(body),
            },
            OwnerContextualOperation::Filter {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Filter {
                list: declaration!(list),
                row: declaration!(row),
                predicate: declaration!(predicate),
            },
            OwnerContextualOperation::Retain {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Retain {
                list: declaration!(list),
                row: declaration!(row),
                predicate: declaration!(predicate),
            },
            OwnerContextualOperation::Remove {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Remove {
                list: declaration!(list),
                row: declaration!(row),
                predicate: declaration!(predicate),
            },
            OwnerContextualOperation::Every {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Every {
                list: declaration!(list),
                row: declaration!(row),
                predicate: declaration!(predicate),
            },
            OwnerContextualOperation::Any {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Any {
                list: declaration!(list),
                row: declaration!(row),
                predicate: declaration!(predicate),
            },
            OwnerContextualOperation::Find {
                list,
                row,
                predicate,
            } => CheckedContextualOperation::Find {
                list: declaration!(list),
                row: declaration!(row),
                predicate: declaration!(predicate),
            },
            OwnerContextualOperation::SortBy {
                list,
                row,
                key,
                direction,
            } => CheckedContextualOperation::SortBy {
                list: declaration!(list),
                row: declaration!(row),
                key: declaration!(key),
                direction: declaration!(direction),
            },
            OwnerContextualOperation::ThenBy {
                list,
                row,
                key,
                direction,
            } => CheckedContextualOperation::ThenBy {
                list: declaration!(list),
                row: declaration!(row),
                key: declaration!(key),
                direction: declaration!(direction),
            },
        })
    }

    fn call_entry(
        &self,
        owner: &StableCheckOwnerKey,
        entry: &OwnerCallEntry,
    ) -> Result<CheckedCallEntry, OwnerCompatibilityAssemblyError> {
        Ok(match entry {
            OwnerCallEntry::Input {
                formal,
                name,
                value,
                from_pipe,
                evaluation_scope,
            } => CheckedCallEntry::Input {
                formal: self.declaration(owner, formal)?,
                name: name.clone(),
                value: self.expression(owner, value)?,
                from_pipe: *from_pipe,
                evaluation_scope: self.evaluation_scope(owner, evaluation_scope)?,
            },
            OwnerCallEntry::FreshOut {
                formal,
                name,
                output,
                scope_id,
            } => CheckedCallEntry::FreshOut {
                formal: self.declaration(owner, formal)?,
                name: name.clone(),
                output: self.local_declaration(owner, *output)?,
                scope_id: self.local_scope(owner, *scope_id)?,
            },
            OwnerCallEntry::ForwardOut {
                formal,
                name,
                target,
                target_name,
            } => CheckedCallEntry::ForwardOut {
                formal: self.declaration(owner, formal)?,
                name: name.clone(),
                target: self.declaration(owner, target)?,
                target_name: target_name.clone(),
            },
        })
    }

    fn context_binding(
        &self,
        owner: &StableCheckOwnerKey,
        binding: &OwnerContextBinding,
    ) -> Result<CheckedContextBinding, OwnerCompatibilityAssemblyError> {
        Ok(match binding {
            OwnerContextBinding::Explicit { value, source } => CheckedContextBinding::Explicit {
                value: self.expression(owner, value)?,
                span: self.source_span(owner, source)?,
            },
            OwnerContextBinding::Inherited { formal } => CheckedContextBinding::Inherited {
                formal: self.context_formal(owner, formal)?,
            },
            OwnerContextBinding::None => CheckedContextBinding::None,
        })
    }
}

fn abi_parameter_declaration(
    layout: &CompatibilityLayout<'_>,
    entry: &AbiCallableEntry,
    ordinal: u32,
) -> Result<DeclId, OwnerCompatibilityAssemblyError> {
    layout
        .abi_declarations
        .get(&(
            entry.name.clone(),
            entry.key,
            OwnerAbiMemberRef::Parameter { ordinal },
        ))
        .copied()
        .ok_or_else(|| {
            OwnerCompatibilityAssemblyError::new(format!(
                "ABI callable `{}` has no dense parameter {ordinal}",
                entry.name
            ))
        })
}

fn abi_evaluation_scope(
    layout: &CompatibilityLayout<'_>,
    entry: &AbiCallableEntry,
    scope: OwnerAbiEvaluationScope,
) -> Result<CheckedEvaluationScope, OwnerCompatibilityAssemblyError> {
    Ok(match scope {
        OwnerAbiEvaluationScope::Parent => CheckedEvaluationScope::Parent,
        OwnerAbiEvaluationScope::Output { parameter_ordinal } => CheckedEvaluationScope::Output {
            formal: abi_parameter_declaration(layout, entry, parameter_ordinal)?,
        },
    })
}

fn abi_contextual_operation(
    layout: &CompatibilityLayout<'_>,
    entry: &AbiCallableEntry,
    operation: OwnerAbiContextualOperation,
) -> Result<CheckedContextualOperation, OwnerCompatibilityAssemblyError> {
    let parameter = |ordinal| abi_parameter_declaration(layout, entry, ordinal);
    Ok(match operation {
        OwnerAbiContextualOperation::Map { list, row, body } => CheckedContextualOperation::Map {
            list: parameter(list)?,
            row: parameter(row)?,
            body: parameter(body)?,
        },
        OwnerAbiContextualOperation::Filter {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Filter {
            list: parameter(list)?,
            row: parameter(row)?,
            predicate: parameter(predicate)?,
        },
        OwnerAbiContextualOperation::Retain {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Retain {
            list: parameter(list)?,
            row: parameter(row)?,
            predicate: parameter(predicate)?,
        },
        OwnerAbiContextualOperation::Remove {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Remove {
            list: parameter(list)?,
            row: parameter(row)?,
            predicate: parameter(predicate)?,
        },
        OwnerAbiContextualOperation::Every {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Every {
            list: parameter(list)?,
            row: parameter(row)?,
            predicate: parameter(predicate)?,
        },
        OwnerAbiContextualOperation::Any {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Any {
            list: parameter(list)?,
            row: parameter(row)?,
            predicate: parameter(predicate)?,
        },
        OwnerAbiContextualOperation::Find {
            list,
            row,
            predicate,
        } => CheckedContextualOperation::Find {
            list: parameter(list)?,
            row: parameter(row)?,
            predicate: parameter(predicate)?,
        },
        OwnerAbiContextualOperation::SortBy {
            list,
            row,
            key,
            direction,
        } => CheckedContextualOperation::SortBy {
            list: parameter(list)?,
            row: parameter(row)?,
            key: parameter(key)?,
            direction: parameter(direction)?,
        },
        OwnerAbiContextualOperation::ThenBy {
            list,
            row,
            key,
            direction,
        } => CheckedContextualOperation::ThenBy {
            list: parameter(list)?,
            row: parameter(row)?,
            key: parameter(key)?,
            direction: parameter(direction)?,
        },
    })
}

fn insert_dense<T>(
    rows: &mut [Option<T>],
    index: usize,
    row: T,
    domain: &str,
) -> Result<(), OwnerCompatibilityAssemblyError> {
    let slot = rows.get_mut(index).ok_or_else(|| {
        OwnerCompatibilityAssemblyError::new(format!(
            "checked owner {domain} row {index} is outside the dense compatibility table"
        ))
    })?;
    if slot.replace(row).is_some() {
        return Err(OwnerCompatibilityAssemblyError::new(format!(
            "checked owner {domain} row {index} is defined more than once"
        )));
    }
    Ok(())
}

fn finish_dense<T>(
    rows: Vec<Option<T>>,
    domain: &str,
) -> Result<Vec<T>, OwnerCompatibilityAssemblyError> {
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            row.ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "checked owner assembly is missing dense {domain} row {index}"
                ))
            })
        })
        .collect()
}

fn owner_function_type_table(callables: &[CheckedCallableSignature]) -> FunctionTypeTable {
    let mut entries = callables
        .iter()
        .filter(|callable| callable.kind == CheckedCallableKind::User)
        .map(|callable| FunctionTypeEntry {
            callable: callable.decl_id,
            name: callable.name.clone(),
            parameters: callable
                .parameters
                .iter()
                .map(|parameter| FunctionTypeParameterEntry {
                    formal: parameter.decl_id,
                    ordinal: parameter.ordinal,
                    name: parameter.name.clone(),
                    flow_type: parameter.flow_type.clone(),
                })
                .collect(),
            result: callable.result.clone(),
            effect: callable.effect,
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.callable);
    FunctionTypeTable { entries }
}

fn owner_named_value_type_table(
    syntax: &crate::TypecheckSyntaxProgram,
    fields: &CheckedProgramFields,
) -> Result<NamedValueTypeTable, String> {
    let mut syntax_sites = BTreeMap::new();
    crate::collect_canonical_named_value_sites(
        syntax.statements(),
        &mut Vec::new(),
        &mut syntax_sites,
    );
    for sites in syntax_sites.values_mut() {
        for site in sites {
            *site = syntax.checked_statement_id(*site).0 as usize;
        }
    }
    let mut table = NamedValueTypeTable {
        checked_statement_sites: Vec::new(),
        entries: syntax_sites
            .keys()
            .cloned()
            .map(|path| NamedValueTypeEntry {
                path,
                origins: Vec::new(),
                flow_type: crate::unknown_flow_type(),
            })
            .collect(),
    };
    let lookup = crate::CheckedProgramLookup::new(fields);
    crate::refresh_named_value_types_from_checked_program(&mut table, &syntax_sites, &lookup)?;
    Ok(table)
}

fn syntax_statement_by_stable_key<'a>(
    syntax: &'a crate::TypecheckSyntaxProgram,
    project: &ProjectSyntaxSnapshot,
    key: &StableStatementKey,
) -> Option<&'a AstStatement> {
    fn find<'a>(
        statements: &'a [AstStatement],
        project: &ProjectSyntaxSnapshot,
        key: &StableStatementKey,
    ) -> Option<&'a AstStatement> {
        for statement in statements {
            if project.stable_statement_key(statement.id).as_ref() == Some(key) {
                return Some(statement);
            }
            if let Some(found) = find(&statement.children, project, key) {
                return Some(found);
            }
        }
        None
    }

    syntax
        .root_statement_units()
        .find_map(|statements| find(statements, project, key))
}

fn owner_output_root_types(
    syntax: &crate::TypecheckSyntaxProgram,
    project: &ProjectSyntaxSnapshot,
    facts: &ProjectDiagnosticFacts,
    fields: &CheckedProgramFields,
) -> Result<Vec<OutputRootTypeEntry>, OwnerCompatibilityAssemblyError> {
    let lookup = crate::CheckedProgramLookup::new(fields);
    let mut entries = Vec::with_capacity(facts.output_roots().len());
    for fact in facts.output_roots() {
        let source =
            syntax_statement_by_stable_key(syntax, project, &fact.statement).ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new(format!(
                    "output root `{}` has no stable syntax statement",
                    fact.name
                ))
            })?;
        let statement_id = syntax.checked_statement_id(source.id);
        let checked_statement = lookup.unique_statement(statement_id).ok_or_else(|| {
            OwnerCompatibilityAssemblyError::new(format!(
                "output root `{}` has no exact checked statement",
                fact.name
            ))
        })?;
        let declaration = match checked_statement.kind {
            CheckedStatementKind::Field { declaration }
            | CheckedStatementKind::List {
                declaration: Some(declaration),
                ..
            } => declaration,
            _ => {
                return Err(OwnerCompatibilityAssemblyError::new(format!(
                    "output root `{}` has no exact checked declaration identity",
                    fact.name
                )));
            }
        };
        let ty = checked_statement
            .value
            .and_then(|value| lookup.expressions.get(&value).copied())
            .map(|expression| {
                expression.flush_type.as_ref().map_or_else(
                    || expression.flow_type.ty.clone(),
                    |flush_type| crate::union_structural_type(&expression.flow_type.ty, flush_type),
                )
            })
            .unwrap_or(Type::Unknown);
        if ty != fact.ty {
            return Err(OwnerCompatibilityAssemblyError::new(format!(
                "output root `{}` checked type differs from its project diagnostic fact",
                fact.name
            )));
        }
        entries.push(OutputRootTypeEntry {
            name: fact.name.clone(),
            declaration,
            statement: statement_id,
            value: checked_statement.value,
            ty,
        });
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn owner_render_slot_table(
    syntax: &crate::TypecheckSyntaxProgram,
    project: &ProjectSyntaxSnapshot,
    facts: &ProjectDiagnosticFacts,
    fields: &CheckedProgramFields,
) -> Result<RenderSlotTable, OwnerCompatibilityAssemblyError> {
    let mut slots = Vec::with_capacity(facts.render_slots().len());
    for fact in facts.render_slots() {
        let source =
            syntax_statement_by_stable_key(syntax, project, &fact.statement).ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new("render slot has no exact syntax statement")
            })?;
        let statement_id = syntax.checked_statement_id(source.id);
        let statement = fields
            .statements
            .get(statement_id.0 as usize)
            .filter(|statement| statement.id == statement_id)
            .ok_or_else(|| {
                OwnerCompatibilityAssemblyError::new("render slot checked statement is missing")
            })?;
        if statement.value_use != CheckedValueUse::RenderSlot {
            return Err(OwnerCompatibilityAssemblyError::new(
                "project render fact points to a non-render checked statement",
            ));
        }
        let value_expr_id = statement.value.map(|value| value.0 as usize);
        let actual_type = statement
            .value
            .and_then(|value| fields.expressions.get(value.0 as usize))
            .map(|expression| expression.flow_type.ty.clone())
            .unwrap_or_else(|| {
                if matches!(fact.slot_name.as_str(), "items" | "children") {
                    Type::List(Type::shared(crate::open_object_type()))
                } else {
                    crate::open_object_type()
                }
            });
        if actual_type != fact.actual_type {
            return Err(OwnerCompatibilityAssemblyError::new(format!(
                "render slot `{}` checked type differs from its project diagnostic fact",
                fact.slot_name
            )));
        }
        slots.push(RenderSlot {
            slot_statement_id: statement.id.0 as usize,
            slot_name: fact.slot_name.clone(),
            expected_contract: fact.expected_contract.clone(),
            value_expr_id,
            actual_type,
            diagnostics: fact.diagnostics.to_vec(),
        });
    }
    slots.sort_by_key(|slot| slot.slot_statement_id);
    Ok(RenderSlotTable { slots })
}

fn owner_host_port_table(
    facts: &ProjectDiagnosticFacts,
    fields: &CheckedProgramFields,
    outputs: &[OutputRootTypeEntry],
) -> Result<HostPortTable, OwnerCompatibilityAssemblyError> {
    let table = crate::resolve_checked_host_port_table(facts.host_ports(), fields, outputs);
    crate::validate_checked_host_port_source_payload_types(fields, facts.host_ports()).map_err(
        |error| {
            OwnerCompatibilityAssemblyError::new(format!(
                "checked host source payload differs from project diagnostic facts: {error}"
            ))
        },
    )?;
    reconcile_owner_host_port_resolution(facts.host_port_resolution_error(), table)
}

fn reconcile_owner_host_port_resolution(
    expected_error: Option<&str>,
    actual: Result<HostPortTable, String>,
) -> Result<HostPortTable, OwnerCompatibilityAssemblyError> {
    match (expected_error, actual) {
        (None, Ok(table)) => Ok(table),
        (Some(expected), Err(actual)) if expected == actual => Ok(HostPortTable::default()),
        (None, Err(actual)) => Err(OwnerCompatibilityAssemblyError::new(format!(
            "checked host-port relocation unexpectedly failed: {actual}"
        ))),
        (Some(expected), Err(actual)) => Err(OwnerCompatibilityAssemblyError::new(format!(
            "checked host-port relocation failed differently from project diagnostic facts\nexpected: {expected}\nfound: {actual}"
        ))),
        (Some(expected), Ok(_)) => Err(OwnerCompatibilityAssemblyError::new(format!(
            "checked host-port rows unexpectedly became relocatable; project diagnostic facts recorded: {expected}"
        ))),
    }
}

fn owner_order_chains(
    facts: &ProjectDiagnosticFacts,
    layout: &CompatibilityLayout<'_>,
) -> Result<Vec<CheckedCallOrderChain>, OwnerCompatibilityAssemblyError> {
    let mut chains = Vec::with_capacity(facts.order().chains().len());
    let mut seen = BTreeSet::new();
    for fact in facts.order().chains() {
        let call = layout.stable_call(&fact.call)?;
        if !seen.insert(call) {
            return Err(OwnerCompatibilityAssemblyError::new(
                "project order facts repeat one checked call",
            ));
        }
        let keys = fact
            .keys
            .iter()
            .map(|key| {
                Ok(CheckedOrderKey {
                    call_path: key
                        .call_path
                        .iter()
                        .map(|call| layout.stable_call(call))
                        .collect::<Result<Vec<_>, _>>()?,
                    key: layout.stable_expression(&key.key)?,
                    direction: match &key.direction {
                        crate::ProjectOrderDirectionFact::Ascending => {
                            CheckedOrderDirection::Ascending
                        }
                        crate::ProjectOrderDirectionFact::Descending => {
                            CheckedOrderDirection::Descending
                        }
                        crate::ProjectOrderDirectionFact::Dynamic { expression } => {
                            CheckedOrderDirection::Dynamic {
                                expression: layout.stable_expression(expression)?,
                            }
                        }
                    },
                    key_type: key.key_type.clone(),
                    pure: key.pure,
                    total: key.total,
                })
            })
            .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
        chains.push(CheckedCallOrderChain {
            call,
            chain: CheckedOrderChain { keys },
        });
    }
    chains.sort_by_key(|chain| chain.call);
    Ok(chains)
}

fn canonicalize_owner_diagnostics(diagnostics: &mut Vec<TypeDiagnostic>) {
    diagnostics.sort_by(|left, right| {
        let severity = |severity| match severity {
            DiagnosticSeverity::Error => 0u8,
            DiagnosticSeverity::Warning => 1u8,
        };
        (
            left.line,
            left.start,
            left.end,
            severity(left.severity),
            &left.message,
        )
            .cmp(&(
                right.line,
                right.start,
                right.end,
                severity(right.severity),
                &right.message,
            ))
    });
    diagnostics.dedup();
}

/// Assemble already-checked owner products into the dense compatibility DTO.
///
/// This is a relocation and projection step only. It neither constructs a
/// whole-project checker database nor performs constraint solving.
pub fn assemble_checked_owner_project<'a>(
    project: &'a ProjectSyntaxSnapshot,
    role: ProgramRole,
    external_types: ExternalTypeEnvironment,
    project_diagnostic_facts: &ProjectDiagnosticFacts,
    diagnostics_aggregate: &crate::OwnerDiagnosticsAggregate,
    shards: impl IntoIterator<Item = &'a CheckedOwnerShard>,
    source_maps: impl IntoIterator<Item = &'a OwnerSourceMap>,
    construction_abis: impl IntoIterator<Item = &'a OwnerConstructionAbiEnvironment>,
) -> Result<CheckedOwnerProjectAssembly, OwnerCompatibilityAssemblyError> {
    if external_types.current_role != role {
        return Err(OwnerCompatibilityAssemblyError::new(
            "checked owner assembly role does not match its external type environment",
        ));
    }
    if project_diagnostic_facts.source_bundle_digest_v1() != project.source_bundle_digest_v1() {
        return Err(OwnerCompatibilityAssemblyError::new(
            "checked owner assembly project diagnostic facts have a different source bundle",
        ));
    }
    if diagnostics_aggregate.source_bundle_digest_v1() != project.source_bundle_digest_v1() {
        return Err(OwnerCompatibilityAssemblyError::new(
            "checked owner assembly diagnostics aggregate has a different source bundle",
        ));
    }
    if diagnostics_aggregate.project_facts_fingerprint_v1()
        != project_diagnostic_facts.fingerprint_v1()
    {
        return Err(OwnerCompatibilityAssemblyError::new(
            "checked owner assembly diagnostics aggregate was produced from different project diagnostic facts",
        ));
    }
    let trace = std::env::var_os("BOON_OWNER_COMPAT_TRACE").is_some();
    let mut phase_started = Instant::now();
    let shards = shards.into_iter().collect::<Vec<_>>();
    let source_maps = source_maps.into_iter().collect::<Vec<_>>();
    let construction_abis = construction_abis.into_iter().collect::<Vec<_>>();
    let mut shard_fingerprints = shards
        .iter()
        .map(|shard| (shard.owner().clone(), shard.fingerprint_v1()))
        .collect::<Vec<_>>();
    let mut source_map_fingerprints = source_maps
        .iter()
        .map(|source_map| (source_map.owner().clone(), source_map.fingerprint_v2()))
        .collect::<Vec<_>>();
    let mut construction_abi_fingerprints = construction_abis
        .iter()
        .map(|abi| (abi.owner().clone(), abi.fingerprint_v1()))
        .collect::<Vec<_>>();
    shard_fingerprints.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    source_map_fingerprints.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    construction_abi_fingerprints.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    // The dense compatibility DTO is a deterministic relocation/projection of
    // these already-proof-bearing inputs. Hashing the DTO itself serialized
    // every repeated structural type into one giant CBOR buffer, duplicating
    // checked construction proof and making a large valid project consume
    // gigabytes merely to publish request currentness. Commit the exact input
    // basis instead; every semantic row, source position, ABI contract, role,
    // and source revision is covered by one of these compact fingerprints.
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        b"boon.checked-owner-project-assembly-basis.v5\0",
        &(
            project.source_bundle_digest_v1(),
            role,
            &external_types,
            project_diagnostic_facts.fingerprint_v1(),
            diagnostics_aggregate.fingerprint_v1(),
            &shard_fingerprints,
            &source_map_fingerprints,
            &construction_abi_fingerprints,
        ),
    )
    .map_err(|error| {
        OwnerCompatibilityAssemblyError::new(format!(
            "cannot fingerprint checked owner project assembly basis: {error}"
        ))
    })?;
    trace_compat_phase(trace, "compact-basis", &mut phase_started, shards.len());
    let layout = CompatibilityLayout::new(
        project,
        role,
        shards.iter().copied(),
        source_maps.iter().copied(),
        construction_abis.iter().copied(),
    )?;
    trace_compat_phase(trace, "layout", &mut phase_started, layout.shards.len());

    let mut scopes = vec![CheckedScope {
        id: LexicalScopeId(0),
        parent: None,
        owner: None,
        kind: CheckedScopeKind::Root,
        span: CheckedSpan::default(),
    }];
    let mut declarations = Vec::new();
    let mut callables = Vec::new();

    for entry in &layout.abi_callables {
        let declaration = layout.abi_declarations[&(
            entry.name.clone(),
            entry.key,
            OwnerAbiMemberRef::Declaration,
        )];
        let parameters = entry
            .contract
            .parameters
            .iter()
            .map(|parameter| {
                let decl_id = abi_parameter_declaration(&layout, entry, parameter.ordinal)?;
                Ok(CheckedParameter {
                    decl_id,
                    name: parameter.name.clone(),
                    kind: parameter.kind,
                    ordinal: parameter.ordinal as usize,
                    flow_type: parameter.flow_type.clone(),
                    requirement: parameter.requirement.clone(),
                    evaluation_scope: abi_evaluation_scope(
                        &layout,
                        entry,
                        parameter.evaluation_scope,
                    )?,
                    start: 0,
                    end: 0,
                })
            })
            .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
        let contexts = entry
            .contract
            .contexts
            .iter()
            .map(|context| {
                Ok(CheckedCallableContext {
                    name: context.name.clone(),
                    kind: context.kind,
                    provider: abi_parameter_declaration(
                        &layout,
                        entry,
                        context.provider_parameter_ordinal,
                    )?,
                    flow_type: context.flow_type.clone(),
                })
            })
            .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?;
        callables.push(CheckedCallableSignature {
            decl_id: declaration,
            scope_id: LexicalScopeId(0),
            kind: entry.contract.kind,
            name: entry.name.clone(),
            intrinsic: entry.contract.intrinsic,
            external_identity: entry.contract.external_identity,
            parameters: parameters.clone(),
            contexts,
            context_formal: None,
            result: entry.contract.result.clone(),
            role: entry.contract.role,
            effect: entry.contract.effect,
            body: None,
            result_expression: None,
            contextual_operation: entry
                .contract
                .contextual_operation
                .map(|operation| abi_contextual_operation(&layout, entry, operation))
                .transpose()?,
        });
        declarations.push(CheckedDeclaration {
            id: declaration,
            scope_id: LexicalScopeId(0),
            name: entry.name.clone(),
            kind: match entry.contract.kind {
                CheckedCallableKind::Builtin => CheckedDeclarationKind::Builtin,
                CheckedCallableKind::External => CheckedDeclarationKind::External,
                CheckedCallableKind::User => {
                    return Err(OwnerCompatibilityAssemblyError::new(
                        "construction ABI emitted a user callable",
                    ));
                }
            },
            flow_type: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Function {
                    args: parameters
                        .iter()
                        .filter(|parameter| parameter.kind == CheckedParameterKind::Value)
                        .map(|parameter| parameter.flow_type.ty.clone())
                        .collect(),
                    result: Box::new(entry.contract.result.clone()),
                },
            },
            value: None,
            body_scope: None,
            span: CheckedSpan::default(),
        });
        declarations.extend(parameters.into_iter().map(|parameter| CheckedDeclaration {
            id: parameter.decl_id,
            scope_id: LexicalScopeId(0),
            name: parameter.name,
            kind: match parameter.kind {
                CheckedParameterKind::Value => CheckedDeclarationKind::ValueParameter,
                CheckedParameterKind::Out => CheckedDeclarationKind::OutParameter,
            },
            flow_type: parameter.flow_type,
            value: None,
            body_scope: None,
            span: CheckedSpan::default(),
        }));
    }
    for entry in &layout.abi_values {
        declarations.push(CheckedDeclaration {
            id: layout.abi_declarations[&(
                entry.path.clone(),
                entry.key,
                OwnerAbiMemberRef::Declaration,
            )],
            scope_id: LexicalScopeId(0),
            name: entry.path.clone(),
            kind: CheckedDeclarationKind::External,
            flow_type: entry.contract.flow_type.clone(),
            value: None,
            body_scope: None,
            span: CheckedSpan::default(),
        });
    }
    trace_compat_phase(trace, "abi-rows", &mut phase_started, declarations.len());

    let mut statements = std::iter::repeat_with(|| None)
        .take(project.statement_count())
        .collect::<Vec<Option<CheckedStatement>>>();
    let mut expressions = std::iter::repeat_with(|| None)
        .take(project.expression_count())
        .collect::<Vec<Option<CheckedExpression>>>();
    let call_count = layout
        .shards
        .values()
        .map(|shard| shard.rows().calls.len())
        .sum();
    let mut calls = std::iter::repeat_with(|| None)
        .take(call_count)
        .collect::<Vec<Option<CheckedCall>>>();
    let source_count = layout
        .shards
        .values()
        .map(|shard| shard.rows().sources.len())
        .sum();
    let state_count = layout
        .shards
        .values()
        .map(|shard| shard.rows().states.len())
        .sum();
    let list_count = layout
        .shards
        .values()
        .map(|shard| shard.rows().lists.len())
        .sum();
    let mut sources = std::iter::repeat_with(|| None)
        .take(source_count)
        .collect::<Vec<Option<CheckedSource>>>();
    let mut states = std::iter::repeat_with(|| None)
        .take(state_count)
        .collect::<Vec<Option<CheckedState>>>();
    let mut lists = std::iter::repeat_with(|| None)
        .take(list_count)
        .collect::<Vec<Option<CheckedList>>>();
    let mut context_formals = Vec::new();
    let mut call_result_paths = Vec::new();
    let mut pattern_bindings = Vec::new();
    let mut occurrences = Vec::new();
    let mut diagnostics = diagnostics_aggregate.diagnostics().to_vec();

    for (owner, shard) in &layout.shards {
        let dense = layout.owner_layout(owner)?;
        for row in &shard.rows().scopes {
            scopes.push(CheckedScope {
                id: layout.local_scope(owner, row.id)?,
                parent: row
                    .parent
                    .as_ref()
                    .map(|parent| layout.scope(owner, parent))
                    .transpose()?,
                owner: row
                    .owner
                    .as_ref()
                    .map(|declaration| layout.declaration(owner, declaration))
                    .transpose()?,
                kind: row.kind,
                span: row
                    .source
                    .as_ref()
                    .map(|source| layout.source_span(owner, source))
                    .transpose()?
                    .unwrap_or_default(),
            });
        }
        for row in &shard.rows().declarations {
            declarations.push(CheckedDeclaration {
                id: layout.local_declaration(owner, row.id)?,
                scope_id: layout.scope(owner, &row.scope)?,
                name: row.name.clone(),
                kind: row.kind,
                flow_type: row.flow_type.clone(),
                value: row
                    .value
                    .as_ref()
                    .map(|value| layout.expression(owner, value))
                    .transpose()?,
                body_scope: row
                    .body_scope
                    .map(|scope| layout.local_scope(owner, scope))
                    .transpose()?,
                span: layout.source_span(owner, &row.source)?,
            });
        }
        for row in &shard.rows().statements {
            let id = layout.local_statement(owner, row.id)?;
            insert_dense(
                &mut statements,
                id.0 as usize,
                CheckedStatement {
                    id,
                    scope_id: layout.scope(owner, &row.scope)?,
                    kind: layout.statement_kind(owner, &row.kind)?,
                    resources: row
                        .resources
                        .iter()
                        .map(|resource| layout.resource(owner, resource))
                        .collect::<Result<Vec<_>, _>>()?,
                    value: row
                        .value
                        .as_ref()
                        .map(|value| layout.expression(owner, value))
                        .transpose()?,
                    value_use: row.value_use,
                    children: row
                        .children
                        .iter()
                        .map(|child| layout.statement_child(owner, child))
                        .collect::<Result<Vec<_>, _>>()?,
                    span: layout.source_span(owner, &row.source)?,
                },
                "statement",
            )?;
        }
        for row in &shard.rows().expressions {
            let id = layout.local_expression(owner, row.id)?;
            insert_dense(
                &mut expressions,
                id.0 as usize,
                CheckedExpression {
                    id,
                    scope_id: layout.scope(owner, &row.scope)?,
                    declaration: row
                        .declaration
                        .as_ref()
                        .map(|declaration| layout.declaration(owner, declaration))
                        .transpose()?,
                    flow_type: row.flow_type.clone(),
                    flush_type: row.flush_type.clone(),
                    effect: row.effect,
                    kind: layout.expression_kind(owner, &row.kind)?,
                    span: layout.source_span(owner, &row.source)?,
                },
                "expression",
            )?;
        }
        for row in &shard.rows().callables {
            callables.push(CheckedCallableSignature {
                decl_id: layout.local_declaration(owner, row.declaration)?,
                scope_id: layout.scope(owner, &row.scope)?,
                kind: row.kind,
                name: row.name.clone(),
                intrinsic: row.intrinsic,
                external_identity: row.external_identity,
                parameters: row
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let span = layout.source_span(owner, &parameter.source)?;
                        Ok(CheckedParameter {
                            decl_id: layout.local_declaration(owner, parameter.declaration)?,
                            name: parameter.name.clone(),
                            kind: parameter.kind,
                            ordinal: parameter.ordinal as usize,
                            flow_type: parameter.flow_type.clone(),
                            requirement: parameter.requirement.clone(),
                            evaluation_scope: layout
                                .evaluation_scope(owner, &parameter.evaluation_scope)?,
                            start: span.start,
                            end: span.end,
                        })
                    })
                    .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
                contexts: row
                    .contexts
                    .iter()
                    .map(|context| {
                        Ok(CheckedCallableContext {
                            name: context.name.clone(),
                            kind: context.kind,
                            provider: layout.declaration(owner, &context.provider)?,
                            flow_type: context.flow_type.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
                context_formal: row
                    .context_formal
                    .map(|formal| layout.local_context_formal(owner, formal))
                    .transpose()?,
                result: row.result.clone(),
                role: row.role,
                effect: row.effect,
                body: row
                    .body
                    .map(|body| layout.local_statement(owner, body))
                    .transpose()?,
                result_expression: row
                    .result_expression
                    .as_ref()
                    .map(|expression| layout.expression(owner, expression))
                    .transpose()?,
                contextual_operation: row
                    .contextual_operation
                    .as_ref()
                    .map(|operation| layout.contextual_operation(owner, operation))
                    .transpose()?,
            });
        }
        for row in &shard.rows().context_formals {
            context_formals.push(CheckedContextFormal {
                id: layout.local_context_formal(owner, row.id)?,
                callable: layout.local_declaration(owner, row.callable)?,
                scheme: CheckedContextScheme {
                    flow_type: row.flow_type.clone(),
                    projections: row.projections.clone(),
                },
            });
        }
        for row in &shard.rows().calls {
            let id = layout.local_call(owner, row.id)?;
            insert_dense(
                &mut calls,
                id.0 as usize,
                CheckedCall {
                    id,
                    expression: layout.local_expression(owner, row.expression)?,
                    callable: layout.declaration(owner, &row.callable)?,
                    owner_callable: row
                        .owner_callable
                        .as_ref()
                        .map(|callable| layout.declaration(owner, callable))
                        .transpose()?,
                    function: row.function.clone(),
                    intrinsic: row.intrinsic,
                    entries: row
                        .entries
                        .iter()
                        .map(|entry| layout.call_entry(owner, entry))
                        .collect::<Result<Vec<_>, _>>()?,
                    contexts: row
                        .contexts
                        .iter()
                        .map(|context| {
                            Ok(CheckedCallContext {
                                declaration: layout
                                    .local_declaration(owner, context.declaration)?,
                                signature: context.context_ordinal as usize,
                                scope_id: layout.local_scope(owner, context.scope_id)?,
                            })
                        })
                        .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
                    context_binding: layout.context_binding(owner, &row.context_binding)?,
                    contextual_substitutions: row
                        .contextual_substitutions
                        .iter()
                        .map(|substitution| {
                            Ok(CheckedContextTypeSubstitution {
                                formal: layout.context_formal(owner, &substitution.formal)?,
                                variable: substitution.variable,
                                value: substitution.value.clone(),
                            })
                        })
                        .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
                    type_substitutions: row
                        .type_substitutions
                        .iter()
                        .map(|substitution| CheckedTypeSubstitution {
                            variable: substitution.variable,
                            value: substitution.value.clone(),
                        })
                        .collect(),
                    syntax_discriminated_result: row.syntax_discriminated_result,
                    result: row.result.clone(),
                    role: row.role,
                    span: layout.source_span(owner, &row.source)?,
                },
                "call",
            )?;
        }
        call_result_paths.extend(
            shard
                .rows()
                .call_result_paths
                .iter()
                .map(|row| {
                    Ok(CheckedCallResultPath {
                        call: layout.local_call(owner, row.call)?,
                        path: CheckedSemanticPath {
                            anchor: layout.declaration(owner, &row.anchor)?,
                            projection: row.projection.clone(),
                        },
                    })
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
        );
        pattern_bindings.extend(
            shard
                .rows()
                .pattern_bindings
                .iter()
                .map(|row| {
                    Ok(CheckedPatternBinding {
                        declaration: layout.local_declaration(owner, row.declaration)?,
                        selector: layout.expression(owner, &row.selector)?,
                        projection: row.projection.clone(),
                    })
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
        );
        for row in &shard.rows().sources {
            let id = layout.local_source(owner, row.id)?;
            insert_dense(
                &mut sources,
                id.0 as usize,
                CheckedSource {
                    id,
                    declaration: layout.declaration(owner, &row.declaration)?,
                    statement: layout.local_statement(owner, row.statement)?,
                    expression: layout.local_expression(owner, row.expression)?,
                    owner_scope: layout.scope(owner, &row.owner_scope)?,
                    path: layout.semantic_path(owner, &row.path)?,
                    interval_ms: row.interval_ms,
                    payload_type: row.payload_type.clone(),
                    span: layout.source_span(owner, &row.source)?,
                },
                "source",
            )?;
        }
        for row in &shard.rows().states {
            let id = layout.local_state(owner, row.id)?;
            insert_dense(
                &mut states,
                id.0 as usize,
                CheckedState {
                    id,
                    declaration: layout.declaration(owner, &row.declaration)?,
                    statement: layout.local_statement(owner, row.statement)?,
                    expression: layout.local_expression(owner, row.expression)?,
                    initial: layout.expression(owner, &row.initial)?,
                    owner_scope: layout.scope(owner, &row.owner_scope)?,
                    path: layout.semantic_path(owner, &row.path)?,
                    kind: row.kind,
                    flow_type: row.flow_type.clone(),
                    span: layout.source_span(owner, &row.source)?,
                },
                "state",
            )?;
        }
        for row in &shard.rows().lists {
            let id = layout.local_list(owner, row.id)?;
            insert_dense(
                &mut lists,
                id.0 as usize,
                CheckedList {
                    id,
                    declaration: layout.declaration(owner, &row.declaration)?,
                    statement: layout.local_statement(owner, row.statement)?,
                    producer: layout.local_expression(owner, row.producer)?,
                    owner_scope: layout.scope(owner, &row.owner_scope)?,
                    path: layout.semantic_path(owner, &row.path)?,
                    item_type: row.item_type.clone(),
                    capacity: row.capacity,
                    key_policy: row.key_policy,
                    span: layout.source_span(owner, &row.source)?,
                },
                "list",
            )?;
        }
        occurrences.extend(
            shard
                .rows()
                .occurrences
                .iter()
                .map(|row| {
                    Ok(SemanticOccurrence {
                        target: layout.declaration(owner, &row.target)?,
                        kind: row.kind,
                        span: layout.source_span(owner, &row.source)?,
                    })
                })
                .collect::<Result<Vec<_>, OwnerCompatibilityAssemblyError>>()?,
        );
        debug_assert_eq!(dense.statements.len(), shard.rows().statements.len());
    }
    trace_compat_phase(
        trace,
        "owner-row-relocation",
        &mut phase_started,
        layout.shards.len(),
    );

    scopes.sort_by_key(|scope| scope.id);
    declarations.sort_by_key(|declaration| declaration.id);
    callables.sort_by_key(|callable| callable.decl_id);
    context_formals.sort_by_key(|formal| formal.id);
    call_result_paths.sort();
    pattern_bindings.sort_by_key(|binding| (binding.declaration, binding.selector));
    occurrences.sort_by_key(|occurrence| (occurrence.span.start, occurrence.span.end));
    for (index, declaration) in declarations.iter().enumerate() {
        let expected = checked_u32(index + 1, "declaration coverage")?;
        if declaration.id != DeclId(expected) {
            return Err(OwnerCompatibilityAssemblyError::new(format!(
                "checked owner declaration coverage skips dense identity {expected}"
            )));
        }
    }
    for (index, scope) in scopes.iter().enumerate() {
        let expected = checked_u32(index, "scope coverage")?;
        if scope.id != LexicalScopeId(expected) {
            return Err(OwnerCompatibilityAssemblyError::new(format!(
                "checked owner scope coverage skips dense identity {expected}"
            )));
        }
    }

    let statements = finish_dense(statements, "statement")?;
    let syntax = crate::TypecheckSyntaxProgram::UnitNative(project.clone());
    for (slot, row) in expressions.iter_mut().enumerate() {
        if row.is_some() {
            continue;
        }
        let syntax_id = project.expression_id_for_slot(slot).ok_or_else(|| {
            OwnerCompatibilityAssemblyError::new("unowned parser expression slot is not reversible")
        })?;
        let expression = syntax.expressions().get(syntax_id).ok_or_else(|| {
            OwnerCompatibilityAssemblyError::new(
                "unowned parser expression has no syntax arena row",
            )
        })?;
        *row = Some(CheckedExpression {
            id: CheckedExprId(checked_u32(slot, "unowned parser expression")?),
            scope_id: LexicalScopeId(0),
            declaration: None,
            flow_type: FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Unknown,
            },
            flush_type: None,
            effect: CheckedEffectSummary::default(),
            kind: CheckedExpressionKind::Invalid {
                tokens: vec!["unowned_parser_expression".to_owned()],
            },
            span: syntax.checked_expr_span(expression),
        });
    }
    let expressions = finish_dense(expressions, "expression")?;
    let calls = finish_dense(calls, "call")?;
    let mut sources = finish_dense(sources, "source")?;
    let states = finish_dense(states, "state")?;
    let lists = finish_dense(lists, "list")?;
    trace_compat_phase(
        trace,
        "dense-finalization",
        &mut phase_started,
        expressions.len(),
    );
    let mut fields = CheckedProgramFields {
        source_bundle_digest_v1: project.source_bundle_digest_v1(),
        role,
        external_types,
        lowering_metadata: CheckedProgramLoweringMetadata::default(),
        root_scope: LexicalScopeId(0),
        scopes,
        declarations,
        statements,
        expressions,
        callables,
        context_formals,
        calls,
        call_result_paths,
        order_chains: Vec::new(),
        pattern_bindings,
        resource_projection_requirements: Vec::new(),
        sources: Vec::new(),
        states,
        lists,
        occurrences,
    };
    fields.resource_projection_requirements = crate::checked_resource_projection_requirements(
        &fields.declarations,
        &fields.callables,
        &fields.calls,
        &fields.expressions,
        &sources,
    );
    trace_compat_phase(
        trace,
        "resource-projection-requirements",
        &mut phase_started,
        fields.resource_projection_requirements.len(),
    );
    crate::refine_checked_source_payload_types_from_requirements(
        &mut sources,
        &fields.resource_projection_requirements,
    );
    fields.sources = sources;
    trace_compat_phase(
        trace,
        "source-payload-refinement",
        &mut phase_started,
        fields.sources.len(),
    );
    fields.order_chains = owner_order_chains(project_diagnostic_facts, &layout)?;
    trace_compat_phase(
        trace,
        "order-chains",
        &mut phase_started,
        fields.order_chains.len(),
    );

    let expr_type_table = ExprTypeTable {
        entries: fields
            .expressions
            .iter()
            .map(|expression| ExprTypeEntry {
                expr_id: expression.id.0 as usize,
                flow_type: expression.flow_type.clone(),
            })
            .collect(),
    };
    let unknown_type_count = expr_type_table
        .entries
        .iter()
        .filter(|entry| matches!(entry.flow_type.ty, Type::Unknown))
        .count();
    let mut unresolved = BTreeSet::new();
    for entry in &expr_type_table.entries {
        crate::collect_type_vars(&entry.flow_type.ty, &mut unresolved);
    }
    trace_compat_phase(
        trace,
        "expression-type-table",
        &mut phase_started,
        expr_type_table.entries.len(),
    );
    let source_payload_shape_table = crate::checked_source_payload_shape_table(&fields);
    let function_type_table = owner_function_type_table(&fields.callables);
    trace_compat_phase(
        trace,
        "source-and-function-tables",
        &mut phase_started,
        source_payload_shape_table.len() + function_type_table.entries.len(),
    );
    let named_value_type_table = owner_named_value_type_table(&syntax, &fields)
        .map_err(OwnerCompatibilityAssemblyError::new)?;
    trace_compat_phase(
        trace,
        "named-value-table",
        &mut phase_started,
        named_value_type_table.entries.len(),
    );
    let output_root_types =
        owner_output_root_types(&syntax, project, project_diagnostic_facts, &fields)?;
    let render_slot_table =
        owner_render_slot_table(&syntax, project, project_diagnostic_facts, &fields)?;
    let host_port_table =
        owner_host_port_table(project_diagnostic_facts, &fields, &output_root_types)?;
    trace_compat_phase(
        trace,
        "output-render-host-tables",
        &mut phase_started,
        output_root_types.len(),
    );
    let lookup = crate::CheckedProgramLookup::new(&fields);
    crate::validate_structural_lowering_metadata(
        &fields,
        &lookup,
        &source_payload_shape_table,
        &function_type_table,
        &named_value_type_table,
        &output_root_types,
        &host_port_table,
    )
    .map_err(OwnerCompatibilityAssemblyError::new)?;
    trace_compat_phase(
        trace,
        "structural-validation",
        &mut phase_started,
        diagnostics.len(),
    );
    canonicalize_owner_diagnostics(&mut diagnostics);
    fields.lowering_metadata = CheckedProgramLoweringMetadata {
        source_units: project
            .source_layouts()
            .iter()
            .map(|unit| CheckedSourceUnitMetadata {
                path: unit.path.clone(),
                module: unit.module.clone(),
                start_line: unit.start_line,
                line_count: unit.line_count,
            })
            .collect(),
        original_source_expression_count: project.expression_count(),
        source_payload_shape_table,
        host_port_table,
        output_root_types,
        expr_type_table,
        function_type_table,
        named_value_type_table,
        render_slot_table,
        checked_expression_count: fields.expressions.len(),
        dynamic_fallback_count: unknown_type_count + unresolved.len(),
        diagnostics: diagnostics.clone(),
    };
    trace_compat_phase(
        trace,
        "metadata-publication",
        &mut phase_started,
        diagnostics.len(),
    );
    Ok(CheckedOwnerProjectAssembly {
        fields,
        diagnostics,
        fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_resolution_reconciliation_is_exact() {
        assert!(reconcile_owner_host_port_resolution(None, Ok(HostPortTable::default())).is_ok());
        assert!(
            reconcile_owner_host_port_resolution(Some("missing"), Err("missing".to_owned()))
                .is_ok()
        );
        assert!(
            reconcile_owner_host_port_resolution(Some("missing"), Err("different".to_owned()))
                .is_err()
        );
        assert!(
            reconcile_owner_host_port_resolution(Some("missing"), Ok(HostPortTable::default()))
                .is_err()
        );
        assert!(reconcile_owner_host_port_resolution(None, Err("missing".to_owned())).is_err());
    }
}
