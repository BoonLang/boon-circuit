//! Bounded differential projection from parser owner views into the dense kernel.
//!
//! This module is test-only. It deliberately bypasses legacy owner syntax,
//! lexical, constraint-seed, interface, and body artifacts; production cutover
//! will reuse the compact projection only after complete SCC parity.

use boon_checked::{FlowMode, FlowType, ObjectShape, Type, Variant, type_is_recursively_closed};
use boon_compiler_kernel::{
    KernelCollectionKind, KernelCompileWork, KernelExpressionId, KernelExternalExpression,
    KernelExternalTarget, KernelInheritedFormal, KernelOwnerEdgeRole, KernelOwnerId,
    KernelOwnerInputEdge, KernelOwnerNode, KernelOwnerNodeKind, KernelOwnerProgramInput,
    KernelPattern, KernelProjectProgramInput, KernelPureBuiltinKind, KernelRenderConstructorKind,
    KernelSolveWork, compile_project_program, is_kernel_host_effect,
};
use boon_parser::{ProjectSyntaxSnapshot, UnitOwnerSyntaxView};
use boon_syntax::{
    AstCallArgKind, AstExprKind, AstMatchPattern, AstParameterKind, AstStatementKind,
    AstTextSegment, StableCheckOwnerKey, StableExpressionKey, StableItemRouteSegment,
    StableStatementKind, UnitItemKind, UnitLocalStatementId,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerOracleEntry {
    pub owner: StableCheckOwnerKey,
    pub result_expression: Option<StableExpressionKey>,
    pub result: FlowType,
    pub expressions: Box<[(StableExpressionKey, FlowType)]>,
    pub public_child_owner_fields: Box<[(String, StableCheckOwnerKey)]>,
    pub public_child_kernel_fields: Box<[(String, FlowType)]>,
    pub exported_as_public_child: bool,
    pub generic_formal_reads: Box<[StableExpressionKey]>,
    pub structured_delimiter_dependents: Box<[StableExpressionKey]>,
    pub record_spread_dependents: Box<[StableExpressionKey]>,
    pub generic_selector_dependents: Box<[StableExpressionKey]>,
    pub detached_generic_reads: Box<[StableExpressionKey]>,
    pub legacy_no_element_dependents: Box<[StableExpressionKey]>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelOwnerOracleReport {
    pub supported: Box<[KernelOwnerOracleEntry]>,
    pub unsupported: Box<[(StableCheckOwnerKey, String)]>,
    pub root_blockers: Box<[KernelOwnerBlockerImpact]>,
    pub work: KernelSolveWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerBlockerImpact {
    pub owner: StableCheckOwnerKey,
    pub reason: String,
    pub affected_owners: usize,
}

/// Directional phase timings for the test-only dense-kernel bridge.
///
/// These values are deliberately kept out of [`KernelOwnerOracleReport`] so
/// semantic/determinism comparisons never include wall time. They are edit-loop
/// observations, not compiler-performance acceptance evidence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelOwnerOracleTimings {
    pub total_us: u64,
    pub owner_projection_us: u64,
    pub direct_projection_us: u64,
    pub dependency_pruning_us: u64,
    pub program_compile_us: u64,
    pub solve_us: u64,
    pub artifact_projection_us: u64,
    pub input_owners: usize,
    pub projected_owners: usize,
    pub solved_owners: usize,
    pub unsupported_owners: usize,
    pub compile_work: KernelCompileWork,
}

pub fn kernel_owner_oracle(project: &ProjectSyntaxSnapshot) -> KernelOwnerOracleReport {
    kernel_owner_oracle_with_source_payloads(project, &BTreeMap::new())
}

/// Runs the dense kernel over the largest closed owner subgraph supported by
/// the current migration slice.
///
/// SOURCE payloads are explicit ABI inputs rather than answers copied from
/// the legacy owner solver. All structural and cross-owner results are
/// recomputed inside one dense component.
pub fn kernel_owner_oracle_with_source_payloads(
    project: &ProjectSyntaxSnapshot,
    source_payloads: &BTreeMap<String, Type>,
) -> KernelOwnerOracleReport {
    profile_kernel_owner_oracle_with_source_payloads(project, source_payloads).0
}

/// Profile the compatibility projection, dense compilation, and dense solve
/// independently. Production compilation never calls this bridge.
pub fn profile_kernel_owner_oracle_with_source_payloads(
    project: &ProjectSyntaxSnapshot,
    source_payloads: &BTreeMap<String, Type>,
) -> (KernelOwnerOracleReport, KernelOwnerOracleTimings) {
    let total_started = Instant::now();
    let owner_order = project.stable_check_owner_keys().collect::<Vec<_>>();
    let input_owners = owner_order.len();
    let callable_surfaces = project_callable_surfaces(project);
    let value_surfaces = project_value_surfaces(project);
    let owner_projection_started = Instant::now();
    let mut direct_projection_elapsed = Duration::ZERO;
    let mut prepared = Vec::<PreparedOwner>::new();
    let mut unsupported = BTreeMap::<StableCheckOwnerKey, String>::new();
    for owner in &owner_order {
        let outcome = (|| {
            let direct_projection_started = Instant::now();
            let compact = compact_owner_view(
                project
                    .owner_view(owner)
                    .ok_or_else(|| "owner has no syntax view".to_owned())?,
                source_payloads,
                &callable_surfaces,
                &value_surfaces,
            );
            direct_projection_elapsed += direct_projection_started.elapsed();
            compact
        })();
        match outcome {
            Ok(owner) => prepared.push(owner),
            Err(reason) => {
                unsupported.insert(owner.clone(), reason);
            }
        }
    }
    let mut root_blocker_by_owner = unsupported
        .keys()
        .cloned()
        .map(|owner| (owner.clone(), owner))
        .collect::<BTreeMap<_, _>>();
    let owner_projection_us = elapsed_us(owner_projection_started.elapsed());

    let dependency_pruning_started = Instant::now();
    let prepared_by_owner = prepared
        .iter()
        .enumerate()
        .map(|(index, owner)| (owner.owner.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut active = (0..prepared.len()).collect::<BTreeSet<_>>();
    loop {
        let rejected = active
            .iter()
            .filter_map(|index| {
                let owner = &prepared[*index];
                let external_reason = owner.external_expressions.iter().find_map(|external| {
                    let Some(target) = prepared_by_owner.get(&external.owner).copied() else {
                        return Some((
                            format!("depends on unsupported owner {:#?}", external.owner),
                            external.owner.clone(),
                        ));
                    };
                    if !active.contains(&target) {
                        return Some((
                            format!("depends on unsupported owner {:#?}", external.owner),
                            root_blocker_by_owner
                                .get(&external.owner)
                                .cloned()
                                .unwrap_or_else(|| external.owner.clone()),
                        ));
                    }
                    match &external.target {
                        PreparedExternalTarget::Result => None,
                        PreparedExternalTarget::Expression(expression) => (!prepared[target]
                            .expressions
                            .iter()
                            .any(|candidate| candidate == expression))
                        .then(|| {
                            (
                                format!(
                                    "imports missing expression {expression:#?} from owner {:#?}",
                                    external.owner
                                ),
                                owner.owner.clone(),
                            )
                        }),
                    }
                });
                let call_reason = owner.call_targets.iter().find_map(|call| {
                    let Some(target) = prepared_by_owner.get(&call.owner).copied() else {
                        return Some((
                            format!("depends on unsupported owner {:#?}", call.owner),
                            call.owner.clone(),
                        ));
                    };
                    (!active.contains(&target)).then(|| {
                        (
                            format!("depends on unsupported owner {:#?}", call.owner),
                            root_blocker_by_owner
                                .get(&call.owner)
                                .cloned()
                                .unwrap_or_else(|| call.owner.clone()),
                        )
                    })
                });
                external_reason
                    .or(call_reason)
                    .map(|(reason, root)| (*index, reason, root))
            })
            .collect::<Vec<_>>();
        if rejected.is_empty() {
            break;
        }
        for (index, reason, root) in rejected {
            active.remove(&index);
            root_blocker_by_owner.insert(prepared[index].owner.clone(), root);
            unsupported.insert(prepared[index].owner.clone(), reason);
        }
    }

    let active = active.into_iter().collect::<Vec<_>>();
    let dependency_pruning_us = elapsed_us(dependency_pruning_started.elapsed());
    let mut dense_owner = vec![None; prepared.len()];
    for (dense, prepared_index) in active.iter().copied().enumerate() {
        dense_owner[prepared_index] = Some(KernelOwnerId(
            u32::try_from(dense).expect("kernel oracle owner count exceeds u32"),
        ));
    }
    if let Some(dense) = std::env::var_os("BOON_KERNEL_ORACLE_TRACE_DENSE_OWNER")
        .and_then(|dense| dense.to_string_lossy().parse::<usize>().ok())
        && let Some(prepared_index) = active.get(dense).copied()
    {
        let compact = &prepared[prepared_index].compact;
        eprintln!(
            "kernel-owner-trace dense_owner={dense} stable_owner={:#?} formals={} result={} nodes={}",
            prepared[prepared_index].owner,
            compact.formal_count,
            compact.result.0,
            compact.nodes.len(),
        );
        for (expression, node) in compact.nodes.iter().enumerate() {
            eprintln!(
                "kernel-owner-node expression={expression} mode={:?} kind={:?} inputs={:?}",
                node.mode, node.kind, node.inputs,
            );
        }
        for call in &prepared[prepared_index].call_targets {
            let target = prepared_by_owner[&call.owner];
            eprintln!(
                "kernel-owner-call expression={} dense_target={} stable_target={:#?}",
                call.node,
                dense_owner[target]
                    .expect("active traced call target has a dense owner")
                    .0,
                call.owner,
            );
        }
    }
    let project_input = KernelProjectProgramInput {
        owners: active
            .iter()
            .map(|prepared_index| {
                let owner = &prepared[*prepared_index];
                let mut compact = owner.compact.clone();
                compact.external_expressions = owner
                    .external_expressions
                    .iter()
                    .map(|external| {
                        let target = prepared_by_owner[&external.owner];
                        let kernel_target = match &external.target {
                            PreparedExternalTarget::Result => KernelExternalTarget::Result,
                            PreparedExternalTarget::Expression(expression) => {
                                let expression = prepared[target]
                                    .expressions
                                    .iter()
                                    .position(|candidate| candidate == expression)
                                    .expect("active external expression was validated");
                                KernelExternalTarget::Expression(KernelExpressionId(
                                    u32::try_from(expression)
                                        .expect("kernel owner expression count exceeds u32"),
                                ))
                            }
                        };
                        KernelExternalExpression {
                            owner: dense_owner[target].expect("active target has a dense owner"),
                            target: kernel_target,
                        }
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                for call in &owner.call_targets {
                    let target = prepared_by_owner[&call.owner];
                    let node = compact
                        .nodes
                        .get_mut(call.node)
                        .expect("prepared user call node is local");
                    let KernelOwnerNodeKind::UserCall {
                        target: call_target,
                        ..
                    } = &mut node.kind
                    else {
                        panic!("prepared user call target references a non-call node")
                    };
                    *call_target =
                        dense_owner[target].expect("active call target has a dense owner");
                }
                compact
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    };

    let mut program_compile_us = 0;
    let mut solve_us = 0;
    let mut compile_work = KernelCompileWork::default();
    let artifact = if project_input.owners.is_empty() {
        None
    } else {
        let compile_started = Instant::now();
        let compiled = compile_project_program(&project_input).map_err(|error| error.to_string());
        if let Ok(program) = &compiled {
            compile_work = program.compile_work();
        }
        program_compile_us = elapsed_us(compile_started.elapsed());
        let solved = compiled.and_then(|program| {
            let solve_started = Instant::now();
            let solved = program.solve().map_err(|error| error.to_string());
            solve_us = elapsed_us(solve_started.elapsed());
            solved
        });
        match solved {
            Ok(artifact) => Some(artifact),
            Err(reason) => {
                for prepared_index in &active {
                    root_blocker_by_owner.insert(
                        prepared[*prepared_index].owner.clone(),
                        prepared[*prepared_index].owner.clone(),
                    );
                    unsupported.insert(
                        prepared[*prepared_index].owner.clone(),
                        format!("kernel project solve failed: {reason}"),
                    );
                }
                None
            }
        }
    };

    let artifact_projection_started = Instant::now();
    let (supported, work) = artifact.map_or_else(
        || (Vec::new(), KernelSolveWork::default()),
        |artifact| {
            let work = artifact.work;
            let definitions = artifact.definitions;
            let result_by_owner = active
                .iter()
                .zip(&definitions)
                .map(|(prepared_index, artifact)| {
                    (prepared[*prepared_index].owner.clone(), artifact.result.clone())
                })
                .collect::<BTreeMap<_, _>>();
            let exported_public_children = prepared
                .iter()
                .flat_map(|owner| {
                    owner
                        .public_child_owner_fields
                        .iter()
                        .map(|(_, child)| child.clone())
                })
                .collect::<BTreeSet<_>>();
            let supported = active
                .iter()
                .zip(definitions)
                .map(|(prepared_index, artifact)| {
                    let owner = &prepared[*prepared_index];
                    let expressions = owner
                        .expressions
                        .iter()
                        .zip(artifact.expressions)
                        .map(|(source, flow_type)| (source.clone(), flow_type))
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    KernelOwnerOracleEntry {
                        owner: owner.owner.clone(),
                        result_expression: owner.result_expression.clone(),
                        result: artifact.result,
                        expressions,
                        public_child_owner_fields: owner.public_child_owner_fields.clone(),
                        public_child_kernel_fields: owner
                            .public_child_owner_fields
                            .iter()
                            .map(|(name, child)| {
                                (
                                    name.clone(),
                                    result_by_owner
                                        .get(child)
                                        .unwrap_or_else(|| {
                                            panic!(
                                                "active public child {child:#?} has no kernel result"
                                            )
                                        })
                                        .clone(),
                                )
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        exported_as_public_child: exported_public_children.contains(&owner.owner),
                        generic_formal_reads: owner.generic_formal_reads.clone(),
                        structured_delimiter_dependents: owner
                            .structured_delimiter_dependents
                            .clone(),
                        record_spread_dependents: owner.record_spread_dependents.clone(),
                        generic_selector_dependents: owner.generic_selector_dependents.clone(),
                        detached_generic_reads: owner.detached_generic_reads.clone(),
                        legacy_no_element_dependents: owner.legacy_no_element_dependents.clone(),
                    }
                })
                .collect::<Vec<_>>();
            (supported, work)
        },
    );
    let mut blocker_counts = BTreeMap::<StableCheckOwnerKey, usize>::new();
    for owner in unsupported.keys() {
        let root = root_blocker_by_owner
            .get(owner)
            .cloned()
            .unwrap_or_else(|| owner.clone());
        *blocker_counts.entry(root).or_default() += 1;
    }
    let mut root_blockers = blocker_counts
        .into_iter()
        .map(|(owner, affected_owners)| KernelOwnerBlockerImpact {
            reason: unsupported
                .get(&owner)
                .cloned()
                .unwrap_or_else(|| "unsupported dependency root".to_owned()),
            owner,
            affected_owners,
        })
        .collect::<Vec<_>>();
    root_blockers.sort_by(|left, right| {
        right
            .affected_owners
            .cmp(&left.affected_owners)
            .then_with(|| left.owner.cmp(&right.owner))
    });
    let unsupported = owner_order
        .into_iter()
        .filter_map(|owner| unsupported.remove(&owner).map(|reason| (owner, reason)))
        .collect::<Vec<_>>();
    let report = KernelOwnerOracleReport {
        supported: supported.into_boxed_slice(),
        unsupported: unsupported.into_boxed_slice(),
        root_blockers: root_blockers.into_boxed_slice(),
        work,
    };
    let artifact_projection_us = elapsed_us(artifact_projection_started.elapsed());
    let timings = KernelOwnerOracleTimings {
        total_us: elapsed_us(total_started.elapsed()),
        owner_projection_us,
        direct_projection_us: elapsed_us(direct_projection_elapsed),
        dependency_pruning_us,
        program_compile_us,
        solve_us,
        artifact_projection_us,
        input_owners,
        projected_owners: prepared.len(),
        solved_owners: report.supported.len(),
        unsupported_owners: report.unsupported.len(),
        compile_work,
    };
    (report, timings)
}

fn elapsed_us(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX)
}

struct PreparedOwner {
    owner: StableCheckOwnerKey,
    expressions: Box<[StableExpressionKey]>,
    external_expressions: Box<[PreparedExternalExpression]>,
    call_targets: Box<[PreparedCallTarget]>,
    compact: KernelOwnerProgramInput,
    result_expression: Option<StableExpressionKey>,
    public_child_owner_fields: Box<[(String, StableCheckOwnerKey)]>,
    generic_formal_reads: Box<[StableExpressionKey]>,
    structured_delimiter_dependents: Box<[StableExpressionKey]>,
    record_spread_dependents: Box<[StableExpressionKey]>,
    generic_selector_dependents: Box<[StableExpressionKey]>,
    detached_generic_reads: Box<[StableExpressionKey]>,
    legacy_no_element_dependents: Box<[StableExpressionKey]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedLexicalBinding {
    provider: PreparedLexicalProvider,
    prefix: Box<[String]>,
    directional: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreparedLexicalProvider {
    Input(PreparedInputReference),
    Known(Type),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PreparedExternalExpression {
    owner: StableCheckOwnerKey,
    target: PreparedExternalTarget,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum PreparedExternalTarget {
    Expression(StableExpressionKey),
    Result,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PreparedInputReference {
    Syntax(usize),
    OwnerResult(StableCheckOwnerKey),
}

#[derive(Clone, Debug)]
enum PreparedSyntheticResult {
    Alias(PreparedInputReference),
    Record(Vec<PreparedRecordEntry>),
}

#[derive(Clone, Debug)]
enum PreparedRecordEntry {
    Field {
        name: String,
        value: PreparedInputReference,
    },
    Spread {
        value: PreparedInputReference,
    },
}

#[derive(Clone, Debug)]
struct PreparedCallTarget {
    node: usize,
    owner: StableCheckOwnerKey,
}

#[derive(Clone, Debug)]
struct CallableSurface {
    owner: StableCheckOwnerKey,
    parameters: Box<[CallableParameter]>,
    context_ordinal: Option<usize>,
}

#[derive(Clone, Debug)]
struct CallableParameter {
    name: String,
    ordinal: usize,
    value: bool,
}

fn project_callable_surfaces(project: &ProjectSyntaxSnapshot) -> BTreeMap<String, CallableSurface> {
    let definitions = project
        .item_index()
        .definitions()
        .filter(|entry| entry.kind == UnitItemKind::Function)
        .collect::<Vec<_>>();
    let mut contexts = definitions
        .iter()
        .filter_map(|entry| {
            let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
            project
                .owner_view(&owner)
                .is_some_and(owner_uses_passed_context)
                .then_some(owner)
        })
        .collect::<BTreeSet<_>>();
    let callable_owner_by_name = definitions
        .iter()
        .flat_map(|entry| {
            let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
            entry
                .names
                .iter()
                .cloned()
                .map(move |name| (name, owner.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut callers_by_callee =
        BTreeMap::<StableCheckOwnerKey, BTreeSet<StableCheckOwnerKey>>::new();
    for entry in &definitions {
        let caller = StableCheckOwnerKey::Item(entry.owner_key.clone());
        let Some(view) = project.owner_view(&caller) else {
            continue;
        };
        for callee in view.expressions().filter_map(|expression| {
            let function = match &expression.kind {
                AstExprKind::Call { function, pass, .. }
                | AstExprKind::Pipe {
                    op: function, pass, ..
                } if pass.is_none() => function,
                _ => return None,
            };
            callable_owner_by_name.get(function).cloned()
        }) {
            callers_by_callee
                .entry(callee)
                .or_default()
                .insert(caller.clone());
        }
    }
    let mut queue = contexts.iter().cloned().collect::<VecDeque<_>>();
    while let Some(callee) = queue.pop_front() {
        for caller in callers_by_callee.get(&callee).into_iter().flatten() {
            if contexts.insert(caller.clone()) {
                queue.push_back(caller.clone());
            }
        }
    }

    definitions
        .into_iter()
        .flat_map(|entry| {
            let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
            let context_ordinal = contexts.contains(&owner).then_some(entry.parameters.len());
            entry.names.iter().cloned().map(move |name| {
                (
                    name,
                    CallableSurface {
                        owner: owner.clone(),
                        parameters: entry
                            .parameters
                            .iter()
                            .map(|parameter| CallableParameter {
                                name: parameter.name.clone(),
                                ordinal: parameter.ordinal,
                                value: parameter.kind == AstParameterKind::Value,
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        context_ordinal,
                    },
                )
            })
        })
        .collect()
}

fn owner_uses_passed_context(view: UnitOwnerSyntaxView<'_>) -> bool {
    view.expressions().any(|expression| match &expression.kind {
        AstExprKind::Identifier(name) => name == "PASSED",
        AstExprKind::Path(path) => path.first().is_some_and(|root| root == "PASSED"),
        _ => false,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValueSurface {
    owner: StableCheckOwnerKey,
    target: PreparedExternalTarget,
    lexical_scope: Box<[StableItemRouteSegment]>,
}

fn project_value_surfaces(project: &ProjectSyntaxSnapshot) -> BTreeMap<String, Vec<ValueSurface>> {
    let mut surfaces = BTreeMap::<String, Vec<ValueSurface>>::new();
    for entry in project
        .item_index()
        .owners()
        .filter(|entry| entry.kind != UnitItemKind::Function)
    {
        let owner = StableCheckOwnerKey::Item(entry.owner_key.clone());
        let Some(view) = project.owner_view(&owner) else {
            continue;
        };
        let Some((_, root_statement)) = view
            .statement_ids()
            .iter()
            .copied()
            .zip(view.statements())
            .find(|(statement, _)| {
                view.stable_statement_key_local(*statement)
                    .is_some_and(|key| {
                        key.route.statement_route.is_empty()
                            && key.route.owner.as_ref() == Some(&entry.owner_key.item_route)
                    })
            })
        else {
            continue;
        };
        let target = match root_statement.expr {
            Some(result) => {
                let Some(expression) = view.stable_expression_key_for_syntax(result) else {
                    continue;
                };
                PreparedExternalTarget::Expression(expression)
            }
            None => PreparedExternalTarget::Result,
        };
        let lexical_scope = entry.route.segments()[..entry.route.segments().len() - 1]
            .to_vec()
            .into_boxed_slice();
        let surface = ValueSurface {
            owner,
            target,
            lexical_scope,
        };
        for name in &entry.names {
            let candidates = surfaces.entry(name.clone()).or_default();
            if !candidates.iter().any(|candidate| candidate == &surface) {
                candidates.push(surface.clone());
            }
        }
    }
    surfaces
}

fn exact_value_surface<'a>(
    name: &str,
    surfaces: &'a BTreeMap<String, Vec<ValueSurface>>,
    current_owner: &StableCheckOwnerKey,
) -> Result<&'a ValueSurface, String> {
    let current_route = match current_owner {
        StableCheckOwnerKey::Item(owner) => Some(owner.item_route.segments()),
        StableCheckOwnerKey::UnitRoot(_) => None,
    };
    let mut visible = surfaces
        .get(name)
        .into_iter()
        .flatten()
        .filter_map(|surface| {
            let same_unit = surface.owner.source_unit_id() == current_owner.source_unit_id();
            let visible = if same_unit {
                current_route.is_some_and(|route| route.starts_with(&surface.lexical_scope))
            } else {
                surface.lexical_scope.is_empty()
            };
            visible.then_some(((same_unit, surface.lexical_scope.len()), surface))
        })
        .collect::<Vec<_>>();
    let best = visible.iter().map(|(rank, _)| *rank).max();
    visible.retain(|(rank, _)| Some(*rank) == best);
    match visible.as_slice() {
        [(_, surface)] => Ok(surface),
        [] => match surfaces.get(name).map(Vec::as_slice).unwrap_or_default() {
            // Boon permits an unqualified reference to one uniquely named
            // nested root value (for example `elements` for
            // `store.elements`). This is a static declaration capture, not an
            // implicit PASSED/context formal, so preserve the exact external
            // owner rather than allocating a call-frame slot.
            [surface] => Ok(surface),
            [] => Err(format!("unresolved top-level value read `{name}`")),
            candidates => Err(format!(
                "ambiguous nested value read `{name}` has {} candidates",
                candidates.len()
            )),
        },
        candidates => Err(format!(
            "ambiguous lexical value read `{name}` has {} nearest candidates",
            candidates.len()
        )),
    }
}

fn local_value_surface_provider(
    surface: &ValueSurface,
    current_owner: &StableCheckOwnerKey,
    stable_expressions: &[StableExpressionKey],
    raw_expressions: &[&boon_syntax::AstExpr],
    raw_result: Option<usize>,
) -> Result<Option<usize>, String> {
    if &surface.owner != current_owner {
        return Ok(None);
    }
    let provider = match &surface.target {
        PreparedExternalTarget::Expression(target) => stable_expressions
            .iter()
            .position(|expression| expression == target)
            .and_then(|index| raw_expressions.get(index))
            .map(|expression| expression.id)
            .ok_or_else(|| {
                format!("owner-local value surface {target:?} has no expression in its owning unit")
            })?,
        PreparedExternalTarget::Result => raw_result.ok_or_else(|| {
            "owner-local result read has no direct syntax result provider".to_owned()
        })?,
    };
    Ok(Some(provider))
}

/// Project the first dense slice straight from the parser-owned owner view.
///
/// This deliberately accepts only owners whose public value is one direct
/// statement expression. More elaborate statement sequencing is rejected at
/// this boundary until it has a compact residual representation; it is never
/// reconstructed through the legacy lexical/constraint graphs.
fn compact_owner_view(
    view: UnitOwnerSyntaxView<'_>,
    source_payloads: &BTreeMap<String, Type>,
    callable_surfaces: &BTreeMap<String, CallableSurface>,
    value_surfaces: &BTreeMap<String, Vec<ValueSurface>>,
) -> Result<PreparedOwner, String> {
    let owner = view.stable_key();
    if !matches!(owner, StableCheckOwnerKey::Item(_)) {
        return Err("owner has no public declaration".to_owned());
    }
    let StableCheckOwnerKey::Item(owner_key) = &owner else {
        unreachable!()
    };
    let owner_context_ordinal = callable_surfaces
        .values()
        .find(|surface| surface.owner == owner)
        .and_then(|surface| surface.context_ordinal);
    let (root_statement_id, root_statement) = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .find(|(statement, _)| {
            view.stable_statement_key_local(*statement)
                .is_some_and(|key| {
                    key.route.statement_route.is_empty()
                        && key.route.owner.as_ref() == Some(&owner_key.item_route)
                })
        })
        .ok_or_else(|| "owner has no public declaration".to_owned())?;
    let result_mode = match &root_statement.kind {
        AstStatementKind::Source { .. } => FlowMode::PresentOrAbsent,
        AstStatementKind::Function { .. }
        | AstStatementKind::Field { .. }
        | AstStatementKind::Hold { .. }
        | AstStatementKind::List { .. } => FlowMode::Continuous,
        AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => {
            return Err("owner has no public declaration".to_owned());
        }
    };
    let (formal_count, formal_by_name) = match &root_statement.kind {
        AstStatementKind::Function { parameters, .. } => {
            let mut by_name = BTreeMap::new();
            for parameter in parameters {
                if parameter.kind != AstParameterKind::Value {
                    return Err(
                        "OUT formals are not in the first call-composition slice".to_owned()
                    );
                }
                if parameter.ordinal >= parameters.len()
                    || by_name
                        .insert(parameter.name.clone(), parameter.ordinal)
                        .is_some()
                {
                    return Err("function formals are not a dense unique frame".to_owned());
                }
            }
            let mut formal_count = parameters.len();
            if let Some(context_ordinal) = owner_context_ordinal {
                if context_ordinal != formal_count {
                    return Err(
                        "function context ordinal is not after its value formals".to_owned()
                    );
                }
                if by_name.insert("PASSED".to_owned(), formal_count).is_some() {
                    return Err("function reserves `PASSED` for its context frame".to_owned());
                }
                formal_count = formal_count
                    .checked_add(1)
                    .ok_or_else(|| "function context frame overflows usize".to_owned())?;
            }
            (formal_count, by_name)
        }
        _ => (0, BTreeMap::<String, usize>::new()),
    };
    let statement_roots = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .filter_map(|(statement_id, statement)| {
            Some((
                statement.expr?,
                view.stable_statement_key_local(statement_id)?,
            ))
        })
        .collect::<Vec<_>>();
    let raw_expressions = view.expressions().collect::<Vec<_>>();
    let expressions = view.stable_expression_keys().collect::<Vec<_>>();
    if raw_expressions.len() != expressions.len() {
        return Err("owner expression identity table is incomplete".to_owned());
    }
    let mut local_by_syntax = BTreeMap::new();
    for (index, expression) in raw_expressions.iter().enumerate() {
        if local_by_syntax.insert(expression.id, index).is_some() {
            return Err("owner repeats a parser expression identity".to_owned());
        }
    }
    let (collection_binding_inputs, collection_bindings_by_scope) =
        direct_collection_callback_bindings(&raw_expressions)?;
    let raw_result = root_statement
        .expr
        .or_else(|| {
            if matches!(root_statement.kind, AstStatementKind::Function { .. }) {
                view.statement_body_result_expression(root_statement_id)
            } else {
                view.statement_value_expression(root_statement_id)
            }
        })
        .filter(|result| local_by_syntax.contains_key(result));
    let child_owner_result = matches!(root_statement.kind, AstStatementKind::Field { .. })
        .then(|| direct_child_owner_result(view, root_statement_id))
        .transpose()?
        .flatten();
    let synthetic_result = raw_result
        .is_none()
        .then(|| child_owner_result.clone())
        .flatten();
    if raw_result.is_none() && synthetic_result.is_none() {
        let direct_child_boundaries = view
            .child_owners()
            .iter()
            .filter(|boundary| boundary.parent() == Some(root_statement_id))
            .count();
        return Err(format!(
            "owner has no direct or structural result: root_kind={:?} root_children={} direct_child_boundaries={direct_child_boundaries}",
            root_statement.kind,
            root_statement.children.len(),
        ));
    }
    let result_index = match raw_result {
        Some(raw_result) => local_by_syntax
            .get(&raw_result)
            .copied()
            .ok_or_else(|| "owner has no local result expression".to_owned())?,
        None => raw_expressions.len(),
    };
    let result_expression = (root_statement.expr.is_some()
        || matches!(root_statement.kind, AstStatementKind::Function { .. }))
    .then(|| {
        raw_result
            .and_then(|raw_result| local_by_syntax.get(&raw_result).copied())
            .map(|index| expressions[index].clone())
    })
    .flatten();

    let source_paths = direct_view_source_payload_paths(
        &raw_expressions,
        &expressions,
        &local_by_syntax,
        &statement_roots,
    )?;
    let mut structured_records = direct_structured_statement_records(view)?;
    if let Some(container) = root_statement.expr
        && !local_by_syntax
            .get(&container)
            .and_then(|index| raw_expressions.get(*index))
            .is_some_and(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. }
                        if !fields.is_empty()
                )
            })
        && let Some(PreparedSyntheticResult::Record(fields)) = &child_owner_result
    {
        let entries = structured_records.entry(container).or_default();
        for field in fields {
            match field {
                PreparedRecordEntry::Field { name, value } => {
                    if let Some(PreparedRecordEntry::Field {
                        value: current, ..
                    }) = entries.iter_mut().find(|entry| {
                        matches!(entry, PreparedRecordEntry::Field { name: current, .. } if current == name)
                    }) {
                        *current = value.clone();
                    } else {
                        entries.push(PreparedRecordEntry::Field {
                            name: name.clone(),
                            value: value.clone(),
                        });
                    }
                }
                spread @ PreparedRecordEntry::Spread { .. } => entries.push(spread.clone()),
            }
        }
    }
    let mut child_owner_by_value = BTreeMap::<usize, StableCheckOwnerKey>::new();
    for boundary in view.child_owners() {
        let child_owner = view
            .stable_check_owner_for_local_statement(boundary.statement())
            .ok_or_else(|| "child owner boundary has no stable owner".to_owned())?;
        let values = [
            view.statement_for_local(boundary.statement())
                .and_then(|statement| statement.expr),
            view.child_owner_boundary_expression(boundary),
            view.child_owner_result_expression(boundary),
        ];
        for value in values.into_iter().flatten() {
            if let Some(previous) = child_owner_by_value.insert(value, child_owner.clone())
                && previous != child_owner
            {
                return Err(format!(
                    "child expression {value} belongs to multiple public owners: {previous:#?} and {child_owner:#?}"
                ));
            }
        }
    }
    let mut direct_child_owner_by_name = BTreeMap::<String, StableCheckOwnerKey>::new();
    if let StableCheckOwnerKey::Item(parent_owner) = &owner {
        for boundary in view.child_owners() {
            let child_owner = view
                .stable_check_owner_for_local_statement(boundary.statement())
                .ok_or_else(|| "child owner boundary has no stable owner".to_owned())?;
            let StableCheckOwnerKey::Item(child_item) = &child_owner else {
                continue;
            };
            let parent_segments = parent_owner.item_route.segments();
            let child_segments = child_item.item_route.segments();
            if child_item.source_unit_id != parent_owner.source_unit_id
                || child_segments.len() != parent_segments.len() + 1
                || !child_segments.starts_with(parent_segments)
            {
                continue;
            }
            let statement = view
                .statement_for_local(boundary.statement())
                .ok_or_else(|| "direct child owner has no parser statement".to_owned())?;
            let name = match &statement.kind {
                AstStatementKind::Field { name } => Some(name),
                AstStatementKind::Source {
                    field: Some(name), ..
                }
                | AstStatementKind::Hold {
                    field: Some(name), ..
                }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => Some(name),
                _ => None,
            };
            if let Some(name) = name
                && let Some(previous) =
                    direct_child_owner_by_name.insert(name.clone(), child_owner.clone())
                && previous != child_owner
            {
                return Err(format!(
                    "direct public field `{name}` belongs to multiple child owners: {previous:#?} and {child_owner:#?}"
                ));
            }
        }
    }
    let result_record_fields = raw_result
        .and_then(|result| structured_records.get(&result))
        .or_else(|| {
            synthetic_result.as_ref().and_then(|result| match result {
                PreparedSyntheticResult::Record(fields) => Some(fields),
                PreparedSyntheticResult::Alias(_) => None,
            })
        });
    let mut public_child_owner_fields = result_record_fields
        .into_iter()
        .flatten()
        .filter_map(|field| match field {
            PreparedRecordEntry::Field { name, value } => {
                let child_owner = direct_child_owner_by_name
                    .get(name)
                    .or_else(|| match value {
                        PreparedInputReference::OwnerResult(owner) => Some(owner),
                        PreparedInputReference::Syntax(value) => child_owner_by_value.get(value),
                    })?;
                Some((name.clone(), child_owner.clone()))
            }
            PreparedRecordEntry::Spread { .. } => None,
        })
        .collect::<Vec<_>>();
    if let Some(result) = raw_result
        && let Some(expression) = local_by_syntax
            .get(&result)
            .and_then(|index| raw_expressions.get(*index))
        && let AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } =
            &expression.kind
    {
        public_child_owner_fields.extend(fields.iter().filter_map(|field| {
            (!field.spread)
                .then(|| {
                    direct_child_owner_by_name
                        .get(&field.name)
                        .or_else(|| child_owner_by_value.get(&field.value))
                })
                .flatten()
                .map(|owner| (field.name.clone(), owner.clone()))
        }));
    }
    let mut public_field_names = BTreeSet::new();
    public_child_owner_fields.retain(|(name, _)| public_field_names.insert(name.clone()));
    let public_child_owner_fields = public_child_owner_fields.into_boxed_slice();
    let lexical_binding_reads = direct_lexical_binding_reads(
        view,
        &owner,
        &raw_expressions,
        &expressions,
        &local_by_syntax,
        &collection_bindings_by_scope,
        &structured_records,
    );
    let structured_delimiter_nodes = structured_records
        .keys()
        .filter_map(|syntax| {
            let index = local_by_syntax.get(syntax).copied()?;
            matches!(raw_expressions[index].kind, AstExprKind::Delimiter).then_some(index)
        })
        .collect::<BTreeSet<_>>();

    let mut external_by_key = BTreeMap::new();
    let mut external_expressions = Vec::new();
    let mut call_targets = Vec::new();
    let node_count = raw_expressions.len() + usize::from(synthetic_result.is_some());
    let mut nodes = Vec::with_capacity(node_count);
    for (index, expression) in raw_expressions.iter().enumerate() {
        let (kind, mut raw_edges, call_target, read_target) = if let Some(fields) =
            structured_records.get(&expression.id)
        {
            (
                match &expression.kind {
                    AstExprKind::MatchArm { pattern, .. } => KernelOwnerNodeKind::MatchArm {
                        pattern: compact_pattern(pattern),
                    },
                    _ => KernelOwnerNodeKind::Record { tag: None },
                },
                fields
                    .iter()
                    .map(|entry| match entry {
                        PreparedRecordEntry::Field { name, value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: name.clone().into_boxed_str(),
                                spread: false,
                            },
                            value.clone(),
                        ),
                        PreparedRecordEntry::Spread { value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: Box::from(""),
                                spread: true,
                            },
                            value.clone(),
                        ),
                    })
                    .collect(),
                None,
                None,
            )
        } else {
            match &expression.kind {
                AstExprKind::Identifier(_)
                    if let Some(provider) = collection_binding_inputs.get(&expression.id) =>
                {
                    (
                        KernelOwnerNodeKind::CollectionItemRead,
                        vec![(
                            KernelOwnerEdgeRole::ReadProvider,
                            PreparedInputReference::Syntax(*provider),
                        )],
                        None,
                        None,
                    )
                }
                AstExprKind::Identifier(name) => {
                    if let Some(binding) = lexical_binding_reads.get(&expression.id) {
                        let (kind, edges) = prepared_lexical_read_node(binding, &[])?;
                        (kind, edges, None, None)
                    } else if let Some(formal) = formal_by_name.get(name).copied() {
                        (
                            KernelOwnerNodeKind::FormalRead {
                                formal: checked_u32(formal, "formal ordinal")?,
                                fields: Box::new([]),
                            },
                            Vec::new(),
                            None,
                            None,
                        )
                    } else {
                        let surface = exact_value_surface(name, value_surfaces, &owner).map_err(
                            |reason| {
                                format!(
                                    "{reason} at expression {} line {} bytes {}..{}",
                                    expression.id,
                                    expression.line,
                                    expression.start,
                                    expression.end
                                )
                            },
                        )?;
                        match local_value_surface_provider(
                            surface,
                            &owner,
                            &expressions,
                            &raw_expressions,
                            raw_result,
                        )? {
                            Some(provider) => (
                                KernelOwnerNodeKind::LexicalRead {
                                    fields: Box::new([]),
                                },
                                vec![(
                                    KernelOwnerEdgeRole::ReadProvider,
                                    PreparedInputReference::Syntax(provider),
                                )],
                                None,
                                None,
                            ),
                            None => (
                                KernelOwnerNodeKind::ValueRead {
                                    fields: Box::new([]),
                                },
                                Vec::new(),
                                None,
                                Some(surface.clone()),
                            ),
                        }
                    }
                }
                AstExprKind::Path(path) => {
                    let (root, fields) = path
                        .split_first()
                        .ok_or_else(|| "value path has no root".to_owned())?;
                    let path_fields = fields
                        .iter()
                        .cloned()
                        .map(String::into_boxed_str)
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    if let Some(binding) = lexical_binding_reads.get(&expression.id) {
                        let (kind, edges) = prepared_lexical_read_node(binding, &path_fields)?;
                        (kind, edges, None, None)
                    } else if let Some(formal) = formal_by_name.get(root).copied() {
                        (
                            KernelOwnerNodeKind::FormalRead {
                                formal: checked_u32(formal, "formal ordinal")?,
                                fields: path_fields,
                            },
                            Vec::new(),
                            None,
                            None,
                        )
                    } else {
                        let surface = exact_value_surface(root, value_surfaces, &owner).map_err(
                            |reason| {
                                format!(
                                    "{reason} at expression {} line {} bytes {}..{}",
                                    expression.id,
                                    expression.line,
                                    expression.start,
                                    expression.end
                                )
                            },
                        )?;
                        match local_value_surface_provider(
                            surface,
                            &owner,
                            &expressions,
                            &raw_expressions,
                            raw_result,
                        )? {
                            Some(provider) => (
                                KernelOwnerNodeKind::LexicalRead {
                                    fields: path_fields,
                                },
                                vec![(
                                    KernelOwnerEdgeRole::ReadProvider,
                                    PreparedInputReference::Syntax(provider),
                                )],
                                None,
                                None,
                            ),
                            None => (
                                KernelOwnerNodeKind::ValueRead {
                                    fields: path_fields,
                                },
                                Vec::new(),
                                None,
                                Some(surface.clone()),
                            ),
                        }
                    }
                }
                AstExprKind::Call {
                    function,
                    args,
                    pass,
                } if render_constructor_kind(function).is_some() => {
                    if pass.is_some() {
                        return Err(format!(
                            "render constructor `{function}` cannot consume PASS in the compact ABI"
                        ));
                    }
                    (
                        KernelOwnerNodeKind::RenderConstructor {
                            kind: render_constructor_kind(function)
                                .expect("render constructor guard resolves"),
                        },
                        args.iter()
                            .map(|argument| {
                                (
                                    KernelOwnerEdgeRole::AbiArgument {
                                        name: argument.name.clone().into_boxed_str(),
                                    },
                                    PreparedInputReference::Syntax(argument.value),
                                )
                            })
                            .collect(),
                        None,
                        None,
                    )
                }
                AstExprKind::Call {
                    function,
                    args,
                    pass,
                } if pure_builtin_kind(function).is_some() => {
                    if pass.is_some() {
                        return Err(format!(
                            "pure builtin `{function}` cannot consume PASS in the compact ABI"
                        ));
                    }
                    (
                        KernelOwnerNodeKind::PureBuiltin {
                            kind: pure_builtin_kind(function).expect("pure builtin guard resolves"),
                        },
                        args.iter()
                            .map(|argument| {
                                (
                                    KernelOwnerEdgeRole::AbiArgument {
                                        name: argument.name.clone().into_boxed_str(),
                                    },
                                    PreparedInputReference::Syntax(argument.value),
                                )
                            })
                            .collect(),
                        None,
                        None,
                    )
                }
                AstExprKind::Pipe {
                    input,
                    op,
                    args,
                    pass,
                    arms,
                } if pure_builtin_kind(op).is_some() => {
                    if pass.is_some() || !arms.is_empty() {
                        return Err(format!(
                            "pure builtin pipe `{op}` cannot consume PASS or arms in the compact ABI"
                        ));
                    }
                    let mut inputs = Vec::with_capacity(args.len() + 1);
                    let input = expression.linked_input.unwrap_or(*input);
                    inputs.push((
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        PreparedInputReference::Syntax(input),
                    ));
                    inputs.extend(args.iter().map(|argument| {
                        (
                            KernelOwnerEdgeRole::AbiArgument {
                                name: argument.name.clone().into_boxed_str(),
                            },
                            PreparedInputReference::Syntax(argument.value),
                        )
                    }));
                    (
                        KernelOwnerNodeKind::PureBuiltin {
                            kind: pure_builtin_kind(op).expect("pure builtin pipe guard resolves"),
                        },
                        inputs,
                        None,
                        None,
                    )
                }
                AstExprKind::Call {
                    function,
                    args,
                    pass,
                } if is_kernel_host_effect(function) => {
                    if pass.is_some() {
                        return Err(format!(
                            "host effect `{function}` cannot consume PASS in the compact ABI"
                        ));
                    }
                    (
                        KernelOwnerNodeKind::HostEffect {
                            operation: function.clone().into_boxed_str(),
                        },
                        args.iter()
                            .map(|argument| {
                                (
                                    KernelOwnerEdgeRole::AbiArgument {
                                        name: argument.name.clone().into_boxed_str(),
                                    },
                                    PreparedInputReference::Syntax(argument.value),
                                )
                            })
                            .collect(),
                        None,
                        None,
                    )
                }
                AstExprKind::Pipe {
                    input,
                    op,
                    args,
                    pass,
                    arms,
                } if is_kernel_host_effect(op) => {
                    if pass.is_some() || !arms.is_empty() {
                        return Err(format!(
                            "host-effect pipe `{op}` cannot consume PASS or arms in the compact ABI"
                        ));
                    }
                    let mut inputs = Vec::with_capacity(args.len() + 1);
                    let input = expression.linked_input.unwrap_or(*input);
                    inputs.push((
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        PreparedInputReference::Syntax(input),
                    ));
                    inputs.extend(args.iter().map(|argument| {
                        (
                            KernelOwnerEdgeRole::AbiArgument {
                                name: argument.name.clone().into_boxed_str(),
                            },
                            PreparedInputReference::Syntax(argument.value),
                        )
                    }));
                    (
                        KernelOwnerNodeKind::HostEffect {
                            operation: op.clone().into_boxed_str(),
                        },
                        inputs,
                        None,
                        None,
                    )
                }
                AstExprKind::Call {
                    function,
                    args,
                    pass,
                } if callable_surfaces.contains_key(function) => {
                    let surface = &callable_surfaces[function];
                    if surface.parameters.iter().any(|parameter| !parameter.value) {
                        return Err(
                            "OUT call frames are not in the first call-composition slice"
                                .to_owned(),
                        );
                    }
                    let mut raw_edges = Vec::with_capacity(args.len());
                    let mut supplied = BTreeSet::new();
                    for argument in args {
                        let parameter = surface
                            .parameters
                            .iter()
                            .find(|parameter| parameter.name == argument.name)
                            .ok_or_else(|| {
                                format!(
                                    "user call `{function}` has no formal named `{}`",
                                    argument.name
                                )
                            })?;
                        if !supplied.insert(parameter.ordinal) {
                            return Err(format!(
                                "user call `{function}` repeats formal `{}`",
                                parameter.name
                            ));
                        }
                        raw_edges.push((
                            KernelOwnerEdgeRole::CallArgument {
                                ordinal: checked_u32(parameter.ordinal, "call argument ordinal")?,
                            },
                            PreparedInputReference::Syntax(argument.value),
                        ));
                    }
                    if supplied.len() != surface.parameters.len() {
                        return Err(format!(
                            "user call `{function}` supplies {} of {} formals",
                            supplied.len(),
                            surface.parameters.len()
                        ));
                    }
                    let inherited_formal = match surface.context_ordinal {
                        Some(target_ordinal) => match pass {
                            Some(pass) => {
                                raw_edges.push((
                                    KernelOwnerEdgeRole::CallArgument {
                                        ordinal: checked_u32(
                                            target_ordinal,
                                            "call context ordinal",
                                        )?,
                                    },
                                    PreparedInputReference::Syntax(pass.value),
                                ));
                                None
                            }
                            None => {
                                let caller_ordinal = formal_by_name.get("PASSED").copied().ok_or_else(
                                || {
                                    format!(
                                        "root call to `{function}` requires an explicit PASS context"
                                    )
                                },
                            )?;
                                Some(KernelInheritedFormal {
                                    target_ordinal: checked_u32(
                                        target_ordinal,
                                        "inherited target context ordinal",
                                    )?,
                                    caller_ordinal: checked_u32(
                                        caller_ordinal,
                                        "inherited caller context ordinal",
                                    )?,
                                })
                            }
                        },
                        None => None,
                    };
                    (
                        KernelOwnerNodeKind::UserCall {
                            target: KernelOwnerId(0),
                            inherited_formal,
                        },
                        raw_edges,
                        Some(surface.owner.clone()),
                        None,
                    )
                }
                _ => (
                    compact_ast_kind(
                        &expression.kind,
                        &expressions[index],
                        &source_paths,
                        source_payloads,
                    )?,
                    compact_ast_edges(&expression.kind)?
                        .into_iter()
                        .map(|(role, reference)| {
                            let reference = if matches!(role, KernelOwnerEdgeRole::WhenInput) {
                                expression.linked_input.unwrap_or(reference)
                            } else {
                                reference
                            };
                            (role, PreparedInputReference::Syntax(reference))
                        })
                        .collect(),
                    None,
                    None,
                ),
            }
        };
        if matches!(kind, KernelOwnerNodeKind::Hold) {
            raw_edges.extend(
                direct_hold_update_expressions(view, expression.id, &raw_expressions)?
                    .into_iter()
                    .map(|update| {
                        (
                            KernelOwnerEdgeRole::HoldUpdate,
                            PreparedInputReference::Syntax(update),
                        )
                    }),
            );
        }
        if let Some(owner) = call_target {
            call_targets.push(PreparedCallTarget { node: index, owner });
        }
        let mode = match &kind {
            KernelOwnerNodeKind::Known(_) if matches!(&expression.kind, AstExprKind::Source) => {
                FlowMode::PresentOrAbsent
            }
            KernelOwnerNodeKind::Then => FlowMode::PresentOrAbsent,
            KernelOwnerNodeKind::Absent => FlowMode::Absent,
            _ => FlowMode::Continuous,
        };
        let mut inputs = raw_edges
            .into_iter()
            .map(|(role, reference)| {
                let reference = prepared_input_reference_index(
                    reference,
                    view,
                    &owner,
                    Some(expression.id),
                    &local_by_syntax,
                    node_count,
                    &mut external_by_key,
                    &mut external_expressions,
                )?;
                Ok(KernelOwnerInputEdge {
                    role,
                    expression: checked_kernel_expression(reference)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if let Some(read_target) = read_target {
            let key = PreparedExternalExpression {
                owner: read_target.owner,
                target: read_target.target,
            };
            let external = match external_by_key.get(&key).copied() {
                Some(external) => external,
                None => {
                    let external = external_expressions.len();
                    external_by_key.insert(key.clone(), external);
                    external_expressions.push(key);
                    external
                }
            };
            let reference = node_count
                .checked_add(external)
                .ok_or_else(|| "owner expression namespace overflowed".to_owned())?;
            inputs.push(KernelOwnerInputEdge {
                role: KernelOwnerEdgeRole::ReadProvider,
                expression: checked_kernel_expression(reference)?,
            });
        }
        nodes.push(KernelOwnerNode {
            kind,
            inputs: inputs.into_boxed_slice(),
            mode,
        });
    }
    if let Some(synthetic_result) = synthetic_result {
        let (kind, edges) = match synthetic_result {
            PreparedSyntheticResult::Alias(reference) => (
                KernelOwnerNodeKind::Block,
                vec![(KernelOwnerEdgeRole::BlockResult, reference)],
            ),
            PreparedSyntheticResult::Record(fields) => (
                KernelOwnerNodeKind::Record { tag: None },
                fields
                    .into_iter()
                    .map(|entry| match entry {
                        PreparedRecordEntry::Field { name, value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: name.into_boxed_str(),
                                spread: false,
                            },
                            value,
                        ),
                        PreparedRecordEntry::Spread { value } => (
                            KernelOwnerEdgeRole::RecordField {
                                name: Box::from(""),
                                spread: true,
                            },
                            value,
                        ),
                    })
                    .collect(),
            ),
        };
        let inputs = edges
            .into_iter()
            .map(|(role, reference)| {
                let reference = prepared_input_reference_index(
                    reference,
                    view,
                    &owner,
                    None,
                    &local_by_syntax,
                    node_count,
                    &mut external_by_key,
                    &mut external_expressions,
                )?;
                Ok(KernelOwnerInputEdge {
                    role,
                    expression: checked_kernel_expression(reference)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        nodes.push(KernelOwnerNode {
            kind,
            inputs: inputs.into_boxed_slice(),
            mode: result_mode,
        });
    }
    let result_node = nodes
        .get_mut(result_index)
        .ok_or_else(|| "owner result is outside its compact node table".to_owned())?;
    if matches!(result_node.mode, FlowMode::Continuous) {
        result_node.mode = result_mode;
    }
    for (when_index, when) in nodes.iter().enumerate() {
        if !matches!(when.kind, KernelOwnerNodeKind::When) {
            continue;
        }
        for arm in &when.inputs {
            if !matches!(arm.role, KernelOwnerEdgeRole::WhenArm) {
                continue;
            }
            let arm_index = arm.expression.0 as usize;
            let Some(KernelOwnerNode {
                kind: KernelOwnerNodeKind::MatchArm { .. },
                inputs,
                ..
            }) = nodes.get(arm_index)
            else {
                continue;
            };
            let unsupported_delimiter = inputs.iter().any(|input| {
                if !matches!(input.role, KernelOwnerEdgeRole::MatchOutput) {
                    return false;
                }
                nodes
                    .get(input.expression.0 as usize)
                    .is_some_and(|output| {
                        matches!(
                            output.kind,
                            KernelOwnerNodeKind::Delimiter | KernelOwnerNodeKind::Unknown
                        )
                    })
            });
            if unsupported_delimiter {
                return Err(format!(
                    "WHEN node {when_index} has a delimiter arm whose structural record was not recovered"
                ));
            }
        }
    }
    debug_assert_eq!(nodes.len(), node_count);
    let structured_delimiter_dependents = local_dependency_cone(&nodes, structured_delimiter_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let record_spread_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            node.inputs
                .iter()
                .any(|input| {
                    matches!(
                        input.role,
                        KernelOwnerEdgeRole::RecordField { spread: true, .. }
                    )
                })
                .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let record_spread_dependents = local_dependency_cone(&nodes, record_spread_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let formal_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(node.kind, KernelOwnerNodeKind::FormalRead { .. }).then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let formal_dependents = local_dependency_cone(&nodes, formal_nodes);
    let generic_selectors = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            if !matches!(node.kind, KernelOwnerNodeKind::When) {
                return None;
            }
            node.inputs
                .iter()
                .any(|input| {
                    matches!(input.role, KernelOwnerEdgeRole::WhenInput)
                        && formal_dependents.contains(&(input.expression.0 as usize))
                })
                .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let mut generic_selector_nodes = local_input_closure(&nodes, generic_selectors);
    generic_selector_nodes.extend(nodes.iter().enumerate().filter_map(|(index, node)| {
        let KernelOwnerNodeKind::UserCall {
            inherited_formal, ..
        } = &node.kind
        else {
            return None;
        };
        (formal_dependents.contains(&index) || inherited_formal.is_some()).then_some(index)
    }));
    let generic_selector_dependents = local_dependency_cone(&nodes, generic_selector_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let detached_generic_reads = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (formal_dependents.contains(&index)
                && matches!(
                    node.kind,
                    KernelOwnerNodeKind::ValueRead { .. } | KernelOwnerNodeKind::DerivedRead { .. }
                ))
            .then(|| expressions[index].clone())
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    // `NoElement` is an ordinary tag in Boon and is intentionally ordinary in
    // the dense kernel. The legacy checker nevertheless treats that spelling
    // as the identity element of structural widening for its built-in UI ABI.
    // Mark only the local dependency cone so the migration differential can
    // recognize that old, narrower surface without teaching the new type
    // algebra the UI library's tag name.
    let no_element_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(&node.kind, KernelOwnerNodeKind::Tag(tag) if tag.as_ref() == "NoElement")
                .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    let legacy_no_element_dependents = local_dependency_cone(&nodes, no_element_nodes)
        .into_iter()
        .map(|index| expressions[index].clone())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    if let Some(pattern) = std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER") {
        let pattern = pattern.to_string_lossy();
        if format!("{owner:?}").contains(pattern.as_ref()) {
            eprintln!("kernel-owner-trace owner={owner:#?}");
            eprintln!("kernel-owner-trace externals={external_expressions:#?}");
            eprintln!("kernel-owner-trace calls={call_targets:#?}");
            for (index, node) in nodes.iter().enumerate() {
                eprintln!(
                    "kernel-owner-trace node={index} stable={:?} value={node:?}",
                    expressions.get(index)
                );
            }
        }
    }
    // Definition formals and contextual collection items are provider roots,
    // not consumer-shaped occurrence rows. The legacy checker lets downstream
    // reads back-shape those roots; the directional kernel deliberately does
    // not. Their uses and every concrete call occurrence remain differential
    // checks, so omit only these synthetic provider expressions themselves.
    let generic_formal_reads = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            matches!(
                node.kind,
                KernelOwnerNodeKind::FormalRead { .. } | KernelOwnerNodeKind::CollectionItemRead
            )
            .then(|| expressions[index].clone())
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(PreparedOwner {
        owner,
        expressions: expressions.into_boxed_slice(),
        external_expressions: external_expressions.into_boxed_slice(),
        call_targets: call_targets.into_boxed_slice(),
        compact: KernelOwnerProgramInput {
            nodes: nodes.into_boxed_slice(),
            formal_count: checked_u32(formal_count, "formal count")?,
            external_expressions: Box::new([]),
            result: checked_kernel_expression(result_index)?,
        },
        result_expression,
        public_child_owner_fields,
        generic_formal_reads,
        structured_delimiter_dependents,
        record_spread_dependents,
        generic_selector_dependents,
        detached_generic_reads,
        legacy_no_element_dependents,
    })
}

fn local_dependency_cone(nodes: &[KernelOwnerNode], seeds: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut dependents = seeds;
    loop {
        let added = nodes
            .iter()
            .enumerate()
            .filter(|(index, _)| !dependents.contains(index))
            .filter_map(|(index, node)| {
                node.inputs
                    .iter()
                    .any(|input| {
                        let input = input.expression.0 as usize;
                        input < nodes.len() && dependents.contains(&input)
                    })
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if added.is_empty() {
            return dependents;
        }
        dependents.extend(added);
    }
}

fn local_input_closure(nodes: &[KernelOwnerNode], seeds: BTreeSet<usize>) -> BTreeSet<usize> {
    let mut closure = seeds;
    let mut pending = closure.iter().copied().collect::<Vec<_>>();
    while let Some(index) = pending.pop() {
        for input in &nodes[index].inputs {
            let input = input.expression.0 as usize;
            if input < nodes.len() && closure.insert(input) {
                pending.push(input);
            }
        }
    }
    closure
}

fn prepared_input_reference_index(
    reference: PreparedInputReference,
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    source_expression: Option<usize>,
    local_by_syntax: &BTreeMap<usize, usize>,
    node_count: usize,
    external_by_key: &mut BTreeMap<PreparedExternalExpression, usize>,
    external_expressions: &mut Vec<PreparedExternalExpression>,
) -> Result<usize, String> {
    let context = source_expression.map_or_else(
        || "synthetic result".to_owned(),
        |expression| format!("expression {expression}"),
    );
    let external = match reference {
        PreparedInputReference::Syntax(reference) => {
            if let Some(local) = local_by_syntax.get(&reference).copied() {
                return Ok(local);
            }
            let target_owner = view
                .stable_check_owner_for_syntax_expression(reference)
                .ok_or_else(|| {
                    format!(
                        "owner {owner:?} {context} references syntax expression {reference} with no owner"
                    )
                })?;
            if &target_owner == owner {
                return Err(format!(
                    "owner {owner:?} {context} lost local input {reference}"
                ));
            }
            let target_expression = view
                .stable_expression_key_for_syntax(reference)
                .ok_or_else(|| {
                    format!(
                        "owner {owner:?} {context} references syntax expression {reference} with no stable identity"
                    )
                })?;
            PreparedExternalExpression {
                owner: target_owner,
                target: PreparedExternalTarget::Expression(target_expression),
            }
        }
        PreparedInputReference::OwnerResult(target_owner) => {
            if &target_owner == owner {
                return Err(format!(
                    "owner {owner:?} {context} recursively imports its own public result"
                ));
            }
            PreparedExternalExpression {
                owner: target_owner,
                target: PreparedExternalTarget::Result,
            }
        }
    };
    let external_index = match external_by_key.get(&external).copied() {
        Some(external_index) => external_index,
        None => {
            let external_index = external_expressions.len();
            external_by_key.insert(external.clone(), external_index);
            external_expressions.push(external);
            external_index
        }
    };
    node_count
        .checked_add(external_index)
        .ok_or_else(|| "owner expression namespace overflowed".to_owned())
}

fn direct_lexical_binding_reads(
    view: UnitOwnerSyntaxView<'_>,
    owner: &StableCheckOwnerKey,
    raw_expressions: &[&boon_syntax::AstExpr],
    stable_expressions: &[StableExpressionKey],
    local_by_syntax: &BTreeMap<usize, usize>,
    collection_bindings_by_scope: &BTreeMap<usize, Box<[(String, usize)]>>,
    structured_records: &BTreeMap<usize, Vec<PreparedRecordEntry>>,
) -> BTreeMap<usize, PreparedLexicalBinding> {
    let syntax_by_stable = stable_expressions
        .iter()
        .cloned()
        .zip(raw_expressions.iter().map(|expression| expression.id))
        .collect::<BTreeMap<_, _>>();
    let parent_by_syntax = raw_expressions
        .iter()
        .filter_map(|expression| {
            let (parent_owner, parent, _) =
                view.stable_expression_parent_edge_for_syntax(expression.id)?;
            (parent_owner == *owner)
                .then(|| syntax_by_stable.get(&parent).copied())
                .flatten()
                .map(|parent| (expression.id, parent))
        })
        .collect::<BTreeMap<_, _>>();
    let containing_statements =
        direct_containing_statements(view, raw_expressions, local_by_syntax);
    let statement_by_placement = view
        .statement_ids()
        .iter()
        .copied()
        .filter_map(|statement| {
            let locator = view.statement_locator(statement)?;
            Some(((locator.parent(), locator.child_index()), statement))
        })
        .collect::<BTreeMap<_, _>>();
    let mut reads = BTreeMap::new();
    for expression in raw_expressions {
        let root = match &expression.kind {
            AstExprKind::Identifier(name) => name.as_str(),
            AstExprKind::Path(path) => match path.first() {
                Some(root) => root,
                None => continue,
            },
            _ => continue,
        };
        let mut cursor = expression.id;
        let mut active = BTreeSet::new();
        while active.insert(cursor) {
            let Some(parent) = parent_by_syntax.get(&cursor).copied() else {
                break;
            };
            let Some(parent_expression) = local_by_syntax
                .get(&parent)
                .and_then(|index| raw_expressions.get(*index))
            else {
                break;
            };
            if let Some(provider) = structured_records
                .get(&parent_expression.id)
                .into_iter()
                .flatten()
                .find_map(|entry| match entry {
                    PreparedRecordEntry::Field { name, value }
                        if name == root && value != &PreparedInputReference::Syntax(cursor) =>
                    {
                        Some(value.clone())
                    }
                    PreparedRecordEntry::Field { .. } | PreparedRecordEntry::Spread { .. } => None,
                })
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(provider),
                        prefix: Box::new([]),
                        directional: false,
                    },
                );
                break;
            }
            if let AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } =
                &parent_expression.kind
                && let Some(provider) = fields
                    .iter()
                    .find(|field| !field.spread && field.name == root && field.value != cursor)
                    .map(|field| field.value)
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        prefix: Box::new([]),
                        directional: false,
                    },
                );
                break;
            }
            let call_surface = match &parent_expression.kind {
                AstExprKind::Call { function, args, .. } => Some((function.as_str(), args)),
                AstExprKind::Pipe { op, args, .. } => Some((op.as_str(), args)),
                _ => None,
            };
            if let Some((function, arguments)) = call_surface
                && let Some(context) = render_call_context_surface(function)
                && root == context.name
                && let Some(provider) = arguments
                    .iter()
                    .find(|argument| argument.named_name() == Some(context.provider))
                    .map(|argument| argument.value)
                && provider != cursor
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Known(context.flow_type),
                        prefix: Box::new([]),
                        directional: false,
                    },
                );
                break;
            }
            if let AstExprKind::Block { bindings, .. } = &parent_expression.kind
                && let Some(binding) = bindings.iter().find(|binding| binding.name == root)
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            binding.value,
                        )),
                        prefix: Box::new([]),
                        directional: false,
                    },
                );
                break;
            }
            if let AstExprKind::MatchArm { pattern, .. } = &parent_expression.kind
                && let Some(prefix) = match_pattern_binding_prefix(pattern, root)
                && let Some(provider) =
                    view.pattern_selector_for_syntax_expression(parent_expression.id)
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        prefix: prefix.into_boxed_slice(),
                        directional: true,
                    },
                );
                break;
            }
            if let Some((_, provider)) = collection_bindings_by_scope
                .get(&parent_expression.id)
                .into_iter()
                .flatten()
                .find(|(name, provider)| name == root && *provider != expression.id)
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            *provider,
                        )),
                        prefix: Box::new([]),
                        directional: true,
                    },
                );
                break;
            }
            cursor = parent;
        }
        if reads.contains_key(&expression.id) {
            continue;
        }
        // Multiline record fields are sibling statements rather than child
        // expression edges of the delimiter. Follow the parser-owned
        // statement containment chain to the nearest enclosing structured
        // record instead of rebuilding a lexical scope graph.
        let Some(statement) = containing_statements.get(&expression.id).copied() else {
            continue;
        };
        let mut direct_child = statement;
        while let Some(locator) = view.statement_locator(direct_child) {
            let Some(parent) = locator.parent() else {
                break;
            };
            // A HOLD name is a private read capability available to authored
            // update statements, not to the initializer expression. Update
            // bodies are statement children of the HOLD, so statement
            // containment distinguishes the two without guessing from names
            // or source spans.
            if let Some(parent_statement) = view.statement_for_local(parent)
                && matches!(&parent_statement.kind, AstStatementKind::Hold { name: Some(name), .. } if name == root)
                && let Some(provider) = parent_statement.expr
            {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(PreparedInputReference::Syntax(
                            provider,
                        )),
                        prefix: Box::new([]),
                        directional: true,
                    },
                );
                break;
            }
            let provider = view
                .statement_for_local(parent)
                .and_then(|parent_statement| {
                    parent_statement.children[..locator.child_index()]
                        .iter()
                        .enumerate()
                        .rev()
                        .find_map(|(child_index, child)| {
                            (statement_binding_name(&child.kind) == Some(root)).then(|| {
                                statement_by_placement
                                    .get(&(Some(parent), child_index))
                                    .and_then(|child| view.statement_value_expression(*child))
                                    .or(child.expr)
                            })
                        })
                        .flatten()
                        .map(PreparedInputReference::Syntax)
                });
            if let Some(provider) = provider {
                reads.insert(
                    expression.id,
                    PreparedLexicalBinding {
                        provider: PreparedLexicalProvider::Input(provider),
                        prefix: Box::new([]),
                        directional: false,
                    },
                );
                break;
            }
            direct_child = parent;
        }
        if !reads.contains_key(&expression.id)
            && std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER").is_some_and(|pattern| {
                format!("{owner:?}").contains(pattern.to_string_lossy().as_ref())
            })
        {
            let mut statement_chain = Vec::new();
            let mut statement = containing_statements.get(&expression.id).copied();
            while let Some(current) = statement {
                statement_chain.push((
                    current,
                    view.statement_for_local(current)
                        .map(|statement| (format!("{:?}", statement.kind), statement.expr)),
                ));
                statement = view
                    .statement_locator(current)
                    .and_then(|locator| locator.parent());
            }
            eprintln!(
                "kernel-owner-trace unresolved-local expression={} root={root} expression_parents={:?} statement_chain={statement_chain:?} structured_records={structured_records:?}",
                expression.id,
                parent_by_syntax.get(&expression.id),
            );
        }
    }
    reads
}

fn statement_binding_name(kind: &AstStatementKind) -> Option<&str> {
    match kind {
        AstStatementKind::Field { name } => Some(name),
        AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::Hold {
            field: Some(name), ..
        }
        | AstStatementKind::List {
            field: Some(name), ..
        } => Some(name),
        AstStatementKind::Function { .. }
        | AstStatementKind::Source { field: None, .. }
        | AstStatementKind::Hold { field: None, .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block
        | AstStatementKind::Spread
        | AstStatementKind::Expression => None,
    }
}

fn direct_containing_statements(
    view: UnitOwnerSyntaxView<'_>,
    raw_expressions: &[&boon_syntax::AstExpr],
    local_by_syntax: &BTreeMap<usize, usize>,
) -> BTreeMap<usize, UnitLocalStatementId> {
    let mut owners = BTreeMap::new();
    let mut statements = view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .collect::<Vec<_>>();
    statements.sort_by_key(|(statement, _)| {
        let mut depth = 0usize;
        let mut cursor = *statement;
        while let Some(parent) = view
            .statement_locator(cursor)
            .and_then(|locator| locator.parent())
        {
            depth = depth.saturating_add(1);
            cursor = parent;
        }
        std::cmp::Reverse(depth)
    });
    for (statement_id, statement) in statements {
        let Some(root) = statement.expr else {
            continue;
        };
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        while let Some(syntax) = pending.pop() {
            if !visited.insert(syntax) {
                continue;
            }
            let Some(expression) = local_by_syntax
                .get(&syntax)
                .and_then(|index| raw_expressions.get(*index))
            else {
                continue;
            };
            owners.entry(syntax).or_insert(statement_id);
            pending.extend(
                source_ast_edges(&expression.kind)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, input)| input),
            );
        }
    }
    owners
}

struct PreparedCallContextSurface {
    name: &'static str,
    provider: &'static str,
    flow_type: Type,
}

/// Return call-local values supplied by the active render-constructor ABI.
///
/// This is ordinary ABI metadata: `element` is not a Boon keyword, and other
/// render libraries may expose a different context name or no context at all.
fn render_call_context_surface(function: &str) -> Option<PreparedCallContextSurface> {
    render_constructor_kind(function)?;
    (function != "Scene/new").then(|| {
        let boolean = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        PreparedCallContextSurface {
            name: "element",
            provider: "element",
            flow_type: Type::object(ObjectShape::from_ordered_fields(
                [
                    ("hovered".to_owned(), boolean.clone()),
                    ("focused".to_owned(), boolean.clone()),
                    ("pressed".to_owned(), boolean.clone()),
                    ("selected".to_owned(), boolean),
                ],
                true,
            )),
        }
    })
}

fn prepared_lexical_read_node(
    binding: &PreparedLexicalBinding,
    suffix: &[Box<str>],
) -> Result<
    (
        KernelOwnerNodeKind,
        Vec<(KernelOwnerEdgeRole, PreparedInputReference)>,
    ),
    String,
> {
    let fields = binding
        .prefix
        .iter()
        .map(String::as_str)
        .chain(suffix.iter().map(Box::<str>::as_ref))
        .collect::<Vec<_>>();
    match &binding.provider {
        PreparedLexicalProvider::Input(provider) => {
            let fields = fields
                .into_iter()
                .map(|field| field.to_owned().into_boxed_str())
                .collect();
            Ok((
                if binding.directional {
                    KernelOwnerNodeKind::DerivedRead { fields }
                } else {
                    KernelOwnerNodeKind::LexicalRead { fields }
                },
                vec![(KernelOwnerEdgeRole::ReadProvider, provider.clone())],
            ))
        }
        PreparedLexicalProvider::Known(provider) => Ok((
            KernelOwnerNodeKind::Known(project_checked_type(provider, &fields)?),
            Vec::new(),
        )),
    }
}

fn project_checked_type(provider: &Type, fields: &[&str]) -> Result<Type, String> {
    let mut current = provider;
    for field in fields {
        let Type::Object(shape) = current else {
            return Err(format!(
                "ABI context projection `{}` crosses non-object type {current:?}",
                fields.join(".")
            ));
        };
        current = shape.fields.get(*field).ok_or_else(|| {
            format!(
                "ABI context projection `{}` has no field `{field}`",
                fields.join(".")
            )
        })?;
    }
    Ok(current.clone())
}

type PreparedCollectionBindingsByScope = BTreeMap<usize, Box<[(String, usize)]>>;

fn direct_collection_callback_bindings(
    raw_expressions: &[&boon_syntax::AstExpr],
) -> Result<(BTreeMap<usize, usize>, PreparedCollectionBindingsByScope), String> {
    let expressions = raw_expressions
        .iter()
        .map(|expression| (expression.id, *expression))
        .collect::<BTreeMap<_, _>>();
    let mut inputs = BTreeMap::new();
    let mut scopes = BTreeMap::new();
    for expression in raw_expressions {
        let (function, provider, arguments) = match &expression.kind {
            AstExprKind::Pipe {
                input, op, args, ..
            } => (
                op.as_str(),
                expression.linked_input.unwrap_or(*input),
                args.as_slice(),
            ),
            AstExprKind::Call { function, args, .. } => {
                let Some(provider) = args
                    .iter()
                    .find(|argument| argument.named_name() == Some("list"))
                    .map(|argument| argument.value)
                else {
                    continue;
                };
                (function.as_str(), provider, args.as_slice())
            }
            _ => continue,
        };
        if !collection_callback_builtin(function) {
            continue;
        }
        let mut bindings = Vec::new();
        for argument in arguments
            .iter()
            .filter(|argument| argument.kind == AstCallArgKind::BareBinding)
        {
            let name = expressions
                .get(&argument.value)
                .and_then(|expression| match &expression.kind {
                    AstExprKind::Identifier(name) => Some(name.clone()),
                    _ => None,
                })
                .ok_or_else(|| {
                    format!(
                        "collection callback `{function}` has a non-identifier binding argument"
                    )
                })?;
            if inputs.insert(argument.value, provider).is_some()
                || bindings.iter().any(|(existing, _)| existing == &name)
            {
                return Err(format!(
                    "collection callback `{function}` repeats binding `{name}`"
                ));
            }
            bindings.push((name, argument.value));
        }
        if !bindings.is_empty() {
            scopes.insert(expression.id, bindings.into_boxed_slice());
        }
    }
    Ok((inputs, scopes))
}

fn collection_callback_builtin(function: &str) -> bool {
    matches!(
        function,
        "List/filter"
            | "List/retain"
            | "List/remove"
            | "List/map"
            | "List/find"
            | "List/sort_by"
            | "List/then_by"
            | "List/any"
            | "List/every"
    )
}

fn match_pattern_binding_prefix(pattern: &AstMatchPattern, root: &str) -> Option<Vec<String>> {
    match pattern {
        AstMatchPattern::Binding { name } if name == root => Some(Vec::new()),
        AstMatchPattern::Tag { fields, .. } if fields.iter().any(|field| field == root) => {
            Some(vec![root.to_owned()])
        }
        AstMatchPattern::Wildcard
        | AstMatchPattern::Number { .. }
        | AstMatchPattern::Text { .. }
        | AstMatchPattern::Tag { .. }
        | AstMatchPattern::Binding { .. }
        | AstMatchPattern::Invalid { .. }
        | AstMatchPattern::Bits { .. } => None,
    }
}

fn direct_child_owner_result(
    view: UnitOwnerSyntaxView<'_>,
    root_statement: boon_syntax::UnitLocalStatementId,
) -> Result<Option<PreparedSyntheticResult>, String> {
    let mut names = BTreeSet::new();
    let mut children = Vec::new();
    for boundary in view
        .child_owners()
        .iter()
        .filter(|boundary| boundary.parent() == Some(root_statement))
    {
        let statement = view
            .statement_for_local(boundary.statement())
            .ok_or_else(|| "child owner boundary has no parser statement".to_owned())?;
        let name = match &statement.kind {
            AstStatementKind::Field { name } => Some(name),
            AstStatementKind::Source {
                field: Some(name), ..
            }
            | AstStatementKind::Hold {
                field: Some(name), ..
            }
            | AstStatementKind::List {
                field: Some(name), ..
            } => Some(name),
            _ => None,
        };
        if let Some(name) = name
            && !names.insert(name.clone())
        {
            return Err(format!(
                "structured field repeats direct child name `{name}`"
            ));
        }
        let child_owner = view
            .stable_check_owner_for_local_statement(boundary.statement())
            .ok_or_else(|| format!("structured child {name:?} has no stable owner"))?;
        if child_owner == view.stable_key() {
            return Err(format!(
                "structured child {name:?} did not cross an owner boundary"
            ));
        }
        children.push((
            name.cloned(),
            PreparedInputReference::OwnerResult(child_owner),
        ));
    }
    match children.as_slice() {
        [] => Ok(None),
        [(None, reference)] => Ok(Some(PreparedSyntheticResult::Alias(reference.clone()))),
        _ if children.iter().all(|(name, _)| name.is_some()) => {
            Ok(Some(PreparedSyntheticResult::Record(
                children
                    .into_iter()
                    .map(|(name, value)| PreparedRecordEntry::Field {
                        name: name.expect("all child names were checked"),
                        value,
                    })
                    .collect(),
            )))
        }
        _ => Ok(None),
    }
}

fn direct_structured_statement_records(
    view: UnitOwnerSyntaxView<'_>,
) -> Result<BTreeMap<usize, Vec<PreparedRecordEntry>>, String> {
    let expressions = view
        .expressions()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let mut records = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    for statement in view.statements() {
        let Some(direct) = statement.expr else {
            continue;
        };
        let mut delimiters = Vec::new();
        if expressions.get(&direct).is_some_and(|expression| {
            matches!(&expression.kind, AstExprKind::Delimiter)
                || matches!(&expression.kind, AstExprKind::Object(fields) if fields.is_empty())
        }) {
            delimiters.push(direct);
        }
        if let Some(expression) = expressions.get(&direct) {
            delimiters.extend(
                compact_ast_edges(&expression.kind)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(_, input)| input)
                    .filter(|input| {
                        expressions.get(input).is_some_and(|expression| {
                            matches!(&expression.kind, AstExprKind::Delimiter)
                                || matches!(&expression.kind, AstExprKind::Object(fields) if fields.is_empty())
                        })
                    }),
            );
        }
        if delimiters.is_empty() {
            continue;
        }
        let mut names = BTreeSet::new();
        let fields = statement
            .children
            .iter()
            .filter_map(|child| {
                let entry = match &child.kind {
                    AstStatementKind::Field { name } => Some(Some(name)),
                    AstStatementKind::Source {
                        field: Some(name), ..
                    }
                    | AstStatementKind::Hold {
                        field: Some(name), ..
                    }
                    | AstStatementKind::List {
                        field: Some(name), ..
                    } => Some(Some(name)),
                    AstStatementKind::Spread => Some(None),
                    _ => None,
                }?;
                let value = child.expr?;
                Some(match entry {
                    Some(name) => PreparedRecordEntry::Field {
                        name: name.clone(),
                        value: PreparedInputReference::Syntax(value),
                    },
                    None => PreparedRecordEntry::Spread {
                        value: PreparedInputReference::Syntax(value),
                    },
                })
            })
            .collect::<Vec<_>>();
        for field in &fields {
            let PreparedRecordEntry::Field { name, .. } = field else {
                continue;
            };
            if !names.insert(name.clone()) {
                return Err(format!(
                    "structured delimiter repeats direct field `{name}`"
                ));
            }
        }
        if fields.is_empty() {
            continue;
        }
        for delimiter in delimiters {
            if claimed.insert(delimiter) {
                records.insert(delimiter, fields.clone());
            }
        }
    }
    Ok(records)
}

fn direct_hold_update_expressions(
    view: UnitOwnerSyntaxView<'_>,
    hold: usize,
    expressions: &[&boon_syntax::AstExpr],
) -> Result<Vec<usize>, String> {
    let statement = view
        .statements()
        .find(|statement| statement.expr == Some(hold))
        .ok_or_else(|| format!("HOLD expression {hold} has no owning statement"))?;
    let mut updates = Vec::new();
    for child in &statement.children {
        let Some(update) = child.expr else {
            return Err("HOLD update statement has no direct expression".to_owned());
        };
        let expression = expressions
            .iter()
            .find(|expression| expression.id == update)
            .ok_or_else(|| format!("HOLD update expression {update} is not local"))?;
        if let AstExprKind::Latest { branches } = &expression.kind {
            updates.extend(branches.iter().copied());
        } else {
            updates.push(update);
        }
    }
    Ok(updates)
}

fn direct_view_source_payload_paths(
    expressions: &[&boon_syntax::AstExpr],
    stable_expressions: &[StableExpressionKey],
    local_by_syntax: &BTreeMap<usize, usize>,
    statement_roots: &[(usize, boon_syntax::StableStatementKey)],
) -> Result<BTreeMap<StableExpressionKey, String>, String> {
    fn visit(
        reference: usize,
        expressions: &[&boon_syntax::AstExpr],
        local_by_syntax: &BTreeMap<usize, usize>,
        prefix: &[String],
        projection: &mut Vec<String>,
        active: &mut BTreeSet<usize>,
        queries: &mut BTreeMap<usize, String>,
    ) -> Result<(), String> {
        let Some(index) = local_by_syntax.get(&reference).copied() else {
            return Ok(());
        };
        if !active.insert(index) {
            return Ok(());
        }
        let expression = expressions[index];
        if matches!(expression.kind, AstExprKind::Source) {
            let canonical_path = prefix
                .iter()
                .chain(projection.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join(".");
            if !canonical_path.is_empty() {
                match queries.entry(index) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(canonical_path);
                    }
                    std::collections::btree_map::Entry::Occupied(entry)
                        if entry.get() == &canonical_path => {}
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        return Err(format!(
                            "source expression {} has conflicting stable paths `{}` and `{canonical_path}`",
                            expression.id,
                            entry.get()
                        ));
                    }
                }
            }
        }
        for (role, input) in source_ast_edges(&expression.kind)? {
            let projection_len = projection.len();
            if let KernelOwnerEdgeRole::RecordField {
                name,
                spread: false,
            } = role
            {
                projection.push(name.into());
            }
            visit(
                input,
                expressions,
                local_by_syntax,
                prefix,
                projection,
                active,
                queries,
            )?;
            projection.truncate(projection_len);
        }
        active.remove(&index);
        Ok(())
    }

    let mut by_index = BTreeMap::new();
    for (root, statement) in statement_roots {
        visit(
            *root,
            expressions,
            local_by_syntax,
            &statement_source_path_prefix(statement),
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut by_index,
        )?;
    }
    by_index
        .into_iter()
        .map(|(index, path)| {
            stable_expressions
                .get(index)
                .cloned()
                .map(|expression| (expression, path))
                .ok_or_else(|| "source expression has no stable identity".to_owned())
        })
        .collect()
}

fn source_ast_edges(kind: &AstExprKind) -> Result<Vec<(KernelOwnerEdgeRole, usize)>, String> {
    match kind {
        AstExprKind::Identifier(_) | AstExprKind::Path(_) => Ok(Vec::new()),
        AstExprKind::Call { args, pass, .. } => Ok(args
            .iter()
            .map(|argument| (KernelOwnerEdgeRole::CollectionItem, argument.value))
            .chain(
                pass.iter()
                    .map(|pass| (KernelOwnerEdgeRole::CollectionItem, pass.value)),
            )
            .collect()),
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => Ok(
            std::iter::once((KernelOwnerEdgeRole::CollectionItem, *input))
                .chain(
                    args.iter()
                        .map(|argument| (KernelOwnerEdgeRole::CollectionItem, argument.value)),
                )
                .chain(
                    pass.iter()
                        .map(|pass| (KernelOwnerEdgeRole::CollectionItem, pass.value)),
                )
                .chain(
                    arms.iter()
                        .map(|arm| (KernelOwnerEdgeRole::CollectionItem, *arm)),
                )
                .collect(),
        ),
        AstExprKind::Block { bindings, result } => Ok(bindings
            .iter()
            .map(|binding| (KernelOwnerEdgeRole::CollectionItem, binding.value))
            .chain(
                result
                    .iter()
                    .map(|result| (KernelOwnerEdgeRole::BlockResult, *result)),
            )
            .collect()),
        _ => compact_ast_edges(kind),
    }
}

fn compact_ast_kind(
    kind: &AstExprKind,
    stable_key: &StableExpressionKey,
    source_paths: &BTreeMap<StableExpressionKey, String>,
    source_payloads: &BTreeMap<String, Type>,
) -> Result<KernelOwnerNodeKind, String> {
    Ok(match kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => KernelOwnerNodeKind::Text,
        AstExprKind::TextTemplate { .. } => KernelOwnerNodeKind::TextTemplate,
        AstExprKind::Number(_) => KernelOwnerNodeKind::Number,
        AstExprKind::ByteLiteral { .. } => KernelOwnerNodeKind::Byte,
        AstExprKind::BitsLiteral { width, .. } => KernelOwnerNodeKind::Bits(*width),
        AstExprKind::Tag(name) if name == "SKIP" => KernelOwnerNodeKind::Absent,
        AstExprKind::Tag(name) => KernelOwnerNodeKind::Tag(name.clone().into()),
        AstExprKind::Source => {
            let canonical_path = source_paths
                .get(stable_key)
                .ok_or_else(|| "SOURCE has no payload ABI query".to_owned())?;
            let payload = source_payloads
                .get(canonical_path)
                .ok_or_else(|| format!("SOURCE payload ABI `{canonical_path}` was not supplied"))?;
            if !type_is_recursively_closed(payload) {
                return Err(format!(
                    "SOURCE payload ABI `{canonical_path}` is not recursively closed: {payload:?}"
                ));
            }
            KernelOwnerNodeKind::Known(payload.clone())
        }
        AstExprKind::TaggedObject { tag, .. } => KernelOwnerNodeKind::Record {
            tag: Some(tag.clone().into()),
        },
        AstExprKind::Object(_) => KernelOwnerNodeKind::Record { tag: None },
        AstExprKind::Block { .. } => KernelOwnerNodeKind::Block,
        AstExprKind::ListLiteral { .. } => {
            KernelOwnerNodeKind::Collection(KernelCollectionKind::List)
        }
        AstExprKind::BytesLiteral { .. } => {
            KernelOwnerNodeKind::Collection(KernelCollectionKind::Bytes)
        }
        AstExprKind::SetLiteral { .. } => {
            KernelOwnerNodeKind::Collection(KernelCollectionKind::Set)
        }
        AstExprKind::MapLiteral { .. } => {
            KernelOwnerNodeKind::Collection(KernelCollectionKind::Map)
        }
        AstExprKind::MapEntry { .. } => KernelOwnerNodeKind::MapEntry,
        AstExprKind::Draining { .. } => KernelOwnerNodeKind::Draining,
        AstExprKind::Hold { .. } => KernelOwnerNodeKind::Hold,
        AstExprKind::Latest { .. } => KernelOwnerNodeKind::Latest,
        AstExprKind::When { .. } => KernelOwnerNodeKind::When,
        AstExprKind::Pipe { op, arms, .. } if op == "WHILE" && !arms.is_empty() => {
            KernelOwnerNodeKind::When
        }
        AstExprKind::Then { .. } => KernelOwnerNodeKind::Then,
        AstExprKind::Infix { op, .. } => KernelOwnerNodeKind::Infix {
            operation: op.clone().into_boxed_str(),
        },
        AstExprKind::MatchArm { pattern, .. } => KernelOwnerNodeKind::MatchArm {
            pattern: compact_pattern(pattern),
        },
        AstExprKind::Delimiter => KernelOwnerNodeKind::Delimiter,
        unsupported => return Err(format!("unsupported owner node {unsupported:?}")),
    })
}

fn render_constructor_kind(function: &str) -> Option<KernelRenderConstructorKind> {
    Some(match function {
        "Scene/new" => KernelRenderConstructorKind::Fixed("Scene".into()),
        "Scene/Element/stripe" => KernelRenderConstructorKind::StripeDirection,
        "Scene/Element/block" => KernelRenderConstructorKind::Fixed("Block".into()),
        "Scene/Element/text" => KernelRenderConstructorKind::Fixed("Text".into()),
        "Scene/Element/label" => KernelRenderConstructorKind::Fixed("Label".into()),
        "Scene/Element/text_input" => KernelRenderConstructorKind::Fixed("TextInput".into()),
        "Scene/Element/button" => KernelRenderConstructorKind::Fixed("Button".into()),
        _ => return None,
    })
}

fn pure_builtin_kind(function: &str) -> Option<KernelPureBuiltinKind> {
    Some(match function {
        "Text/trim" | "Text/to_lowercase" | "Text/to_uppercase" => {
            KernelPureBuiltinKind::TextTransform
        }
        "Text/slice" => KernelPureBuiltinKind::TextSlice,
        "Text/length" => KernelPureBuiltinKind::TextLength,
        "Text/concat" => KernelPureBuiltinKind::TextConcat,
        "Text/time_range_label" => KernelPureBuiltinKind::TextConcat,
        "Text/is_empty" | "Text/is_not_empty" | "Text/starts_with" | "Text/contains"
        | "Text/all_chars_in" => KernelPureBuiltinKind::TextPredicate,
        "Text/to_number" => KernelPureBuiltinKind::TextToNumber,
        "Number/to_text" | "Number/to_ascii_text" | "Number/to_codepoint_text" => {
            KernelPureBuiltinKind::NumberToText
        }
        "Number/add" | "Number/subtract" | "Number/min" | "Number/max" | "Number/bit_width"
        | "Number/ceil" | "Number/floor" | "Number/truncate" | "Number/interpolate" => {
            KernelPureBuiltinKind::NumberMath
        }
        "Number/round" => KernelPureBuiltinKind::NumberRound,
        "Number/project_offset" | "Number/project_time" | "Number/project_width" => {
            KernelPureBuiltinKind::NumberProjection
        }
        "List/count" | "List/length" | "List/sum" => KernelPureBuiltinKind::ListLength,
        "List/is_not_empty" | "List/any" | "List/every" => KernelPureBuiltinKind::ListPredicate,
        "List/filter" | "List/retain" | "List/remove" => KernelPureBuiltinKind::ListFilter,
        "List/map" => KernelPureBuiltinKind::ListMap,
        "List/find" => KernelPureBuiltinKind::ListFind,
        "List/latest" => KernelPureBuiltinKind::ListLatest,
        "List/append" => KernelPureBuiltinKind::ListAppend,
        "List/sort_by" | "List/then_by" => KernelPureBuiltinKind::ListSort,
        "List/chunk" => KernelPureBuiltinKind::ListChunk,
        "Text/join" => KernelPureBuiltinKind::TextJoin,
        "Field/color" => KernelPureBuiltinKind::FieldColor,
        _ => return None,
    })
}

fn compact_pattern(pattern: &AstMatchPattern) -> KernelPattern {
    match pattern {
        AstMatchPattern::Wildcard => KernelPattern::Wildcard,
        AstMatchPattern::Number { .. } => KernelPattern::Number,
        AstMatchPattern::Text { .. } => KernelPattern::Text,
        AstMatchPattern::Bits { width, .. } => KernelPattern::Bits { width: *width },
        AstMatchPattern::Tag { name, fields } => KernelPattern::Tag {
            name: name.clone().into_boxed_str(),
            fields: fields
                .iter()
                .cloned()
                .map(String::into_boxed_str)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        },
        AstMatchPattern::Binding { name } => KernelPattern::Binding {
            name: name.clone().into_boxed_str(),
        },
        AstMatchPattern::Invalid { .. } => KernelPattern::Invalid,
    }
}

fn compact_ast_edges(kind: &AstExprKind) -> Result<Vec<(KernelOwnerEdgeRole, usize)>, String> {
    let edges = match kind {
        AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::BitsLiteral { .. }
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Delimiter => Vec::new(),
        AstExprKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                AstTextSegment::Static { .. } => None,
                AstTextSegment::Dynamic { value } => {
                    Some((KernelOwnerEdgeRole::TextDynamic, *value))
                }
            })
            .collect(),
        AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => fields
            .iter()
            .map(|field| {
                (
                    KernelOwnerEdgeRole::RecordField {
                        name: field.name.clone().into(),
                        spread: field.spread,
                    },
                    field.value,
                )
            })
            .collect(),
        AstExprKind::Block { bindings, result } => {
            let _ = bindings;
            if result.is_none() {
                return Err("empty BLOCK is not in the first dense slice".to_owned());
            }
            result
                .iter()
                .map(|result| (KernelOwnerEdgeRole::BlockResult, *result))
                .collect()
        }
        AstExprKind::ListLiteral { items, .. }
        | AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => items
            .iter()
            .map(|item| (KernelOwnerEdgeRole::CollectionItem, *item))
            .collect(),
        AstExprKind::MapEntry { key, value } => vec![
            (KernelOwnerEdgeRole::MapKey, *key),
            (KernelOwnerEdgeRole::MapValue, *value),
        ],
        AstExprKind::MapLiteral { entries } => entries
            .iter()
            .map(|entry| (KernelOwnerEdgeRole::MapEntry, *entry))
            .collect(),
        AstExprKind::Draining { input } => {
            vec![(KernelOwnerEdgeRole::DrainingInput, *input)]
        }
        AstExprKind::Hold { initial, .. } => {
            vec![(KernelOwnerEdgeRole::HoldInitial, *initial)]
        }
        AstExprKind::Latest { branches } => branches
            .iter()
            .map(|branch| (KernelOwnerEdgeRole::LatestBranch, *branch))
            .collect(),
        AstExprKind::When { input, arms }
        | AstExprKind::Pipe {
            input, op: _, arms, ..
        } => std::iter::once((KernelOwnerEdgeRole::WhenInput, *input))
            .chain(arms.iter().map(|arm| (KernelOwnerEdgeRole::WhenArm, *arm)))
            .collect(),
        AstExprKind::Then { input, output } => {
            std::iter::once((KernelOwnerEdgeRole::ThenInput, *input))
                .chain(
                    output
                        .iter()
                        .map(|output| (KernelOwnerEdgeRole::ThenOutput, *output)),
                )
                .collect()
        }
        AstExprKind::Infix { left, right, .. } => vec![
            (KernelOwnerEdgeRole::InfixLeft, *left),
            (KernelOwnerEdgeRole::InfixRight, *right),
        ],
        AstExprKind::MatchArm { output, .. } => output
            .iter()
            .map(|output| (KernelOwnerEdgeRole::MatchOutput, *output))
            .collect(),
        AstExprKind::Arrow { output, .. } => output
            .iter()
            .map(|output| (KernelOwnerEdgeRole::ArrowOutput, *output))
            .collect(),
        unsupported => return Err(format!("unsupported owner node {unsupported:?}")),
    };
    Ok(edges)
}

fn statement_source_path_prefix(statement: &boon_syntax::StableStatementKey) -> Vec<String> {
    let mut prefix = statement
        .route
        .owner
        .iter()
        .flat_map(|owner| owner.segments())
        .filter_map(|segment| {
            let name = segment.names.first()?;
            Some(match segment.kind {
                UnitItemKind::Function => format!("FUNCTION:{name}"),
                UnitItemKind::Field
                | UnitItemKind::Source
                | UnitItemKind::Hold
                | UnitItemKind::List => name.clone(),
            })
        })
        .collect::<Vec<_>>();
    prefix.extend(
        statement
            .route
            .statement_route
            .iter()
            .filter_map(|segment| {
                let name = segment.names.first()?;
                Some(match segment.kind {
                    StableStatementKind::Function => format!("FUNCTION:{name}"),
                    StableStatementKind::Field
                    | StableStatementKind::Source
                    | StableStatementKind::Hold
                    | StableStatementKind::List => name.clone(),
                    StableStatementKind::Block
                    | StableStatementKind::Spread
                    | StableStatementKind::Expression => return None,
                })
            }),
    );
    prefix
}

fn checked_kernel_expression(expression: usize) -> Result<KernelExpressionId, String> {
    checked_u32(expression, "kernel owner expression namespace").map(KernelExpressionId)
}

fn checked_u32(value: usize, context: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("{context} exceeds u32"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::{
        CheckedDeclarationKind, CheckedProgramFields, CheckedStatementKind, ObjectShape,
        SharedVariantSet, TypeVar, Variant,
    };
    use boon_parser::{parse_project_syntax, parse_source};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::time::Instant;

    fn alpha_normalize_owner(
        result: &FlowType,
        expressions: impl IntoIterator<Item = FlowType>,
    ) -> (FlowType, Vec<FlowType>) {
        fn normalize_flow(flow: &FlowType, variables: &mut BTreeMap<TypeVar, TypeVar>) -> FlowType {
            FlowType {
                mode: flow.mode,
                ty: normalize_type(&flow.ty, variables),
            }
        }
        fn normalize_shape(
            shape: &ObjectShape,
            variables: &mut BTreeMap<TypeVar, TypeVar>,
        ) -> ObjectShape {
            ObjectShape {
                fields: shape
                    .fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), normalize_type(ty, variables)))
                    .collect(),
                field_order: shape.field_order.clone(),
                open: shape.open,
            }
        }
        fn normalize_type(ty: &Type, variables: &mut BTreeMap<TypeVar, TypeVar>) -> Type {
            match ty {
                Type::Var(variable) => {
                    let next = TypeVar(
                        u32::try_from(variables.len()).expect("oracle alpha count exceeds u32"),
                    );
                    Type::Var(*variables.entry(*variable).or_insert(next))
                }
                Type::VariantSet(variants) => Type::VariantSet(SharedVariantSet::new(
                    variants
                        .iter()
                        .map(|variant| match variant {
                            Variant::Tag(tag) => Variant::Tag(tag.clone()),
                            Variant::Tagged { tag, fields } => {
                                Variant::tagged(tag.clone(), normalize_shape(fields, variables))
                            }
                        })
                        .collect(),
                )),
                Type::Object(shape) => Type::object(normalize_shape(shape, variables)),
                Type::List(item) => Type::List(Type::shared(normalize_type(item, variables))),
                Type::Function { args, result } => Type::Function {
                    args: args
                        .iter()
                        .map(|arg| normalize_type(arg, variables))
                        .collect(),
                    result: Box::new(normalize_flow(result, variables)),
                },
                Type::Union(members) => Type::Union(
                    members
                        .iter()
                        .map(|member| normalize_type(member, variables))
                        .collect(),
                ),
                Type::Map { key, value } => Type::Map {
                    key: Box::new(normalize_type(key, variables)),
                    value: Box::new(normalize_type(value, variables)),
                },
                Type::Set(item) => Type::Set(Type::shared(normalize_type(item, variables))),
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

        let mut variables = BTreeMap::new();
        let result = normalize_flow(result, &mut variables);
        let expressions = expressions
            .into_iter()
            .map(|flow| normalize_flow(&flow, &mut variables))
            .collect();
        (result, expressions)
    }

    fn assert_owner_matches_current(
        owner: &KernelOwnerOracleEntry,
        checked_by_stable_key: &BTreeMap<StableExpressionKey, FlowType>,
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        context: &str,
    ) {
        // The old checked image loses multiline delimiter structure and the
        // fields contributed by record spreads at their local owner boundary.
        // Keep only those exact local cones out of the differential; concrete
        // downstream calls and all unaffected expressions remain oracle checks.
        let compare_result = owner.result_expression.as_ref().is_none_or(|result| {
            !owner.structured_delimiter_dependents.contains(result)
                && !owner.record_spread_dependents.contains(result)
        });
        let generic_selector_result = owner
            .result_expression
            .as_ref()
            .is_some_and(|result| owner.generic_selector_dependents.contains(result));
        let legacy_no_element_result = owner
            .result_expression
            .as_ref()
            .is_some_and(|result| owner.legacy_no_element_dependents.contains(result));
        // `DefinitionArtifact.result` is the public owner interface. A checked
        // body/root expression may legitimately be a narrower occurrence
        // surface (notably a record containing the initial epoch of a HOLD),
        // so compare that row below with the other expressions rather than
        // substituting it for declaration authority here.
        let current_result = if !compare_result {
            owner.result.clone()
        } else {
            let current = checked_public_owner_result(checked, project, &owner.owner)
                .or_else(|| {
                    owner
                        .result_expression
                        .as_ref()
                        .and_then(|result| checked_by_stable_key.get(result))
                        .cloned()
                })
                .unwrap_or_else(|| panic!("{context} has no current public owner result"));
            checked_public_child_composed_result(owner, current, checked, project, context)
        };
        let compared = owner
            .expressions
            .iter()
            .filter(|(stable_key, _)| {
                !owner.generic_formal_reads.contains(stable_key)
                    && !owner.detached_generic_reads.contains(stable_key)
                    && !owner.structured_delimiter_dependents.contains(stable_key)
                    && !owner.record_spread_dependents.contains(stable_key)
            })
            .map(|(stable_key, flow)| {
                let current = if owner.result_expression.as_ref() == Some(stable_key)
                    && !owner.public_child_owner_fields.is_empty()
                {
                    current_result.clone()
                } else {
                    checked_by_stable_key
                        .get(stable_key)
                        .unwrap_or_else(|| {
                            panic!("{context} has no current expression {stable_key:#?}")
                        })
                        .clone()
                };
                (
                    flow.clone(),
                    current,
                    owner.generic_selector_dependents.contains(stable_key),
                    owner.legacy_no_element_dependents.contains(stable_key),
                )
            })
            .collect::<Vec<_>>();
        let mut kernel_expressions = Vec::with_capacity(compared.len());
        let mut current_expressions = Vec::with_capacity(compared.len());
        let mut generic_selector_expressions = Vec::with_capacity(compared.len());
        let mut legacy_no_element_expressions = Vec::with_capacity(compared.len());
        let mut compared_keys = Vec::with_capacity(compared.len());
        for (kernel, current, generic_selector, legacy_no_element) in compared {
            kernel_expressions.push(kernel);
            current_expressions.push(current);
            generic_selector_expressions.push(generic_selector);
            legacy_no_element_expressions.push(legacy_no_element);
        }
        compared_keys.extend(owner.expressions.iter().filter_map(|(stable_key, _)| {
            (!owner.generic_formal_reads.contains(stable_key)
                && !owner.detached_generic_reads.contains(stable_key)
                && !owner.structured_delimiter_dependents.contains(stable_key)
                && !owner.record_spread_dependents.contains(stable_key))
            .then_some(stable_key)
        }));
        let (kernel_result, mut kernel_expressions) =
            alpha_normalize_owner(&owner.result, kernel_expressions);
        let (current_result, mut current_expressions) =
            alpha_normalize_owner(&current_result, current_expressions);
        let result_exact = kernel_result == current_result;
        let result_matches = result_exact
            || flow_matches_current_or_legacy_render_projection(&kernel_result, &current_result)
            || (owner.exported_as_public_child
                && legacy_public_child_narrowing_matches(&kernel_result, &current_result))
            || (generic_selector_result
                && legacy_generic_selector_member_matches(&kernel_result, &current_result))
            || (legacy_no_element_result
                && legacy_no_element_widening_matches(&kernel_result, &current_result));
        if compare_result {
            assert!(
                result_matches,
                "{context} owner result mismatch (direct public child count {}): {}",
                owner.public_child_owner_fields.len(),
                first_flow_difference(&kernel_result, &current_result)
            );
        }
        if !compare_result || !result_exact {
            // A known lossy legacy result (for example a kind-only render
            // surface) does not expose the same alpha namespace as the dense
            // result. Re-normalize only the still-comparable expression rows;
            // their cross-row correlations remain strict.
            let neutral = FlowType {
                mode: FlowMode::Absent,
                ty: Type::Absent,
            };
            kernel_expressions = alpha_normalize_owner(&neutral, kernel_expressions).1;
            current_expressions = alpha_normalize_owner(&neutral, current_expressions).1;
        }
        assert_eq!(
            kernel_expressions.len(),
            current_expressions.len(),
            "{context} expression count mismatch"
        );
        for (index, (kernel, current)) in kernel_expressions
            .iter()
            .zip(&current_expressions)
            .enumerate()
        {
            assert!(
                flow_matches_current_or_legacy_render_projection(kernel, current)
                    || (generic_selector_expressions[index]
                        && legacy_generic_selector_member_matches(kernel, current))
                    || (legacy_no_element_expressions[index]
                        && legacy_no_element_widening_matches(kernel, current)),
                "{context} expression {index} ({:#?}) mismatch: {}",
                compared_keys[index],
                first_flow_difference(kernel, current)
            );
        }
    }

    fn checked_public_child_composed_result(
        owner: &KernelOwnerOracleEntry,
        mut result: FlowType,
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        context: &str,
    ) -> FlowType {
        if owner.public_child_owner_fields.is_empty() {
            return result;
        }
        let Type::Object(shape) = result.ty.clone() else {
            panic!(
                "{context} has direct public child fields but its public result is not an object"
            );
        };
        let mut shape = shape.into_owned();
        for ((name, child_owner), (kernel_name, kernel_child)) in owner
            .public_child_owner_fields
            .iter()
            .zip(&owner.public_child_kernel_fields)
        {
            assert_eq!(name, kernel_name, "{context} child field authority drifted");
            let child = checked_public_owner_result(checked, project, child_owner).unwrap_or_else(|| {
                panic!(
                    "{context} direct child field `{name}` has no checked public owner result: {child_owner:#?}"
                )
            });
            assert!(
                child == *kernel_child
                    || legacy_public_child_narrowing_matches(kernel_child, &child),
                "{context} direct child field `{name}` is neither exact nor a checked legacy narrowing: {}",
                first_flow_difference(kernel_child, &child)
            );
            let Some(field) = shape.fields.get_mut(name) else {
                panic!(
                    "{context} checked public object omits direct child field `{name}` from {child_owner:#?}"
                );
            };
            *field = kernel_child.ty.clone();
        }
        result.ty = Type::Object(shape.into());
        result
    }

    fn first_flow_difference(kernel: &FlowType, current: &FlowType) -> String {
        if kernel.mode != current.mode {
            return format!(
                "flow mode differs: kernel={:?}, current={:?}",
                kernel.mode, current.mode
            );
        }
        first_type_difference("$", &kernel.ty, &current.ty)
            .unwrap_or_else(|| "types differ only after legacy projection rules".to_owned())
    }

    fn first_type_difference(path: &str, kernel: &Type, current: &Type) -> Option<String> {
        if kernel == current {
            return None;
        }
        match (kernel, current) {
            (Type::VariantSet(kernel), Type::VariantSet(current)) => {
                if kernel.len() != current.len() {
                    return Some(format!(
                        "{path} variant count differs: kernel={kernel:?}, current={current:?}"
                    ));
                }
                kernel
                    .iter()
                    .zip(current.iter())
                    .find_map(|(kernel, current)| match (kernel, current) {
                        (Variant::Tag(kernel), Variant::Tag(current)) if kernel == current => None,
                        (
                            Variant::Tagged {
                                tag: kernel_tag,
                                fields: kernel_fields,
                            },
                            Variant::Tagged {
                                tag: current_tag,
                                fields: current_fields,
                            },
                        ) if kernel_tag == current_tag => first_type_difference(
                            &format!("{path}<{kernel_tag}>"),
                            &Type::Object(kernel_fields.clone()),
                            &Type::Object(current_fields.clone()),
                        ),
                        _ => Some(format!(
                            "{path} variant differs: kernel={kernel:?}, current={current:?}"
                        )),
                    })
            }
            (Type::Object(kernel), Type::Object(current)) => {
                if kernel.open != current.open {
                    return Some(format!(
                        "{path} openness differs: kernel={}, current={}",
                        kernel.open, current.open
                    ));
                }
                for name in kernel.fields.keys().chain(current.fields.keys()) {
                    match (kernel.fields.get(name), current.fields.get(name)) {
                        (Some(kernel), Some(current)) => {
                            if let Some(difference) =
                                first_type_difference(&format!("{path}.{name}"), kernel, current)
                            {
                                return Some(difference);
                            }
                        }
                        (Some(_), None) => {
                            return Some(format!("{path}.{name} exists only in kernel"));
                        }
                        (None, Some(_)) => {
                            return Some(format!("{path}.{name} exists only in current"));
                        }
                        (None, None) => unreachable!(),
                    }
                }
                (kernel.field_order != current.field_order).then(|| {
                    format!(
                        "{path} field order differs: kernel={:?}, current={:?}",
                        kernel.field_order, current.field_order
                    )
                })
            }
            (Type::List(kernel), Type::List(current)) => {
                first_type_difference(&format!("{path}[]"), kernel, current)
            }
            (Type::Set(kernel), Type::Set(current)) => {
                first_type_difference(&format!("{path}{{}}"), kernel, current)
            }
            (
                Type::Map {
                    key: kernel_key,
                    value: kernel_value,
                },
                Type::Map {
                    key: current_key,
                    value: current_value,
                },
            ) => first_type_difference(&format!("{path}.key"), kernel_key, current_key).or_else(
                || first_type_difference(&format!("{path}.value"), kernel_value, current_value),
            ),
            (Type::Union(kernel), Type::Union(current)) => {
                if kernel.len() != current.len() {
                    return Some(format!(
                        "{path} union length differs: kernel={}, current={}",
                        kernel.len(),
                        current.len()
                    ));
                }
                kernel
                    .iter()
                    .zip(current)
                    .enumerate()
                    .find_map(|(index, (kernel, current))| {
                        first_type_difference(&format!("{path}|{index}"), kernel, current)
                    })
            }
            (
                Type::Function {
                    args: kernel_args,
                    result: kernel_result,
                },
                Type::Function {
                    args: current_args,
                    result: current_result,
                },
            ) => {
                if kernel_args.len() != current_args.len() {
                    return Some(format!(
                        "{path} function arity differs: kernel={}, current={}",
                        kernel_args.len(),
                        current_args.len()
                    ));
                }
                kernel_args
                    .iter()
                    .zip(current_args)
                    .enumerate()
                    .find_map(|(index, (kernel, current))| {
                        first_type_difference(&format!("{path}.arg{index}"), kernel, current)
                    })
                    .or_else(|| {
                        (kernel_result != current_result).then(|| {
                            format!(
                                "{path}.result differs: kernel={kernel_result:?}, current={current_result:?}"
                            )
                        })
                    })
            }
            _ => Some(format!(
                "{path} differs: kernel={kernel:?}, current={current:?}"
            )),
        }
    }

    /// The compatibility-assembled checked image can retain an initial-state
    /// slice where the owner interface and the dense kernel retain later HOLD
    /// epochs. Accept only that exact structural narrowing: record shape and
    /// ordering stay identical, and every legacy union/tag member must occur
    /// in the kernel authority.
    fn legacy_public_child_narrowing_matches(kernel: &FlowType, current: &FlowType) -> bool {
        fn variant_matches(kernel: &Variant, current: &Variant) -> bool {
            match (kernel, current) {
                (Variant::Tag(kernel), Variant::Tag(current)) => kernel == current,
                (
                    Variant::Tagged {
                        tag: kernel_tag,
                        fields: kernel_fields,
                    },
                    Variant::Tagged {
                        tag: current_tag,
                        fields: current_fields,
                    },
                ) => {
                    kernel_tag == current_tag
                        && type_matches(
                            &Type::Object(kernel_fields.clone()),
                            &Type::Object(current_fields.clone()),
                        )
                }
                _ => false,
            }
        }

        fn type_matches(kernel: &Type, current: &Type) -> bool {
            if kernel == current {
                return true;
            }
            match (kernel, current) {
                (Type::VariantSet(kernel), Type::VariantSet(current)) => current
                    .iter()
                    .all(|current| kernel.iter().any(|kernel| variant_matches(kernel, current))),
                (Type::Object(kernel), Type::Object(current)) => {
                    kernel.open == current.open
                        && kernel.field_order == current.field_order
                        && kernel.fields.len() == current.fields.len()
                        && kernel.fields.iter().all(|(name, kernel)| {
                            current
                                .fields
                                .get(name)
                                .is_some_and(|current| type_matches(kernel, current))
                        })
                }
                (Type::List(kernel), Type::List(current))
                | (Type::Set(kernel), Type::Set(current)) => type_matches(kernel, current),
                (
                    Type::Map {
                        key: kernel_key,
                        value: kernel_value,
                    },
                    Type::Map {
                        key: current_key,
                        value: current_value,
                    },
                ) => {
                    type_matches(kernel_key, current_key)
                        && type_matches(kernel_value, current_value)
                }
                (Type::Union(kernel), Type::Union(current)) => current
                    .iter()
                    .all(|current| kernel.iter().any(|kernel| type_matches(kernel, current))),
                _ => false,
            }
        }

        kernel.mode == current.mode
            && kernel.ty != current.ty
            && type_matches(&kernel.ty, &current.ty)
    }

    fn flow_matches_current_or_legacy_render_projection(
        kernel: &FlowType,
        current: &FlowType,
    ) -> bool {
        kernel == current
            || (kernel.mode == current.mode
                && legacy_kind_only_render_projection_matches(&kernel.ty, &current.ty))
    }

    fn legacy_generic_selector_member_matches(kernel: &FlowType, current: &FlowType) -> bool {
        kernel.mode == current.mode
            && (matches!(&current.ty, Type::Unknown)
                || legacy_generic_selector_type_matches(&kernel.ty, &current.ty))
    }

    fn legacy_no_element_widening_matches(kernel: &FlowType, current: &FlowType) -> bool {
        kernel.mode == current.mode
            && boon_checked::resolved_type_is_assignable_to(&current.ty, &kernel.ty)
    }

    fn legacy_generic_selector_type_matches(kernel: &Type, current: &Type) -> bool {
        if kernel == current {
            return true;
        }
        // The legacy public/result surface can retain a wider render kind or
        // omit fields that the occurrence residual proves. Accept only the
        // standard checked assignability direction: dense actual -> legacy
        // expected. This stays scoped to known generic-selector/call cones.
        if boon_checked::resolved_type_is_assignable_to(kernel, current) {
            return true;
        }
        if let Type::Union(members) = kernel
            && let Some(widened) = members
                .iter()
                .cloned()
                .reduce(|left, right| boon_checked::widen_structural_type(&left, &right))
            && legacy_generic_selector_type_matches(&widened, current)
        {
            return true;
        }
        match (kernel, current) {
            // A generic WHEN's principal surface intentionally owns a broad
            // union, while each compiled invocation slices one selector arm.
            // The legacy checker numbers those arm-local alphas by a different
            // traversal and its structural widening can replace a placeholder
            // arm with a concrete sibling. Exact occurrence calls remain
            // strict; only the already-marked generic selector cone treats an
            // unresolved kernel alpha as that legacy schematic member.
            (Type::Var(_), _) => true,
            (Type::Union(kernel), Type::Union(current)) => current.iter().all(|current| {
                kernel
                    .iter()
                    .any(|kernel| legacy_generic_selector_type_matches(kernel, current))
            }),
            (Type::Union(kernel), Type::Object(current)) => {
                current.fields.iter().all(|(name, current_field)| {
                    let projected = kernel
                        .iter()
                        .filter_map(|member| {
                            let Type::Object(shape) = member else {
                                return None;
                            };
                            shape.fields.get(name).cloned()
                        })
                        .collect::<Vec<_>>();
                    projected.len() == kernel.len()
                        && legacy_generic_selector_type_matches(
                            &boon_checked::canonical_union_type(projected),
                            current_field,
                        )
                })
            }
            (Type::Union(kernel), current) => kernel
                .iter()
                .any(|kernel| legacy_generic_selector_type_matches(kernel, current)),
            (Type::Object(kernel), Type::Object(current))
                if kernel.open == current.open && current.fields.len() <= kernel.fields.len() =>
            {
                current.fields.iter().all(|(name, current)| {
                    kernel
                        .fields
                        .get(name)
                        .is_some_and(|kernel| legacy_generic_selector_type_matches(kernel, current))
                })
            }
            (Type::List(kernel), Type::List(current)) | (Type::Set(kernel), Type::Set(current)) => {
                legacy_generic_selector_type_matches(kernel, current)
            }
            (
                Type::Map {
                    key: kernel_key,
                    value: kernel_value,
                },
                Type::Map {
                    key: current_key,
                    value: current_value,
                },
            ) => {
                legacy_generic_selector_type_matches(kernel_key, current_key)
                    && legacy_generic_selector_type_matches(kernel_value, current_value)
            }
            _ => false,
        }
    }

    fn legacy_kind_only_render_projection_matches(kernel: &Type, current: &Type) -> bool {
        if let (Type::List(kernel), Type::List(current)) = (kernel, current) {
            return legacy_kind_only_render_projection_matches(kernel, current);
        }
        let (Type::Object(kernel), Type::Object(current)) = (kernel, current) else {
            return false;
        };
        if current.open || current.field_order.as_ref() != ["kind"] || current.fields.len() != 1 {
            return false;
        }
        let Some(current_kind) = current.fields.get("kind") else {
            return false;
        };
        let Some(kernel_kind) = kernel.fields.get("kind") else {
            return false;
        };
        render_kind_refines_legacy_base(kernel_kind, current_kind)
    }

    fn render_kind_refines_legacy_base(kernel: &Type, current: &Type) -> bool {
        let Some(kernel_tags) = render_constructor_tags(kernel) else {
            return false;
        };
        let Some(current_tags) = render_constructor_tags(current) else {
            return false;
        };
        kernel_tags.is_subset(&current_tags)
    }

    fn render_constructor_tags(ty: &Type) -> Option<BTreeSet<&str>> {
        let Type::VariantSet(variants) = ty else {
            return None;
        };
        let tags = variants
            .iter()
            .map(|variant| {
                let Variant::Tag(tag) = variant else {
                    return None;
                };
                matches!(
                    tag.as_str(),
                    "Block"
                        | "Button"
                        | "Checkbox"
                        | "Document"
                        | "EmbeddedMedia"
                        | "EmbeddedProgram"
                        | "Label"
                        | "Link"
                        | "MapViewport"
                        | "Paragraph"
                        | "Row"
                        | "Scene"
                        | "Stack"
                        | "Text"
                        | "TextInput"
                )
                .then_some(tag.as_str())
            })
            .collect::<Option<BTreeSet<_>>>()?;
        (!tags.is_empty()).then_some(tags)
    }

    fn checked_public_owner_result(
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        owner: &StableCheckOwnerKey,
    ) -> Option<FlowType> {
        let StableCheckOwnerKey::Item(owner) = owner else {
            return None;
        };
        let entry = project
            .item_index()
            .owners()
            .find(|entry| entry.owner_key == *owner)?;
        checked_public_statement_result(checked, project, entry.statement_id)
    }

    fn checked_public_statement_result(
        checked: &CheckedProgramFields,
        project: &ProjectSyntaxSnapshot,
        statement_id: usize,
    ) -> Option<FlowType> {
        let statement = checked
            .statements
            .get(project.statement_slot(statement_id)?)?;
        let declaration = match statement.kind {
            CheckedStatementKind::Function { declaration }
            | CheckedStatementKind::Field { declaration } => declaration,
            CheckedStatementKind::Source {
                declaration: Some(declaration),
                ..
            }
            | CheckedStatementKind::Hold {
                declaration: Some(declaration),
                ..
            }
            | CheckedStatementKind::List {
                declaration: Some(declaration),
                ..
            } => declaration,
            _ => return None,
        };
        checked
            .declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .map(|declaration| match &declaration.flow_type.ty {
                Type::Function { result, .. } => (**result).clone(),
                _ => declaration.flow_type.clone(),
            })
    }

    #[test]
    fn parsed_owner_kernel_matches_current_checked_rows() {
        let source = concat!(
            "rows: LIST {\n",
            "    [kind: Header, file: TEXT { a }]\n",
            "    [kind: Empty, file: TEXT { b }]\n",
            "}\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parsed project snapshot");
        let oracle = kernel_owner_oracle(&project);
        let [owner] = oracle.supported.as_ref() else {
            panic!(
                "fixture must produce one supported owner: {:#?}",
                oracle.unsupported
            )
        };
        let [(unsupported_owner, reason)] = oracle.unsupported.as_ref() else {
            panic!(
                "fixture must leave only its declaration-less unit root unsupported: {:#?}",
                oracle.unsupported
            )
        };
        assert!(matches!(
            unsupported_owner,
            StableCheckOwnerKey::UnitRoot(_)
        ));
        assert_eq!(reason, "owner has no public declaration");

        let parsed = parse_source("app/RUN.bn", source).expect("parsed current fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            checked.report.diagnostics.is_empty(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let declaration = checked
            .declarations
            .iter()
            .find(|declaration| {
                declaration.kind == CheckedDeclarationKind::List && declaration.name == "rows"
            })
            .expect("checked rows declaration");
        assert_eq!(owner.result, declaration.flow_type);

        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        for (stable, flow_type) in owner.expressions.iter() {
            assert_eq!(
                Some(flow_type),
                checked_by_stable_key.get(stable),
                "kernel/current expression mismatch at {stable:#?}"
            );
        }
        assert_eq!(
            owner
                .result_expression
                .as_ref()
                .and_then(|result| checked_by_stable_key.get(result)),
            Some(&owner.result)
        );
        assert!(oracle.work.operations > 0);
        assert!(oracle.work.activations < oracle.work.operations.saturating_mul(8));
    }

    #[test]
    fn dynamic_text_templates_keep_dependencies_but_publish_text() {
        let source = concat!(
            "FUNCTION marker(value) {\n",
            "    TEXT { M {value} }\n",
            "}\n",
            "result: marker(value: TEXT { one })\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse dynamic text-template fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "dynamic templates must not prune their owner graph: {:#?}",
            oracle.unsupported
        );
        let marker = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["marker"]))
            })
            .expect("template function compiles");
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .expect("template call compiles");
        assert_eq!(marker.result.ty, Type::Text);
        assert_eq!(result.result.ty, Type::Text);
        assert!(
            oracle.work.operations >= 2,
            "the interpolated value remains an authored dependency"
        );
    }

    #[test]
    fn unique_nested_root_values_are_static_callable_captures() {
        let source = concat!(
            "store: [\n",
            "    elements: [fire: True]\n",
            "]\n",
            "FUNCTION read_fire() {\n",
            "    elements.fire\n",
            "}\n",
            "result: read_fire()\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unique nested-root capture fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["read_fire"]))
            })
            .unwrap_or_else(|| {
                panic!(
                    "unique nested root must be captured by exact owner: {:#?}",
                    oracle.unsupported
                )
            });
        let result = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["result"]))
            })
            .expect("capturing call compiles");
        let expected = Type::VariantSet(vec![Variant::Tag("True".to_owned())].into());
        assert_eq!(function.result.ty, expected);
        assert_eq!(result.result.ty, expected);
    }

    #[test]
    fn direct_list_builtins_accept_the_named_list_input() {
        let source = concat!(
            "chunks:\n",
            "    List/chunk(\n",
            "        list: LIST {\n",
            "            1\n",
            "            2\n",
            "        }\n",
            "        size: 1\n",
            "    )\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse direct list builtin fixture");
        let oracle = kernel_owner_oracle(&project);
        let chunks = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["chunks"])))
            .unwrap_or_else(|| panic!("direct List/chunk must compile: {:#?}", oracle.unsupported));
        assert_eq!(
            chunks.result.ty,
            Type::List(Type::shared(Type::object(
                ObjectShape::from_ordered_fields(
                    [
                        ("label".to_owned(), Type::Text),
                        ("items".to_owned(), Type::List(Type::shared(Type::Number)),),
                    ],
                    false,
                )
            )))
        );
    }

    #[test]
    fn parsed_calls_compose_fresh_parameter_frames_without_owner_dispatch() {
        let source = concat!(
            "FUNCTION box(value) {\n",
            "    [value: value]\n",
            "}\n",
            "number_box: box(value: 1)\n",
            "text_box: box(value: TEXT { text })\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse call-composition fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native call-composition fixture");
        let oracle = kernel_owner_oracle(&project);
        let function_owner = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function))
            })
            .expect("parameterized function is compiled into the dense component");
        assert_owner_matches_current(
            function_owner,
            &checked_by_stable_key,
            &checked,
            &project,
            "call-composition function",
        );
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number_box" | "text_box")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            results["number_box"],
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Number)],
                false,
            ))
        );
        assert_eq!(
            results["text_box"],
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Text)],
                false,
            ))
        );
    }

    #[test]
    fn infix_residuals_constrain_operands_and_publish_fixed_results() {
        let source = concat!(
            "FUNCTION numerator(value) {\n",
            "    value + 2\n",
            "}\n",
            "FUNCTION digits(value) {\n",
            "    numerator(value: value) / 3\n",
            "}\n",
            "sum: 1 + 2\n",
            "ordered: 1 <= 2\n",
            "same: Left == Right\n",
            "answer: digits(value: 9)\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse infix fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native infix fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "infix residual",
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "sum" | "ordered" | "same" | "answer")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let boolean = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        assert_eq!(results["sum"], Type::Number);
        assert_eq!(results["answer"], Type::Number);
        assert_eq!(results["ordered"], boolean);
        assert_eq!(results["same"], boolean);
    }

    #[test]
    fn pure_builtin_calls_compile_to_fixed_residual_equations() {
        let source = concat!(
            "FUNCTION format(value) {\n",
            "    value |> Number/to_text() |> Text/trim()\n",
            "}\n",
            "text: format(value: 9)\n",
            "items:\n",
            "    LIST {\n",
            "        1\n",
            "        2\n",
            "    }\n",
            "count: items |> List/length()\n",
            "empty: TEXT { value } |> Text/is_empty()\n",
            "minimum: Number/min(left: 1, right: 2)\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse pure builtin fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native pure builtin fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "pure builtin residual",
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "text" | "count" | "empty" | "minimum")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            results.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "count".to_owned(),
                "empty".to_owned(),
                "minimum".to_owned(),
                "text".to_owned(),
            ]),
            "every pure builtin result must compile: {:#?}",
            oracle.unsupported
        );
        let boolean = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        assert_eq!(results["text"], Type::Text);
        assert_eq!(results["count"], Type::Number);
        assert_eq!(results["empty"], boolean);
        assert_eq!(results["minimum"], Type::Number);
    }

    #[test]
    fn singleton_selectors_choose_one_compiled_match_arm() {
        let source = concat!(
            "FUNCTION choose(kind) {\n",
            "    kind |> WHEN {\n",
            "        A => SelectedA\n",
            "        B => SelectedB\n",
            "        __ => Fallback\n",
            "    }\n",
            "}\n",
            "number: choose(kind: A)\n",
            "text: choose(kind: B)\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse selector fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native selector fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            if matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function))
            {
                continue;
            }
            let context = format!("selector residual {:#?}", owner.owner);
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                &context,
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number" | "text")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function)))
            .expect("selector function is compiled");
        assert_eq!(
            function.result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("Fallback".to_owned()),
                    Variant::Tag("SelectedA".to_owned()),
                    Variant::Tag("SelectedB".to_owned()),
                ]
                .into(),
            )
        );
        assert_eq!(
            results["number"],
            Type::VariantSet(vec![Variant::Tag("SelectedA".to_owned())].into())
        );
        assert_eq!(
            results["text"],
            Type::VariantSet(vec![Variant::Tag("SelectedB".to_owned())].into())
        );
    }

    #[test]
    fn multiline_match_records_compile_as_structural_arm_outputs() {
        let source = concat!(
            "FUNCTION choose(kind) {\n",
            "    kind |> WHEN {\n",
            "        Record => [\n",
            "            value: 1\n",
            "            label: TEXT { chosen }\n",
            "        ]\n",
            "        __ => [\n",
            "            value: 2\n",
            "        ]\n",
            "    }\n",
            "}\n",
            "selected: choose(kind: Record)\n",
        );
        let checked = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_text(
            "app/RUN.bn",
            source,
            crate::ProgramRole::Client,
        ))
        .expect("check multiline record fixture through the owner pipeline");
        assert!(
            checked.output.report.diagnostics.is_empty(),
            "current checker diagnostics: {:#?}",
            checked.output.report.diagnostics
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native multiline record fixture");
        let checked = checked
            .output
            .checked_program_fields()
            .expect("multiline record diagnostics own checked fields");
        let mut checked_by_stable_key = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let view = project
                .owner_view(&owner)
                .expect("multiline record owner has a view");
            for (expression, stable_key) in view.expressions().zip(view.stable_expression_keys()) {
                let Some(flow_type) = project
                    .expression_slot(expression.id)
                    .and_then(|slot| checked.expressions.get(slot))
                    .map(|expression| expression.flow_type.clone())
                else {
                    continue;
                };
                checked_by_stable_key.insert(stable_key, flow_type);
            }
        }
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "multiline record residual",
            );
        }
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["choose"])))
            .expect("generic multiline selector function compiles");
        assert_eq!(
            function.result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            )),
            "the generic principal must structurally widen compatible record arms"
        );
        let selected = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["selected"])))
            .unwrap_or_else(|| {
                panic!(
                    "selected multiline record must compile: {:#?}",
                    oracle.unsupported
                )
            });
        assert_eq!(
            selected.result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn record_spreads_compile_as_ordered_residual_overlays() {
        let source = concat!(
            "FUNCTION base() {\n",
            "    [family: 1, size: 12]\n",
            "}\n",
            "style: [\n",
            "    ...base()\n",
            "    family: TEXT { Mono }\n",
            "    color: TEXT { #ffffff }\n",
            "]\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse record-spread fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native record-spread fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "record-spread residual",
            );
        }
        let style = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names.as_ref() == ["style"])))
            .unwrap_or_else(|| panic!("style spread must compile: {:#?}", oracle.unsupported));
        assert_eq!(
            style.result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("family".to_owned(), Type::Text),
                    ("size".to_owned(), Type::Number),
                    ("color".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn render_constructors_compile_named_fields_and_kind_without_abi_replay() {
        let source = concat!(
            "FUNCTION make_button(label) {\n",
            "    Scene/Element/button(\n",
            "        element: [event: [press: False], hovered: False]\n",
            "        label: label\n",
            "    )\n",
            "}\n",
            "button: make_button(label: TEXT { Go })\n",
        );
        let checked = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_text(
            "app/RUN.bn",
            source,
            crate::ProgramRole::Client,
        ))
        .expect("check render residual fixture through the owner pipeline");
        assert!(
            checked.output.report.diagnostics.is_empty(),
            "current checker diagnostics: {:#?}",
            checked.output.report.diagnostics
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native render residual fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "every declared render owner must compile: {:#?}",
            oracle.unsupported
        );
        // The independent dense checker still exposes the constructor's
        // kind-only base record here. The production owner ABI already makes
        // supplied fields part of the result contract, so assert that contract
        // directly instead of preserving the lossy oracle surface.
        let button = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["button"])))
            .unwrap_or_else(|| panic!("render result must be supported: {:#?}", oracle.unsupported));
        let Type::Object(shape) = &button.result.ty else {
            panic!("render constructor result must be an object")
        };
        assert_eq!(shape.field_order, ["element", "label", "kind"]);
        assert!(matches!(shape.fields["element"], Type::Object(_)));
        assert_eq!(shape.fields["label"], Type::Text);
        assert_eq!(
            shape.fields["kind"],
            Type::VariantSet(vec![Variant::Tag("Button".to_owned())].into())
        );
    }

    #[test]
    fn explicit_pass_contexts_compose_as_fresh_call_frames() {
        let source = concat!(
            "FUNCTION read() {\n",
            "    PASSED.value\n",
            "}\n",
            "FUNCTION inherited() {\n",
            "    read()\n",
            "}\n",
            "number: inherited(PASS: [value: 1])\n",
            "text: inherited(PASS: [value: TEXT { text }])\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse explicit PASS fixture");
        let oracle = kernel_owner_oracle(&project);
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number" | "text")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(results["number"], Type::Number);
        assert_eq!(results["text"], Type::Text);
    }

    #[test]
    fn repeated_passed_reads_share_one_projection_alpha() {
        let source = concat!(
            "FUNCTION pair() {\n",
            "    [\n",
            "        event: [\n",
            "            first: PASSED.store.elements.first\n",
            "            repeated: PASSED.store.elements.repeated\n",
            "            third: PASSED.store.elements.third\n",
            "        ]\n",
            "        second: PASSED.store.elements.repeated\n",
            "    ]\n",
            "}\n",
            "unrelated: 1\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse repeated PASSED fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Function)))
            .unwrap_or_else(|| panic!("pair function must compile: {:#?}", oracle.unsupported));
        let Type::Object(shape) = &function.result.ty else {
            panic!("pair function must return an object")
        };
        let Type::Object(event) = &shape.fields["event"] else {
            panic!("pair function event must be an object")
        };
        assert_eq!(event.fields["repeated"], shape.fields["second"]);
    }

    #[test]
    fn block_bindings_compile_as_lexical_alias_edges() {
        let source = concat!(
            "FUNCTION duplicate(value) {\n",
            "    BLOCK {\n",
            "        first: value\n",
            "        second: first\n",
            "        [left: first, right: second]\n",
            "    }\n",
            "}\n",
            "number: duplicate(value: 1)\n",
            "text: duplicate(value: TEXT { value })\n",
        );
        let parsed = parse_source("app/RUN.bn", source).expect("parse BLOCK lexical fixture");
        let checked = boon_typecheck::check_program(&parsed);
        assert!(
            !checked.report.has_errors(),
            "current checker diagnostics: {:#?}",
            checked.report.diagnostics
        );
        let (checked, _) = checked.program.expect("fixture checks").into_parts();
        let checked_by_stable_key = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| {
                Some((
                    parsed.stable_expression_key(expression.id)?,
                    checked.expressions.get(expression.id)?.flow_type.clone(),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse unit-native BLOCK lexical fixture");
        let oracle = kernel_owner_oracle(&project);
        for owner in &oracle.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                &checked,
                &project,
                "BLOCK lexical residual",
            );
        }
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "number" | "text")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let pair = |ty: Type| {
            Type::object(ObjectShape::from_ordered_fields(
                [("left".to_owned(), ty.clone()), ("right".to_owned(), ty)],
                false,
            ))
        };
        assert_eq!(results["number"], pair(Type::Number));
        assert_eq!(results["text"], pair(Type::Text));
    }

    #[test]
    fn multiline_record_siblings_are_visible_inside_a_later_hold_field() {
        let source = concat!(
            "FUNCTION stateful(value) {\n",
            "    [\n",
            "        controls: [fire: value]\n",
            "        state:\n",
            "            False |> HOLD state {\n",
            "                controls.fire |> THEN { True }\n",
            "            }\n",
            "    ]\n",
            "}\n",
            "result: stateful(value: 1)\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse multiline sibling fixture");
        let oracle = kernel_owner_oracle(&project);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "every declared multiline sibling owner must compile: {:#?}",
            oracle.unsupported
        );
        let result = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["result"])))
            .expect("stateful call result must compile");
        let Type::Object(shape) = &result.result.ty else {
            panic!("stateful result must be an object")
        };
        assert_eq!(
            shape.fields["state"],
            Type::VariantSet(
                vec![
                    Variant::Tag("False".to_owned()),
                    Variant::Tag("True".to_owned())
                ]
                .into()
            )
        );
    }

    #[test]
    fn hold_update_statements_read_the_private_state_capability() {
        let source = concat!(
            "FUNCTION toggle(trigger) {\n",
            "    False |> HOLD state {\n",
            "        trigger |> THEN {\n",
            "            state |> WHEN {\n",
            "                False => True\n",
            "                True => False\n",
            "            }\n",
            "        }\n",
            "    }\n",
            "}\n",
            "result: toggle(trigger: True)\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse HOLD self-read fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["toggle"])))
            .unwrap_or_else(|| panic!("HOLD self-read must compile: {:#?}", oracle.unsupported));
        let expected = Type::VariantSet(
            vec![
                Variant::Tag("False".to_owned()),
                Variant::Tag("True".to_owned()),
            ]
            .into(),
        );
        assert_eq!(function.result.ty, expected);
    }

    #[test]
    fn hold_capability_reaches_nested_pipe_callback_arguments() {
        let source = concat!(
            "FUNCTION preserve(rows) {\n",
            "    False |> HOLD state {\n",
            "        rows\n",
            "        |> List/map(item, new: item |> THEN { state })\n",
            "        |> List/latest()\n",
            "    }\n",
            "}\n",
            "result: preserve(rows: LIST { True })\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse nested HOLD callback fixture");
        let oracle = kernel_owner_oracle(&project);
        let function = oracle
            .supported
            .iter()
            .find(|owner| matches!(&owner.owner, StableCheckOwnerKey::Item(key) if key.item_route.segments().last().is_some_and(|segment| segment.names == ["preserve"])))
            .unwrap_or_else(|| {
                panic!(
                    "HOLD capability must reach pipe arguments: {:#?}",
                    oracle.unsupported
                )
            });
        assert!(
            !matches!(function.result.ty, Type::UnresolvedShape { .. }),
            "the nested callback must retain a resolved HOLD dependency"
        );
    }

    #[test]
    fn parsed_value_reads_share_one_component_without_owner_reconstruction() {
        let source = concat!(
            "base: [\n",
            "    value: 1\n",
            "    label: TEXT { base }\n",
            "]\n",
            "copy: base.value\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse cross-owner value-read fixture");
        let oracle = kernel_owner_oracle(&project);
        let results = oracle
            .supported
            .iter()
            .filter_map(|owner| {
                let StableCheckOwnerKey::Item(key) = &owner.owner else {
                    return None;
                };
                let name = key.item_route.segments().last()?.names.first()?;
                matches!(name.as_str(), "base" | "copy")
                    .then(|| (name.clone(), owner.result.ty.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            results["base"],
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            ))
        );
        assert_eq!(results["copy"], Type::Number);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "both value owners must solve in one component: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn multiline_fields_alias_child_owner_results_without_expression_guessing() {
        let source = concat!(
            "store: [\n",
            "    rows:\n",
            "        LIST {\n",
            "            1\n",
            "            2\n",
            "        }\n",
            "]\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse multiline child-owner fixture");
        let oracle = kernel_owner_oracle(&project);
        let rows = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["rows"]))
            })
            .unwrap_or_else(|| {
                panic!(
                    "multiline rows field must alias its LIST owner result: {:#?}",
                    oracle.unsupported
                )
            });
        assert!(
            rows.result_expression.is_none(),
            "the public multiline field result is a declaration authority, not a guessed expression"
        );
        assert_eq!(rows.result.ty, Type::List(Type::shared(Type::Number)));
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "the store record, rows field, and LIST owner must share one component: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn hold_updates_widen_in_one_component_without_recursive_replay() {
        let source = concat!(
            "state:\n",
            "    NotStarted |> HOLD state {\n",
            "        LATEST {\n",
            "            True |> THEN { WaveformOpened[timescale: TEXT { ns }] }\n",
            "            False |> THEN { Failed }\n",
            "        }\n",
            "    }\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse HOLD residual fixture");
        let oracle = kernel_owner_oracle(&project);
        let state = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Field && segment.names == ["state"]))
            })
            .unwrap_or_else(|| panic!("state field must consume its HOLD result: {:#?}", oracle.unsupported));
        assert_eq!(
            state.result.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("Failed".to_owned()),
                    Variant::Tag("NotStarted".to_owned()),
                    Variant::tagged(
                        "WaveformOpened".to_owned(),
                        ObjectShape::from_ordered_fields(
                            [("timescale".to_owned(), Type::Text)],
                            false,
                        ),
                    ),
                ]
                .into(),
            )
        );
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "the public field and HOLD owner must solve together: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn owner_local_state_reads_compile_as_explicit_lexical_cycles() {
        let source = concat!(
            "state:\n",
            "    0 |> HOLD state {\n",
            "        True |> THEN { state }\n",
            "    }\n",
        );
        let project =
            parse_project_syntax("app/RUN.bn", [("app/RUN.bn".to_owned(), source.to_owned())])
                .expect("parse owner-local state cycle fixture");
        let oracle = kernel_owner_oracle(&project);
        let state = oracle
            .supported
            .iter()
            .find(|owner| {
                matches!(&owner.owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.kind == UnitItemKind::Field && segment.names == ["state"]))
            })
            .unwrap_or_else(|| {
                panic!(
                    "state field must compile its self read as a lexical cycle: {:#?}",
                    oracle.unsupported
                )
            });
        assert_eq!(state.result.ty, Type::Number);
        assert!(
            oracle
                .unsupported
                .iter()
                .all(|(owner, _)| matches!(owner, StableCheckOwnerKey::UnitRoot(_))),
            "no owner-local value read may be rejected: {:#?}",
            oracle.unsupported
        );
    }

    #[test]
    fn real_example_coverage_is_deterministic_and_explicit() {
        for (disk_relative, project_path) in [
            ("../../examples/counter.bn", "examples/counter.bn"),
            ("../../examples/todomvc.bn", "examples/todomvc.bn"),
        ] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(disk_relative);
            let source = fs::read_to_string(&path).expect("read example source");
            let parsed = parse_source(project_path, &source).expect("parse checked example");
            let checked = boon_typecheck::check_program(&parsed);
            assert!(
                !checked.report.has_errors(),
                "current checker diagnostics for {project_path}: {:#?}",
                checked.report.diagnostics
            );
            let (checked, _) = checked.program.expect("example checks").into_parts();
            let checked_by_stable_key = parsed
                .ast
                .expressions
                .iter()
                .filter_map(|expression| {
                    Some((
                        parsed.stable_expression_key(expression.id)?,
                        checked.expressions.get(expression.id)?.flow_type.clone(),
                    ))
                })
                .collect::<BTreeMap<_, _>>();
            let project = parse_project_syntax(project_path, [(project_path.to_owned(), source)])
                .expect("parse example project");
            let source_payloads = boon_typecheck::project_source_payload_abi_types(&project)
                .expect("project closed SOURCE ABI without running the checker");
            let kernel_started = Instant::now();
            let first = kernel_owner_oracle_with_source_payloads(&project, &source_payloads);
            let kernel_elapsed = kernel_started.elapsed();
            let second = kernel_owner_oracle_with_source_payloads(&project, &source_payloads);
            assert_eq!(
                first, second,
                "kernel oracle must be deterministic for {project_path}"
            );
            assert!(
                !first.supported.is_empty(),
                "first kernel slice must cover real owners in {project_path}: {:#?}",
                first.unsupported
            );
            assert_eq!(
                first.supported.len() + first.unsupported.len(),
                project.stable_check_owner_keys().count(),
                "every example owner must be classified explicitly"
            );
            for owner in &first.supported {
                assert_owner_matches_current(
                    owner,
                    &checked_by_stable_key,
                    &checked,
                    &project,
                    &format!("kernel/current {project_path} owner {:#?}", owner.owner),
                );
            }
            assert!(first.work.operations > 0);
            assert!(first.work.activations < first.work.operations.saturating_mul(8));
            if std::env::var_os("BOON_KERNEL_ORACLE_TRACE").is_some() {
                eprintln!(
                    "kernel-oracle {project_path}: supported={}/{} operations={} activations={} mutations={} dynamic_edges={} elapsed_us={}",
                    first.supported.len(),
                    first.supported.len() + first.unsupported.len(),
                    first.work.operations,
                    first.work.activations,
                    first.work.mutations,
                    first.work.dynamic_dependency_edges,
                    kernel_elapsed.as_micros(),
                );
            }
        }
    }

    #[test]
    #[ignore = "directional NovyWave kernel timing probe"]
    fn novywave_kernel_timing_probe() {
        let source_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/novywave/RUN.bn");
        let bundle_started = Instant::now();
        let (entrypoint, units) = crate::compiler_source_project_for_path(&source_path)
            .expect("load NovyWave source bundle");
        let bundle_us = elapsed_us(bundle_started.elapsed());

        let parse_started = Instant::now();
        let project = parse_project_syntax(
            entrypoint.clone(),
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )
        .expect("parse NovyWave unit-native project");
        let parse_us = elapsed_us(parse_started.elapsed());

        let source_abi_started = Instant::now();
        let source_payloads = boon_typecheck::project_source_payload_abi_types(&project)
            .expect("project NovyWave SOURCE ABI without running the checker");
        let source_abi_us = elapsed_us(source_abi_started.elapsed());

        let (report, timings) =
            profile_kernel_owner_oracle_with_source_payloads(&project, &source_payloads);

        if std::env::var_os("BOON_KERNEL_CANDIDATE_ONLY").is_some() {
            if report.supported.is_empty() {
                if let Some((owner, reason)) = report
                    .unsupported
                    .iter()
                    .find(|(_, reason)| reason.starts_with("kernel project solve failed:"))
                    .or_else(|| report.unsupported.first())
                {
                    eprintln!("kernel-novywave first_unsupported_owner={owner:?} reason={reason}");
                }
            }
            let retained_snapshot_total_us = source_abi_us.saturating_add(timings.total_us);
            let candidate_total_us = parse_us.saturating_add(retained_snapshot_total_us);
            eprintln!(
                "kernel-novywave candidate_only=true parity=not_run profile={} bundle_us={} parse_us={} source_abi_us={} retained_snapshot_total_us={} candidate_total_us={} kernel_total_us={} compile_us={} solve_us={} solved_owners={} unsupported_owners={} residual_modules={} residual_frames={} acyclic_residual_frames={} invocation_frames={} direct_result_summaries={} linked_operations={} scheduled_work_items={} acyclic_initial_work_items={} dominant_module_owner={} dominant_module_operations={} dominant_module_frames={} dominant_module_linked_operations={} variables={} activations={} unify_activations={} publish_activations={} projection_activations={} select_activations={} record_activations={} summary_call_activations={} summary_node_evaluations={} mutations={} term_materializations={} term_intern_requests={} term_intern_hits={} term_intern_requests_by_kind={:?} term_intern_hits_by_kind={:?} structural_widen_requests={} structural_widen_hits={} dynamic_edges={}",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                },
                bundle_us,
                parse_us,
                source_abi_us,
                retained_snapshot_total_us,
                candidate_total_us,
                timings.total_us,
                timings.program_compile_us,
                timings.solve_us,
                timings.solved_owners,
                timings.unsupported_owners,
                timings.compile_work.residual_type_modules,
                timings.compile_work.residual_frames,
                timings.compile_work.acyclic_residual_frames,
                timings.compile_work.invocation_frames,
                timings.compile_work.direct_result_summaries,
                timings.compile_work.linked_operations,
                timings.compile_work.scheduled_work_items,
                timings.compile_work.acyclic_initial_operations,
                timings.compile_work.dominant_module_owner,
                timings.compile_work.dominant_module_operations,
                timings.compile_work.dominant_module_frames,
                timings.compile_work.dominant_module_linked_operations,
                report.work.variables,
                report.work.activations,
                report.work.unify_activations,
                report.work.publish_activations,
                report.work.projection_activations,
                report.work.select_activations,
                report.work.record_activations,
                report.work.summary_call_activations,
                report.work.summary_node_evaluations,
                report.work.mutations,
                report.work.term_materializations,
                report.work.term_intern_requests,
                report.work.term_intern_hits,
                report.work.term_intern_requests_by_kind,
                report.work.term_intern_hits_by_kind,
                report.work.structural_widen_requests,
                report.work.structural_widen_hits,
                report.work.dynamic_dependency_edges,
            );
            return;
        }

        // The old compiler runs only as the differential oracle. It is timed
        // outside the candidate parse + ABI + kernel path and contributes no
        // input to the dense solve.
        let oracle_check_started = Instant::now();
        let checked = crate::check_diagnostics_source(crate::CompilerCheckRequest::source_units(
            &entrypoint,
            &units,
            crate::ProgramRole::Client,
        ))
        .expect("check NovyWave differential oracle");
        let oracle_check_us = elapsed_us(oracle_check_started.elapsed());
        assert!(
            checked.output.report.diagnostics.is_empty(),
            "NovyWave timing fixture diagnostics: {:#?}",
            checked.output.report.diagnostics
        );
        let fields = checked
            .output
            .checked_program_fields()
            .expect("NovyWave diagnostics own checked fields");
        assert_eq!(
            report.supported.len() + report.unsupported.len(),
            project.stable_check_owner_keys().count(),
            "every NovyWave owner must be classified"
        );
        assert!(
            !report.supported.is_empty(),
            "NovyWave must exercise at least one dense owner: {:#?}",
            report.unsupported
        );
        assert_eq!(timings.solved_owners, report.supported.len());
        assert_eq!(timings.unsupported_owners, report.unsupported.len());
        let mut checked_by_stable_key = BTreeMap::new();
        for owner in project.stable_check_owner_keys() {
            let view = project
                .owner_view(&owner)
                .expect("NovyWave owner has a view");
            for (expression, stable_key) in view.expressions().zip(view.stable_expression_keys()) {
                let Some(flow_type) = project
                    .expression_slot(expression.id)
                    .and_then(|slot| fields.expressions.get(slot))
                    .map(|expression| expression.flow_type.clone())
                else {
                    continue;
                };
                checked_by_stable_key.insert(stable_key, flow_type);
            }
        }
        if let Some(pattern) = std::env::var_os("BOON_KERNEL_ORACLE_TRACE_OWNER") {
            let pattern = pattern.to_string_lossy();
            for owner in report
                .supported
                .iter()
                .filter(|owner| format!("{:?}", owner.owner).contains(pattern.as_ref()))
            {
                eprintln!(
                    "kernel-owner-trace solved owner={:?} result={:?}",
                    owner.owner, owner.result
                );
                for (index, (stable, flow)) in owner.expressions.iter().enumerate() {
                    let current = checked_by_stable_key.get(stable);
                    let current_mode = current.map(|current| current.mode);
                    eprintln!(
                        "kernel-owner-trace solved node={index} mode={:?} current_mode={current_mode:?} stable={stable:?} type={:?} current_type={:?}",
                        flow.mode,
                        flow.ty,
                        current.map(|current| &current.ty),
                    );
                }
            }
        }
        for owner in &report.supported {
            assert_owner_matches_current(
                owner,
                &checked_by_stable_key,
                fields,
                &project,
                &format!("NovyWave kernel/current owner {:#?}", owner.owner),
            );
        }
        let unsupported_classes = report.unsupported.iter().fold(
            BTreeMap::<String, usize>::new(),
            |mut classes, (_, reason)| {
                let class = unsupported_reason_class(reason);
                *classes.entry(class).or_default() += 1;
                classes
            },
        );
        let candidate_total_us = parse_us
            .saturating_add(source_abi_us)
            .saturating_add(timings.total_us);
        let retained_snapshot_total_us = source_abi_us.saturating_add(timings.total_us);
        let candidate_with_bundle_us = bundle_us.saturating_add(candidate_total_us);
        eprintln!(
            "kernel-novywave profile={} bundle_us={} parse_us={} source_abi_us={} retained_snapshot_total_us={} candidate_total_us={} candidate_with_bundle_us={} oracle_check_us={} legacy_parse_ms={:.3} legacy_typecheck_ms={:.3} kernel_total_us={} owner_projection_us={} direct_projection_us={} dependency_pruning_us={} program_compile_us={} solve_us={} artifact_projection_us={} projected_owners={} solved_owners={} unsupported_owners={} definition_modules={} principal_expressions={} residual_type_modules={} residual_module_operations={} residual_module_terms={} residual_frames={} linked_operations={} scheduled_work_items={} linked_terms={} acyclic_initial_operations={} compiled_call_sites={} invocation_frames={} reused_invocation_frames={} principal_result_reuses={} principal_expression_reuses={} pruned_invocation_expressions={} specialization_plans={} reused_specialization_plans={} max_call_depth={} variables={} operations={} activations={} unify_activations={} publish_activations={} projection_activations={} select_activations={} record_activations={} mutations={} dynamic_edges={}",
            if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            bundle_us,
            parse_us,
            source_abi_us,
            retained_snapshot_total_us,
            candidate_total_us,
            candidate_with_bundle_us,
            oracle_check_us,
            checked.profile.parse_ms,
            checked.profile.typecheck_ms,
            timings.total_us,
            timings.owner_projection_us,
            timings.direct_projection_us,
            timings.dependency_pruning_us,
            timings.program_compile_us,
            timings.solve_us,
            timings.artifact_projection_us,
            timings.projected_owners,
            timings.solved_owners,
            timings.unsupported_owners,
            timings.compile_work.definition_modules,
            timings.compile_work.principal_expressions,
            timings.compile_work.residual_type_modules,
            timings.compile_work.residual_module_operations,
            timings.compile_work.residual_module_terms,
            timings.compile_work.residual_frames,
            timings.compile_work.linked_operations,
            timings.compile_work.scheduled_work_items,
            timings.compile_work.linked_terms,
            timings.compile_work.acyclic_initial_operations,
            timings.compile_work.compiled_call_sites,
            timings.compile_work.invocation_frames,
            timings.compile_work.reused_invocation_frames,
            timings.compile_work.principal_result_reuses,
            timings.compile_work.principal_expression_reuses,
            timings.compile_work.pruned_invocation_expressions,
            timings.compile_work.specialization_plans,
            timings.compile_work.reused_specialization_plans,
            timings.compile_work.max_call_depth,
            report.work.variables,
            report.work.operations,
            report.work.activations,
            report.work.unify_activations,
            report.work.publish_activations,
            report.work.projection_activations,
            report.work.select_activations,
            report.work.record_activations,
            report.work.mutations,
            report.work.dynamic_dependency_edges,
        );
        eprintln!("kernel-novywave unsupported_classes={unsupported_classes:?}");
        eprintln!(
            "kernel-novywave root_blockers={:?}",
            report
                .root_blockers
                .iter()
                .take(16)
                .map(|blocker| (
                    blocker.affected_owners,
                    unsupported_reason_class(&blocker.reason),
                    &blocker.owner,
                ))
                .collect::<Vec<_>>()
        );
        if std::env::var_os("BOON_KERNEL_ORACLE_UNSUPPORTED_TRACE").is_some() {
            for (owner, reason) in &report.unsupported {
                eprintln!("kernel-novywave unsupported owner={owner:?} reason={reason}");
            }
        }
    }

    fn unsupported_reason_class(reason: &str) -> String {
        if reason.starts_with("unresolved top-level value read") {
            return "unresolved_top_level_value".to_owned();
        }
        if reason.starts_with("owner has no direct or structural result") {
            return "owner has no direct or structural result".to_owned();
        }
        if reason.starts_with("ambiguous top-level value read") {
            return "ambiguous_top_level_value".to_owned();
        }
        if reason.contains("needs a lexical equation") {
            return "owner_local_value_read".to_owned();
        }
        if reason.contains("requires an explicit PASS context") {
            return "missing_pass_context".to_owned();
        }
        if let Some(kind) = reason.strip_prefix("unsupported owner node ") {
            let end = kind.find([' ', '{', '(']).unwrap_or(kind.len());
            return format!("unsupported_node:{}", &kind[..end]);
        }
        if reason.starts_with("depends on unsupported owner") {
            return "dependency_pruned".to_owned();
        }
        if reason.starts_with("imports missing expression") {
            return "missing_import".to_owned();
        }
        reason.to_owned()
    }
}
