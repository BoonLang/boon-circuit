use super::{
    CompileProfile, CompiledMachinePlanFromSource, CompilerResult, CompilerSourceUnit, elapsed_ms,
    machine_plan_backend, parse_source_units,
};
use boon_ir::{DistributedCall, ErasedProgram, verify_hidden_identity, verify_static_schedule};
use boon_plan::{
    ApplicationIdentity, DataTypeFieldPlan, DataTypePlan, DataVariantPlan, DistributedCallMode,
    DistributedCallResultPlan, DistributedCallRowBindingPlan, DistributedDeclarationId,
    DistributedEndpointContractPlan, DistributedEndpointId, DistributedEndpointPlan,
    DistributedEventExportPlan, DistributedEventImportPlan, DistributedFunctionExportPlan,
    DistributedGraphIdentityPlan, DistributedGraphPlan, DistributedInvocationArmPlan,
    DistributedValueExportPlan, DistributedValueImportPlan, ExportId, FieldId, ImportId, ListId,
    MigrationPredecessorBinding, PlanError, PlanLocalId, PlanOwner, PlanSourceRouteId,
    PlanStaticOwnerId, ProducerFunctionInstancePlan, ProgramRole, RemoteCallSiteId,
    RemoteCallSitePlan, SourceId, SourcePayloadDescriptor, SourcePayloadField, SourcePayloadSchema,
    SourceRoute, TargetProfile, ValueRef, verify_plan,
};
use boon_typecheck::{
    CheckedCallEntry, CheckedCallableKind, CheckedParameterKind, CheckedProgram,
    ExternalFunctionArgument, ExternalFunctionType, ExternalTypeEnvironment, FlowMode, FlowType,
    FunctionTypeEntry, ObjectShape, Type, TypeCheckReport, Variant,
};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

#[derive(Clone, Debug)]
pub struct DistributedCompilerProgram {
    pub revision: u64,
    pub role: ProgramRole,
    pub source_label: String,
    pub units: Vec<CompilerSourceUnit>,
    pub application: ApplicationIdentity,
    pub schema_version: u64,
    pub migration_predecessors: Vec<MigrationPredecessorBinding>,
}

#[derive(Clone, Debug)]
pub struct CompiledDistributedMachinePlans {
    pub graph: DistributedGraphPlan,
    pub programs: Vec<(ProgramRole, CompiledMachinePlanFromSource)>,
}

impl CompiledDistributedMachinePlans {
    pub fn program(&self, role: ProgramRole) -> Option<&CompiledMachinePlanFromSource> {
        self.programs
            .iter()
            .find_map(|(candidate, compiled)| (*candidate == role).then_some(compiled))
    }

    pub fn into_programs(self) -> Vec<(ProgramRole, CompiledMachinePlanFromSource)> {
        self.programs
    }
}

struct LoweredRole {
    request: DistributedCompilerProgram,
    checked: CheckedProgram,
    ir: ErasedProgram,
    source_unit_count: usize,
    parse_ms: f64,
    lower_ms: f64,
    verify_ms: f64,
}

struct ParsedRole {
    request: DistributedCompilerProgram,
    parsed: boon_parser::ParsedProgram,
    checked: CheckedProgram,
    parse_ms: f64,
}

struct SolvedBundleInterfaces {
    checked: BTreeMap<ProgramRole, CheckedProgram>,
}

#[derive(Clone, Debug)]
struct BundleValueReference {
    consumer_role: ProgramRole,
    producer_role: ProgramRole,
    canonical_path: String,
    local_path: String,
}

#[derive(Clone, Debug)]
struct BundleCallReference {
    consumer_role: ProgramRole,
    producer_role: ProgramRole,
    canonical_function: String,
    local_function: String,
    arguments: Vec<(String, usize)>,
}

#[derive(Clone, Debug, Default)]
struct BundleReferences {
    values: Vec<BundleValueReference>,
    calls: Vec<BundleCallReference>,
}

#[derive(Clone)]
struct EndpointIdentity {
    stable_identity: DistributedDeclarationId,
    endpoint_id: DistributedEndpointId,
}

#[derive(Clone)]
struct ValueLink {
    consumer_role: ProgramRole,
    producer_role: ProgramRole,
    canonical_path: String,
    local_path: String,
    export_stable_identity: DistributedDeclarationId,
    export_id: ExportId,
    import_stable_identity: DistributedDeclarationId,
    import_id: ImportId,
    flow: DistributedReferenceFlow,
    event_source_id: Option<SourceId>,
    event_payload_field: Option<SourcePayloadField>,
    producer_value_ref: ValueRef,
    data_type: DataTypePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DistributedReferenceFlow {
    Current,
    Event,
}

fn distributed_flow_for_value_ref(value: &ValueRef) -> DistributedReferenceFlow {
    match value {
        ValueRef::Source(_) | ValueRef::SourcePayload { .. } => DistributedReferenceFlow::Event,
        ValueRef::State(_)
        | ValueRef::StateProjection { .. }
        | ValueRef::Field(_)
        | ValueRef::List(_)
        | ValueRef::Constant(_)
        | ValueRef::DistributedImport(_) => DistributedReferenceFlow::Current,
    }
}

#[derive(Clone)]
struct FunctionLink {
    producer_role: ProgramRole,
    canonical_function: String,
    local_function: String,
    stable_identity: DistributedDeclarationId,
    export_id: ExportId,
    signature: FunctionTypeEntry,
    result_type: DataTypePlan,
}

#[derive(Clone)]
struct CallLink {
    consumer_role: ProgramRole,
    owner_path: String,
    owner: PlanOwner,
    stable_identity: DistributedDeclarationId,
    call_site_id: RemoteCallSiteId,
    call: DistributedCall,
    function_export_id: ExportId,
    result: DistributedCallResultPlan,
    mode: DistributedCallMode,
}

pub fn compile_distributed_runtime_source_programs(
    programs: &[DistributedCompilerProgram],
    target_profile: TargetProfile,
) -> CompilerResult<CompiledDistributedMachinePlans> {
    validate_bundle_requests(programs)?;
    let requests = programs
        .iter()
        .cloned()
        .map(|program| (program.role, program))
        .collect::<BTreeMap<_, _>>();
    let parsed = requests
        .values()
        .map(parse_role)
        .collect::<CompilerResult<Vec<_>>>()?
        .into_iter()
        .map(|program| (program.request.role, program))
        .collect::<BTreeMap<_, _>>();
    let references = collect_bundle_references(&parsed)?;
    let mut solved = solve_bundle_interfaces(&parsed, &references)?;
    let mut lowered = BTreeMap::<ProgramRole, LoweredRole>::new();
    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let program = lower_parsed_role(
            parsed.get(&role).expect("validated parsed role"),
            solved
                .checked
                .remove(&role)
                .expect("solved checked role authority"),
        )?;
        lowered.insert(role, program);
    }
    link_lowered_roles(lowered, target_profile)
}

fn validate_bundle_requests(programs: &[DistributedCompilerProgram]) -> Result<(), PlanError> {
    if programs.len() != 3 {
        return Err(PlanError::new(
            "distributed compilation requires exactly one Client, Session, and Server program",
        ));
    }
    let roles = programs
        .iter()
        .map(|program| program.role)
        .collect::<BTreeSet<_>>();
    if roles
        != BTreeSet::from([
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ])
    {
        return Err(PlanError::new(
            "distributed compilation requires one Client, Session, and Server role",
        ));
    }
    let first = &programs[0].application;
    let mut namespaces = BTreeSet::new();
    for program in programs {
        if program.revision == 0
            || program.schema_version == 0
            || program.units.is_empty()
            || !program.application.is_valid()
        {
            return Err(PlanError::new(
                "distributed program revisions, schema versions, sources, and application identities must be valid",
            ));
        }
        if program.application.package_id != first.package_id
            || program.application.deployment_domain != first.deployment_domain
        {
            return Err(PlanError::new(
                "distributed programs must share one package and deployment domain",
            ));
        }
        if !namespaces.insert(program.application.state_namespace.clone()) {
            return Err(PlanError::new(
                "distributed roles must use distinct state namespaces",
            ));
        }
    }
    Ok(())
}

fn parse_role(request: &DistributedCompilerProgram) -> CompilerResult<ParsedRole> {
    let parse_started = Instant::now();
    let parsed = parse_source_units(&request.source_label, &request.units)?;
    let (checked, _) = boon_typecheck::check_runtime_program_profiled_with_external_types(
        &parsed,
        &ExternalTypeEnvironment::provisional(request.role),
    );
    let checked = checked.program.ok_or_else(|| {
        let diagnostics = checked
            .report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        PlanError::new(format!(
            "{} distributed interface cannot be checked: {diagnostics}",
            request.role.namespace()
        ))
    })?;
    Ok(ParsedRole {
        request: request.clone(),
        parsed,
        checked,
        parse_ms: elapsed_ms(parse_started),
    })
}

fn lower_parsed_role(program: &ParsedRole, checked: CheckedProgram) -> CompilerResult<LoweredRole> {
    let lower_started = Instant::now();
    let ir = boon_ir::lower_checked(checked.clone(), &[])?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    Ok(LoweredRole {
        request: program.request.clone(),
        checked,
        ir,
        source_unit_count: program.parsed.files.len(),
        parse_ms: program.parse_ms,
        lower_ms,
        verify_ms,
    })
}

fn relower_role_with_producer_functions(
    program: &mut LoweredRole,
    requests: &[boon_ir::ProducerFunctionLoweringRequest],
) -> CompilerResult<()> {
    let lower_started = Instant::now();
    let ir = boon_ir::lower_checked(program.checked.clone(), requests)?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    program.ir = ir;
    program.lower_ms = lower_ms;
    program.verify_ms = verify_ms;
    Ok(())
}

fn program_role_for_namespace(namespace: &str) -> Option<ProgramRole> {
    Some(match boon_parser::program_role_root(namespace)? {
        boon_parser::ProgramRoleRoot::Client => ProgramRole::Client,
        boon_parser::ProgramRoleRoot::Session => ProgramRole::Session,
        boon_parser::ProgramRoleRoot::Server => ProgramRole::Server,
    })
}

fn collect_bundle_references(
    programs: &BTreeMap<ProgramRole, ParsedRole>,
) -> Result<BundleReferences, PlanError> {
    let mut references = BundleReferences::default();
    let mut seen_values = BTreeSet::new();

    for (consumer_role, program) in programs {
        for expression in &program.checked.expressions {
            let boon_typecheck::CheckedExpressionKind::ExternalRead { canonical_path } =
                &expression.kind
            else {
                continue;
            };
            let Some((namespace, local_path)) = canonical_path.split_once('/') else {
                continue;
            };
            let Some(producer_role) = program_role_for_namespace(namespace) else {
                continue;
            };
            validate_distributed_reference_roles(*consumer_role, producer_role)?;
            if !local_path.starts_with("store.") {
                return Err(PlanError::new(format!(
                    "qualified external value `{canonical_path}` must use `{}/store.<value>`; role outputs are host boundaries, not distributed application state",
                    producer_role.namespace()
                )));
            }
            if seen_values.insert((*consumer_role, canonical_path.clone())) {
                references.values.push(BundleValueReference {
                    consumer_role: *consumer_role,
                    producer_role,
                    canonical_path: canonical_path.clone(),
                    local_path: local_path.to_owned(),
                });
            }
        }
        for call in &program.checked.calls {
            let Some((namespace, local_function)) = call.function.split_once('/') else {
                continue;
            };
            let Some(producer_role) = program_role_for_namespace(namespace) else {
                continue;
            };
            validate_distributed_reference_roles(*consumer_role, producer_role)?;
            if local_function.is_empty() {
                return Err(PlanError::new(format!(
                    "qualified role function `{}` must name a function after the role root",
                    call.function
                )));
            }
            let arguments = call
                .entries
                .iter()
                .map(|entry| match entry {
                    CheckedCallEntry::Input { name, value, .. } => {
                        Ok((name.clone(), value.0 as usize))
                    }
                    CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => {
                        Err(PlanError::new(format!(
                            "distributed function `{}` cannot expose OUT across a runtime island",
                            call.function
                        )))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            references.calls.push(BundleCallReference {
                consumer_role: *consumer_role,
                producer_role,
                canonical_function: call.function.clone(),
                local_function: local_function.to_owned(),
                arguments,
            });
        }
    }

    references.values.sort_by(|left, right| {
        (left.consumer_role, left.canonical_path.as_str())
            .cmp(&(right.consumer_role, right.canonical_path.as_str()))
    });
    references.calls.sort_by(|left, right| {
        (
            left.consumer_role,
            left.canonical_function.as_str(),
            left.arguments.as_slice(),
        )
            .cmp(&(
                right.consumer_role,
                right.canonical_function.as_str(),
                right.arguments.as_slice(),
            ))
    });
    Ok(references)
}

fn validate_distributed_reference_roles(
    consumer_role: ProgramRole,
    producer_role: ProgramRole,
) -> Result<(), PlanError> {
    if producer_role == consumer_role {
        return Err(PlanError::new(format!(
            "same-role qualification is not allowed in {}; use an unqualified local name",
            consumer_role.namespace()
        )));
    }
    if !consumer_role.can_depend_on(producer_role) {
        return Err(PlanError::new(format!(
            "{} cannot depend directly on {}; route the value through Session",
            consumer_role.namespace(),
            producer_role.namespace()
        )));
    }
    Ok(())
}

fn solve_bundle_interfaces(
    programs: &BTreeMap<ProgramRole, ParsedRole>,
    references: &BundleReferences,
) -> Result<SolvedBundleInterfaces, PlanError> {
    let mut value_types = references
        .values
        .iter()
        .map(|reference| {
            (
                (reference.producer_role, reference.local_path.clone()),
                FlowType {
                    mode: FlowMode::Continuous,
                    ty: Type::Unknown,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut function_types = BTreeMap::<(ProgramRole, String), ExternalFunctionType>::new();
    for reference in &references.calls {
        let key = (reference.producer_role, reference.local_function.clone());
        if function_types.contains_key(&key) {
            continue;
        }
        let producer = programs
            .get(&reference.producer_role)
            .expect("validated producer role");
        let declared = declared_function_signature(&producer.checked, &reference.local_function)?;
        function_types.insert(
            key,
            ExternalFunctionType {
                args: declared
                    .parameters
                    .iter()
                    .map(|parameter| ExternalFunctionArgument {
                        name: parameter.name.clone(),
                        flow_type: FlowType {
                            mode: parameter.flow_type.mode,
                            ty: Type::Unknown,
                        },
                    })
                    .collect(),
                result: FlowType {
                    mode: declared.result.mode,
                    ty: Type::Unknown,
                },
                effect: declared.effect,
            },
        );
    }

    let mut local_requirements = BTreeMap::<(ProgramRole, String), BTreeMap<String, Type>>::new();
    let mut resolved_value_modes = BTreeSet::<(ProgramRole, String)>::new();
    let interface_slot_count = value_types.len()
        + function_types
            .values()
            .map(|function| function.args.len() + 1)
            .sum::<usize>();
    let max_passes = interface_slot_count.saturating_mul(2).max(2) + 1;
    let mut settled_checks = None;

    for _ in 0..max_passes {
        let environments = build_bundle_environments(
            references,
            &value_types,
            &function_types,
            &local_requirements,
            true,
        );
        let checks = programs
            .iter()
            .map(|(role, program)| {
                let (output, _) =
                    boon_typecheck::check_runtime_program_profiled_with_external_types(
                        &program.parsed,
                        environments.get(role).expect("provisional environment"),
                    );
                (*role, output)
            })
            .collect::<BTreeMap<_, _>>();
        let mut progress = false;

        for ((producer_role, local_path), flow_type) in &mut value_types {
            let Some(candidate) = named_value_type(
                &checks.get(producer_role).expect("producer report").report,
                local_path,
            ) else {
                continue;
            };
            let mode_key = (*producer_role, local_path.clone());
            if type_to_data_plan(&candidate.ty).is_some() {
                if resolved_value_modes.insert(mode_key.clone()) {
                    flow_type.mode = candidate.mode;
                    progress = true;
                } else if flow_type.mode != candidate.mode {
                    return Err(PlanError::new(format!(
                        "distributed value `{}/{local_path}` has inconsistent inferred flow modes",
                        producer_role.namespace()
                    )));
                }
            }
            progress |= merge_interface_type(
                &mut flow_type.ty,
                &candidate.ty,
                &format!(
                    "distributed value `{}/{local_path}`",
                    producer_role.namespace()
                ),
            )?;
        }

        for ((producer_role, local_function), signature) in &mut function_types {
            let Some(candidate) = checked_function_type(
                &checks.get(producer_role).expect("producer report").report,
                local_function,
            )?
            else {
                continue;
            };
            if candidate.args
                != signature
                    .args
                    .iter()
                    .map(|arg| arg.name.clone())
                    .collect::<Vec<_>>()
                || candidate.args.len() != candidate.arg_flows.len()
            {
                return Err(PlanError::new(format!(
                    "distributed function `{}/{local_function}` has an inconsistent checked signature",
                    producer_role.namespace()
                )));
            }
            for (argument, candidate_flow) in signature.args.iter_mut().zip(&candidate.arg_flows) {
                if argument.flow_type.mode != candidate_flow.mode {
                    argument.flow_type.mode = candidate_flow.mode;
                    progress = true;
                }
                progress |= merge_interface_type(
                    &mut argument.flow_type.ty,
                    &candidate_flow.ty,
                    &format!(
                        "distributed function `{}/{local_function}` argument `{}`",
                        producer_role.namespace(),
                        argument.name
                    ),
                )?;
            }
            progress |= merge_interface_type(
                &mut signature.result.ty,
                &candidate.result.ty,
                &format!(
                    "distributed function `{}/{local_function}` result",
                    producer_role.namespace()
                ),
            )?;
            if signature.result.mode != candidate.result.mode {
                signature.result.mode = candidate.result.mode;
                progress = true;
            }
            if signature.effect != candidate.effect {
                signature.effect = candidate.effect;
                progress = true;
            }
        }

        for call in &references.calls {
            let report = &checks
                .get(&call.consumer_role)
                .expect("consumer report")
                .report;
            let signature = function_types
                .get(&(call.producer_role, call.local_function.clone()))
                .expect("collected function signature");
            let expected_names = signature
                .args
                .iter()
                .map(|argument| argument.name.as_str())
                .collect::<BTreeSet<_>>();
            for (argument_name, expr_id) in &call.arguments {
                if !expected_names.contains(argument_name.as_str()) {
                    return Err(PlanError::new(format!(
                        "distributed function `{}` has no argument `{argument_name}`",
                        call.canonical_function
                    )));
                }
                let Some(candidate) = expression_type(report, *expr_id) else {
                    continue;
                };
                let requirements = local_requirements
                    .entry((call.producer_role, call.local_function.clone()))
                    .or_default();
                let requirement = requirements
                    .entry(argument_name.clone())
                    .or_insert(Type::Unknown);
                progress |= merge_interface_type(
                    requirement,
                    &candidate.ty,
                    &format!(
                        "distributed function `{}` argument `{argument_name}` call sites",
                        call.canonical_function
                    ),
                )?;
            }
        }

        if !progress {
            settled_checks = Some(checks);
            break;
        }
    }

    let mut unresolved = unresolved_bundle_interfaces(&value_types, &function_types);
    unresolved.extend(
        value_types
            .keys()
            .filter(|key| !resolved_value_modes.contains(*key))
            .map(|(role, path)| format!("{}/{path} flow", role.namespace())),
    );
    unresolved.sort();
    unresolved.dedup();
    if !unresolved.is_empty() {
        return Err(PlanError::new(format!(
            "distributed interface types did not resolve; add a concrete value or temporal boundary to break the interface cycle: {}",
            unresolved.join(", ")
        )));
    }

    let environments = build_bundle_environments(
        references,
        &value_types,
        &function_types,
        &local_requirements,
        false,
    );
    settled_checks.ok_or_else(|| {
        PlanError::new("distributed interface solver exceeded its bounded fixed-point passes")
    })?;
    let checked = seal_solved_bundle_checks(programs, &environments)?;
    Ok(SolvedBundleInterfaces { checked })
}

fn seal_solved_bundle_checks(
    programs: &BTreeMap<ProgramRole, ParsedRole>,
    environments: &BTreeMap<ProgramRole, ExternalTypeEnvironment>,
) -> Result<BTreeMap<ProgramRole, CheckedProgram>, PlanError> {
    programs
        .iter()
        .map(|(role, program)| {
            let (output, _) = boon_typecheck::check_runtime_program_profiled_with_external_types(
                &program.parsed,
                environments
                    .get(role)
                    .expect("settled environment exists for every role"),
            );
            if output.report.has_errors() {
                let diagnostics = output
                    .report
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| {
                        diagnostic.severity == boon_typecheck::DiagnosticSeverity::Error
                    })
                    .map(|diagnostic| diagnostic.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(PlanError::new(format!(
                    "{} distributed program failed its settled typecheck: {diagnostics}",
                    role.namespace()
                )));
            }
            let checked = output.program.ok_or_else(|| {
                PlanError::new(format!(
                    "{} settled typecheck produced no CheckedProgram",
                    role.namespace()
                ))
            })?;
            debug_assert_eq!(
                &checked.external_types,
                environments
                    .get(role)
                    .expect("settled environment exists for every role")
            );
            Ok((*role, checked))
        })
        .collect()
}

fn build_bundle_environments(
    references: &BundleReferences,
    value_types: &BTreeMap<(ProgramRole, String), FlowType>,
    function_types: &BTreeMap<(ProgramRole, String), ExternalFunctionType>,
    local_requirements: &BTreeMap<(ProgramRole, String), BTreeMap<String, Type>>,
    provisional: bool,
) -> BTreeMap<ProgramRole, ExternalTypeEnvironment> {
    let mut environments = [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ]
    .into_iter()
    .map(|role| {
        let environment = if provisional {
            ExternalTypeEnvironment::provisional(role)
        } else {
            ExternalTypeEnvironment::empty(role)
        };
        (role, environment)
    })
    .collect::<BTreeMap<_, _>>();

    for reference in &references.values {
        environments
            .get_mut(&reference.consumer_role)
            .expect("consumer environment")
            .values
            .insert(
                reference.canonical_path.clone(),
                value_types
                    .get(&(reference.producer_role, reference.local_path.clone()))
                    .expect("collected value interface")
                    .clone(),
            );
    }
    for reference in &references.calls {
        environments
            .get_mut(&reference.consumer_role)
            .expect("consumer environment")
            .functions
            .insert(
                reference.canonical_function.clone(),
                function_types
                    .get(&(reference.producer_role, reference.local_function.clone()))
                    .expect("collected function interface")
                    .clone(),
            );
    }
    for ((role, function), requirements) in local_requirements {
        environments
            .get_mut(role)
            .expect("producer environment")
            .local_function_requirements
            .insert(function.clone(), requirements.clone());
    }
    environments
}

fn declared_function_signature<'a>(
    program: &'a CheckedProgram,
    local_function: &str,
) -> Result<&'a boon_typecheck::CheckedCallableSignature, PlanError> {
    let matches = program
        .callables
        .iter()
        .filter(|callable| {
            callable.kind == CheckedCallableKind::User
                && (callable.name == local_function
                    || local_function
                        .rsplit_once('/')
                        .is_some_and(|(_, suffix)| suffix == callable.name))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [callable] => {
            if callable
                .parameters
                .iter()
                .any(|parameter| parameter.kind != CheckedParameterKind::Value)
            {
                return Err(PlanError::new(format!(
                    "distributed function `{local_function}` may contain only ordinary value parameters"
                )));
            }
            Ok(*callable)
        }
        [] => Err(PlanError::new(format!(
            "distributed function `{local_function}` is not declared by its producer role"
        ))),
        _ => Err(PlanError::new(format!(
            "distributed function `{local_function}` is ambiguous in its producer role"
        ))),
    }
}

fn named_value_type<'a>(report: &'a TypeCheckReport, path: &str) -> Option<&'a FlowType> {
    report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .map(|entry| &entry.flow_type)
}

fn checked_function_type<'a>(
    report: &'a TypeCheckReport,
    local_function: &str,
) -> Result<Option<&'a FunctionTypeEntry>, PlanError> {
    let matches = report
        .function_type_table
        .entries
        .iter()
        .filter(|entry| {
            entry.name == local_function
                || local_function
                    .rsplit_once('/')
                    .is_some_and(|(_, suffix)| suffix == entry.name)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [signature] => Ok(Some(*signature)),
        [] => Ok(None),
        _ => Err(PlanError::new(format!(
            "distributed function `{local_function}` has an ambiguous checked signature"
        ))),
    }
}

fn expression_type(report: &TypeCheckReport, expr_id: usize) -> Option<&FlowType> {
    report
        .expr_type_table
        .entries
        .iter()
        .find(|entry| entry.expr_id == expr_id)
        .map(|entry| &entry.flow_type)
}

fn merge_interface_type(
    current: &mut Type,
    candidate: &Type,
    label: &str,
) -> Result<bool, PlanError> {
    if type_to_data_plan(candidate).is_none() {
        return Ok(false);
    }
    if type_to_data_plan(current).is_none() {
        *current = candidate.clone();
        return Ok(true);
    }
    let Some(merged) = merge_closed_boundary_types(current, candidate) else {
        return Err(PlanError::new(format!(
            "{label} has incompatible inferred boundary types: {} and {}",
            boundary_type_label(current),
            boundary_type_label(candidate)
        )));
    };
    if *current == merged {
        Ok(false)
    } else {
        *current = merged;
        Ok(true)
    }
}

fn merge_closed_boundary_types(left: &Type, right: &Type) -> Option<Type> {
    if left == right {
        return Some(left.clone());
    }
    match (left, right) {
        (Type::Bytes(_), Type::Bytes(_)) => Some(Type::Bytes(boon_typecheck::BytesType::Dynamic)),
        (Type::List(left), Type::List(right)) => Some(Type::List(Box::new(
            merge_closed_boundary_types(left, right)?,
        ))),
        (Type::Object(left), Type::Object(right))
            if !left.open && !right.open && left.fields.keys().eq(right.fields.keys()) =>
        {
            let ordered_fields = left
                .fields
                .iter()
                .map(|(name, left_type)| {
                    Some((
                        name.clone(),
                        merge_closed_boundary_types(left_type, right.fields.get(name)?)?,
                    ))
                })
                .collect::<Option<Vec<_>>>()?;
            Some(Type::Object(ObjectShape {
                fields: ordered_fields.iter().cloned().collect(),
                field_order: ordered_fields.into_iter().map(|(name, _)| name).collect(),
                open: false,
            }))
        }
        (Type::VariantSet(left), Type::VariantSet(right)) => {
            let mut variants = left.clone();
            for candidate in right {
                if !variants.contains(candidate) {
                    variants.push(candidate.clone());
                }
            }
            variants.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
            Some(Type::VariantSet(variants))
        }
        _ => None,
    }
}

fn boundary_type_label(ty: &Type) -> String {
    format!("{ty:?}")
}

fn unresolved_bundle_interfaces(
    value_types: &BTreeMap<(ProgramRole, String), FlowType>,
    function_types: &BTreeMap<(ProgramRole, String), ExternalFunctionType>,
) -> Vec<String> {
    let mut unresolved = Vec::new();
    for ((role, path), flow_type) in value_types {
        if type_to_data_plan(&flow_type.ty).is_none() {
            unresolved.push(format!("{}/{path}", role.namespace()));
        }
    }
    for ((role, function), signature) in function_types {
        for argument in &signature.args {
            if type_to_data_plan(&argument.flow_type.ty).is_none() {
                unresolved.push(format!(
                    "{}/{function} argument {}",
                    role.namespace(),
                    argument.name
                ));
            }
        }
        if type_to_data_plan(&signature.result.ty).is_none() {
            unresolved.push(format!("{}/{function} result", role.namespace()));
        }
    }
    unresolved.sort();
    unresolved.dedup();
    unresolved
}

fn resolve_distributed_producer_value_ref(
    role: ProgramRole,
    local_path: &str,
    lowered: &BTreeMap<ProgramRole, LoweredRole>,
    graph_id: boon_plan::DistributedGraphId,
    endpoints: &BTreeMap<ProgramRole, EndpointIdentity>,
    visiting: &mut Vec<(ProgramRole, String)>,
) -> Result<ValueRef, PlanError> {
    let key = (role, local_path.to_owned());
    if visiting.contains(&key) {
        return Err(PlanError::new(format!(
            "distributed event aliases form a route cycle at `{}/{local_path}`",
            role.namespace()
        )));
    }
    visiting.push(key);
    let result = (|| {
        let program = &lowered.get(&role).expect("validated role").ir;
        if let Some(reference) =
            program
                .distributed_references
                .value_references
                .iter()
                .find(|reference| {
                    reference
                        .local_alias_paths
                        .iter()
                        .any(|path| path == local_path)
                })
        {
            if reference.flow_mode == FlowMode::Absent {
                return Err(PlanError::new(format!(
                    "distributed alias `{}/{local_path}` is always absent",
                    role.namespace()
                )));
            }
            let upstream_local =
                strip_role_value_prefix(&reference.canonical_path, reference.producer_role)?;
            let upstream = resolve_distributed_producer_value_ref(
                reference.producer_role,
                &upstream_local,
                lowered,
                graph_id,
                endpoints,
                visiting,
            )?;
            return match distributed_flow_for_value_ref(&upstream) {
                DistributedReferenceFlow::Current => {
                    let stable_identity = DistributedDeclarationId::from_semantic_path(
                        role_namespace(role),
                        &format!("import:{}", reference.canonical_path),
                    )?;
                    Ok(ValueRef::DistributedImport(ImportId::from_value_identity(
                        graph_id,
                        endpoints.get(&role).expect("role endpoint").endpoint_id,
                        stable_identity,
                    )?))
                }
                DistributedReferenceFlow::Event => {
                    let payload_field = match upstream {
                        ValueRef::Source(_) => None,
                        ValueRef::SourcePayload { field, .. } => Some(field),
                        _ => unreachable!("event authority is always a SOURCE value"),
                    };
                    let source_path =
                        boon_ir::distributed_event_source_path(&reference.canonical_path);
                    let source_id = SourceId(
                        program
                            .sources
                            .iter()
                            .find(|source| source.path == source_path)
                            .ok_or_else(|| {
                                PlanError::new(format!(
                                    "distributed event alias `{}/{local_path}` has no local source lane",
                                    role.namespace()
                                ))
                            })?
                            .id
                            .as_usize(),
                    );
                    Ok(match payload_field {
                        Some(field) => ValueRef::SourcePayload { source_id, field },
                        None => ValueRef::Source(source_id),
                    })
                }
            };
        }

        machine_plan_backend::distributed_exportable_values(program)
            .get(local_path)
            .map(|(_, value_ref)| value_ref.clone())
            .ok_or_else(|| {
                PlanError::new(format!(
                    "distributed value `{}/{local_path}` has no executable producer value",
                    role.namespace()
                ))
            })
    })();
    visiting.pop();
    result
}

fn link_lowered_roles(
    lowered: BTreeMap<ProgramRole, LoweredRole>,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledDistributedMachinePlans> {
    link_lowered_roles_round(lowered, target_profile, 0)
}

fn link_lowered_roles_round(
    mut lowered: BTreeMap<ProgramRole, LoweredRole>,
    target_profile: TargetProfile,
    producer_round: usize,
) -> CompilerResult<CompiledDistributedMachinePlans> {
    let producer_round_limit = lowered
        .values()
        .map(|program| program.checked.expressions.len())
        .sum::<usize>()
        .saturating_add(1);
    if producer_round > producer_round_limit {
        return Err(PlanError::new(
            "distributed producer-function expansion did not reach a finite call graph",
        )
        .into());
    }
    let application = lowered
        .get(&ProgramRole::Client)
        .expect("validated client role")
        .request
        .application
        .clone();
    let graph_revision = lowered
        .values()
        .map(|program| program.request.revision)
        .max()
        .unwrap_or(1);
    let graph_stable_identity = DistributedDeclarationId::from_semantic_path(
        &application.package_id,
        "Client+Session+Server",
    )?;
    let graph_identity =
        DistributedGraphIdentityPlan::new(&application, graph_stable_identity, graph_revision)?;
    let endpoints = [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ]
    .into_iter()
    .map(|role| {
        let stable_identity = DistributedDeclarationId::from_semantic_path(
            &application.package_id,
            role_namespace(role),
        )?;
        let endpoint_id =
            DistributedEndpointId::from_identity(graph_identity.graph_id, role, stable_identity)?;
        Ok((
            role,
            EndpointIdentity {
                stable_identity,
                endpoint_id,
            },
        ))
    })
    .collect::<Result<BTreeMap<_, _>, PlanError>>()?;

    let mut contexts = [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ]
    .into_iter()
    .map(|role| {
        (
            role,
            machine_plan_backend::DistributedMachineContext::default(),
        )
    })
    .collect::<BTreeMap<_, _>>();
    let mut value_links = BTreeMap::<(ProgramRole, ProgramRole, String), ValueLink>::new();
    let mut function_links = BTreeMap::<(ProgramRole, String), FunctionLink>::new();
    let mut call_links = Vec::new();
    let mut call_occurrences = BTreeMap::<(ProgramRole, String, String), usize>::new();
    let mut next_synthetic_source_ids = lowered
        .iter()
        .map(|(role, program)| (*role, program.ir.sources.len()))
        .collect::<BTreeMap<_, _>>();

    for (consumer_role, consumer) in &lowered {
        for reference in &consumer.ir.distributed_references.value_references {
            let producer_role = reference.producer_role;
            if reference.flow_mode == FlowMode::Absent {
                return Err(PlanError::new(format!(
                    "distributed value `{}` is always absent",
                    reference.canonical_path
                ))
                .into());
            }
            let local_path = strip_role_value_prefix(&reference.canonical_path, producer_role)?;
            let producer_value_ref = resolve_distributed_producer_value_ref(
                producer_role,
                &local_path,
                &lowered,
                graph_identity.graph_id,
                &endpoints,
                &mut Vec::new(),
            )?;
            let producer_values = machine_plan_backend::distributed_exportable_values(
                &lowered.get(&producer_role).expect("producer role").ir,
            );
            let producer_flow = producer_values
                .get(&local_path)
                .map(|(flow, _)| flow.mode)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "distributed value `{}` has no producer flow",
                        reference.canonical_path
                    ))
                })?;
            if producer_flow == FlowMode::Absent {
                return Err(PlanError::new(format!(
                    "distributed value `{}` is always absent",
                    reference.canonical_path
                ))
                .into());
            }
            let flow = distributed_flow_for_value_ref(&producer_value_ref);
            let event_payload_field = match (flow, &producer_value_ref) {
                (DistributedReferenceFlow::Current, _)
                | (DistributedReferenceFlow::Event, ValueRef::Source(_)) => None,
                (DistributedReferenceFlow::Event, ValueRef::SourcePayload { field, .. }) => {
                    Some(field.clone())
                }
                (DistributedReferenceFlow::Event, _) => {
                    return Err(PlanError::new(format!(
                        "distributed event `{}` is not backed by a SOURCE value",
                        reference.canonical_path
                    ))
                    .into());
                }
            };
            let data_type = type_to_data_plan(&reference.value_type).ok_or_else(|| {
                PlanError::new(format!(
                    "distributed value `{}` does not have a closed boundary type",
                    reference.canonical_path
                ))
            })?;
            let key = (
                *consumer_role,
                producer_role,
                reference.canonical_path.clone(),
            );
            let link = if let Some(link) = value_links.get(&key) {
                link.clone()
            } else {
                let producer_endpoint = endpoints.get(&producer_role).expect("producer endpoint");
                let consumer_endpoint = endpoints.get(consumer_role).expect("consumer endpoint");
                let export_stable_identity = DistributedDeclarationId::from_semantic_path(
                    role_namespace(producer_role),
                    &local_path,
                )?;
                let export_id = ExportId::from_identity(
                    graph_identity.graph_id,
                    producer_endpoint.endpoint_id,
                    match flow {
                        DistributedReferenceFlow::Current => {
                            boon_plan::DistributedExportKind::Value
                        }
                        DistributedReferenceFlow::Event => boon_plan::DistributedExportKind::Event,
                    },
                    export_stable_identity,
                )?;
                let import_stable_identity = DistributedDeclarationId::from_semantic_path(
                    role_namespace(*consumer_role),
                    &format!("import:{}", reference.canonical_path),
                )?;
                let import_id = match flow {
                    DistributedReferenceFlow::Current => ImportId::from_value_identity(
                        graph_identity.graph_id,
                        consumer_endpoint.endpoint_id,
                        import_stable_identity,
                    )?,
                    DistributedReferenceFlow::Event => ImportId::from_event_identity(
                        graph_identity.graph_id,
                        consumer_endpoint.endpoint_id,
                        import_stable_identity,
                    )?,
                };
                let event_source_id = if flow == DistributedReferenceFlow::Event {
                    let source_path =
                        boon_ir::distributed_event_source_path(&reference.canonical_path);
                    Some(SourceId(
                        consumer
                            .ir
                            .sources
                            .iter()
                            .find(|source| source.path == source_path)
                            .ok_or_else(|| {
                                PlanError::new(format!(
                                    "distributed event `{}` has no IR source lane",
                                    reference.canonical_path
                                ))
                            })?
                            .id
                            .as_usize(),
                    ))
                } else {
                    None
                };
                let link = ValueLink {
                    consumer_role: *consumer_role,
                    producer_role,
                    canonical_path: reference.canonical_path.clone(),
                    local_path,
                    export_stable_identity,
                    export_id,
                    import_stable_identity,
                    import_id,
                    flow,
                    event_source_id,
                    event_payload_field,
                    producer_value_ref,
                    data_type,
                };
                value_links.insert(key, link.clone());
                link
            };
            let context = contexts.get_mut(consumer_role).expect("consumer context");
            let imported_value_ref = match link.flow {
                DistributedReferenceFlow::Current => ValueRef::DistributedImport(link.import_id),
                DistributedReferenceFlow::Event => match &link.event_payload_field {
                    Some(field) => ValueRef::SourcePayload {
                        source_id: link.event_source_id.expect("event link source"),
                        field: field.clone(),
                    },
                    None => ValueRef::Source(link.event_source_id.expect("event link source")),
                },
            };
            bind_checked_expression_refs(
                &mut context.expression_refs,
                &consumer.ir,
                reference.expr_id.as_usize(),
                imported_value_ref.clone(),
            )?;
            context
                .path_refs
                .insert(reference.canonical_path.clone(), imported_value_ref.clone());
            for local_path in &reference.local_alias_paths {
                context
                    .path_refs
                    .insert(local_path.clone(), imported_value_ref.clone());
            }
            if let Some(source_id) = link.event_source_id
                && !context
                    .synthetic_source_routes
                    .iter()
                    .any(|route| route.source_id == source_id)
            {
                let fields = link.event_payload_field.iter().cloned().collect::<Vec<_>>();
                let typed_fields = link
                    .event_payload_field
                    .iter()
                    .cloned()
                    .map(|field| SourcePayloadDescriptor {
                        field,
                        data_type: link.data_type.clone(),
                    })
                    .collect();
                context.synthetic_source_routes.push(SourceRoute {
                    id: PlanSourceRouteId(usize::MAX),
                    source_id,
                    owner: PlanOwner::root(),
                    path: boon_ir::distributed_event_source_path(&link.canonical_path),
                    scoped: false,
                    scope_id: None,
                    interval_ms: None,
                    payload_schema: SourcePayloadSchema {
                        fields,
                        typed_fields,
                    },
                });
            }
        }

        for (call_reference, call) in consumer.ir.distributed_references.calls.iter().enumerate() {
            let producer_role = call.producer_role;
            let local_function =
                strip_role_function_prefix(&call.canonical_function, producer_role)?;
            let function_key = (producer_role, call.canonical_function.clone());
            let function = if let Some(function) = function_links.get(&function_key) {
                function.clone()
            } else {
                let producer = lowered.get(&producer_role).expect("producer program");
                let signature = find_function_signature(&producer.ir, &local_function)?.clone();
                let result_type = type_to_data_plan(&signature.result.ty).ok_or_else(|| {
                    PlanError::new(format!(
                        "distributed function `{}` result is not a closed boundary type",
                        call.canonical_function
                    ))
                })?;
                let stable_identity = DistributedDeclarationId::from_semantic_path(
                    role_namespace(producer_role),
                    &local_function,
                )?;
                let export_id = ExportId::from_identity(
                    graph_identity.graph_id,
                    endpoints
                        .get(&producer_role)
                        .expect("producer endpoint")
                        .endpoint_id,
                    boon_plan::DistributedExportKind::Function,
                    stable_identity,
                )?;
                let function = FunctionLink {
                    producer_role,
                    canonical_function: call.canonical_function.clone(),
                    local_function,
                    stable_identity,
                    export_id,
                    signature,
                    result_type,
                };
                function_links.insert(function_key, function.clone());
                function
            };
            if call.effect != function.signature.effect {
                return Err(PlanError::new(format!(
                    "distributed call `{}` effect summary differs from its producer signature",
                    call.canonical_function
                ))
                .into());
            }
            let mode = if call.invocation_arms.is_empty()
                && call.result.mode == FlowMode::Continuous
                && call
                    .arguments
                    .iter()
                    .all(|argument| argument.flow_type.mode == FlowMode::Continuous)
                && !call.effect.emits_source
                && !call.effect.invokes_host
            {
                DistributedCallMode::Current
            } else {
                DistributedCallMode::Invocation
            };
            if mode == DistributedCallMode::Invocation && call.invocation_arms.is_empty() {
                return Err(PlanError::new(format!(
                    "distributed call `{}` requires invocation semantics but has no exact trigger arm",
                    call.canonical_function
                ))
                .into());
            }
            let owner_path =
                erased_external_call_owner_path(&consumer.ir, call_reference, call.owner)?;
            let owner = machine_plan_backend::plan_owner_for_static_owner(
                &consumer.ir,
                call.owner,
                &format!("distributed call `{}`", call.canonical_function),
            )?;
            let occurrence_key = (
                *consumer_role,
                owner_path.clone(),
                call.canonical_function.clone(),
            );
            let occurrence = call_occurrences.entry(occurrence_key).or_default();
            let call_path = format!(
                "call:{owner_path}:{}:{}",
                call.canonical_function, *occurrence
            );
            *occurrence += 1;
            let stable_identity = DistributedDeclarationId::from_semantic_path(
                role_namespace(*consumer_role),
                &call_path,
            )?;
            let call_site_id = boon_plan::RemoteCallSiteId::from_identity(
                graph_identity.graph_id,
                endpoints
                    .get(consumer_role)
                    .expect("consumer endpoint")
                    .endpoint_id,
                stable_identity,
            )?;
            let result = match mode {
                DistributedCallMode::Current => DistributedCallResultPlan::Current {
                    import_id: ImportId::from_remote_call_result(call_site_id)?,
                },
                DistributedCallMode::Invocation => {
                    let next_source_id = next_synthetic_source_ids
                        .get_mut(consumer_role)
                        .expect("consumer source allocator");
                    let source_id = SourceId(*next_source_id);
                    *next_source_id = next_source_id.checked_add(1).ok_or_else(|| {
                        PlanError::new("distributed invocation result source IDs exhausted")
                    })?;
                    let payload_field = SourcePayloadField::Named("result".to_owned());
                    let owner = owner.clone();
                    let scope_id = owner.ancestors.last().map(|ancestor| ancestor.scope);
                    let context = contexts.get_mut(consumer_role).expect("consumer context");
                    context.synthetic_source_routes.push(SourceRoute {
                        id: PlanSourceRouteId(usize::MAX),
                        source_id,
                        owner,
                        path: format!("@distributed/result/{call_site_id}"),
                        scoped: scope_id.is_some(),
                        scope_id,
                        interval_ms: None,
                        payload_schema: SourcePayloadSchema {
                            fields: vec![payload_field.clone()],
                            typed_fields: vec![SourcePayloadDescriptor {
                                field: payload_field.clone(),
                                data_type: function.result_type.clone(),
                            }],
                        },
                    });
                    context.invocation_result_sources.insert(source_id);
                    DistributedCallResultPlan::Invocation {
                        source_id,
                        payload_field,
                    }
                }
            };
            bind_executable_expression_ref(
                &mut contexts
                    .get_mut(consumer_role)
                    .expect("consumer context")
                    .expression_refs,
                call.expression,
                result.value_ref(),
            )?;
            call_links.push(CallLink {
                consumer_role: *consumer_role,
                owner_path,
                owner,
                stable_identity,
                call_site_id,
                call: call.clone(),
                function_export_id: function.export_id,
                result,
                mode,
            });
        }
    }

    let mut function_exports = BTreeMap::<ExportId, DistributedFunctionExportPlan>::new();
    for function in function_links.values() {
        let parameters = function
            .signature
            .args
            .iter()
            .cloned()
            .zip(function.signature.arg_flows.iter())
            .map(|(name, flow)| {
                type_to_data_plan(&flow.ty)
                    .map(|data_type| (name, data_type))
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "distributed function `{}` argument is not a closed boundary type",
                            function.canonical_function
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let export = DistributedFunctionExportPlan::new(
            graph_identity.graph_id,
            endpoints
                .get(&function.producer_role)
                .expect("producer endpoint")
                .endpoint_id,
            function.stable_identity,
            lowered
                .get(&function.producer_role)
                .expect("producer")
                .request
                .revision,
            function.producer_role,
            parameters,
            function.result_type.clone(),
        )?;
        if export.export_id != function.export_id {
            return Err(
                PlanError::new("distributed function export ID changed during linking").into(),
            );
        }
        function_exports.insert(export.export_id, export);
    }

    let mut requested_producers =
        BTreeMap::<ProgramRole, BTreeSet<boon_ir::ProducerFunctionLoweringRequest>>::new();
    for link in &call_links {
        let function_link = function_links
            .values()
            .find(|candidate| candidate.export_id == link.function_export_id)
            .expect("linked function definition");
        requested_producers
            .entry(function_link.producer_role)
            .or_default()
            .insert(boon_ir::ProducerFunctionLoweringRequest {
                identity: link.call_site_id.0,
                local_function: function_link.local_function.clone(),
                mode: match link.mode {
                    DistributedCallMode::Current => boon_ir::ProducerFunctionMode::Current,
                    DistributedCallMode::Invocation => boon_ir::ProducerFunctionMode::Invocation,
                },
            });
    }
    let mut relower_roles = Vec::new();
    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let expected = requested_producers.get(&role).cloned().unwrap_or_default();
        let actual = lowered
            .get(&role)
            .expect("lowered producer role")
            .ir
            .producer_function_instances
            .iter()
            .map(|instance| boon_ir::ProducerFunctionLoweringRequest {
                identity: instance.identity,
                local_function: instance.function_name.clone(),
                mode: instance.mode,
            })
            .collect::<BTreeSet<_>>();
        if actual != expected {
            relower_roles.push((role, expected.into_iter().collect::<Vec<_>>()));
        }
    }
    if !relower_roles.is_empty() {
        for (role, requests) in relower_roles {
            relower_role_with_producer_functions(
                lowered.get_mut(&role).expect("lowered producer role"),
                &requests,
            )?;
        }
        return link_lowered_roles_round(lowered, target_profile, producer_round.saturating_add(1));
    }

    for link in &call_links {
        let function = function_exports
            .get(&link.function_export_id)
            .expect("linked function export");
        let function_link = function_links
            .values()
            .find(|candidate| candidate.export_id == link.function_export_id)
            .expect("linked function definition");
        let producer = lowered
            .get(&function.producer_role)
            .expect("compiled function producer");
        let instance = producer
            .ir
            .producer_function_instances
            .iter()
            .find(|instance| {
                instance.identity == link.call_site_id.0
                    && instance.function_name == function_link.local_function
            })
            .ok_or_else(|| {
                PlanError::new(format!(
                    "producer function `{}` has no exact pre-lowered graph instance",
                    link.call.canonical_function
                ))
            })?;
        let instance_plan = ProducerFunctionInstancePlan::new(
            link.call_site_id,
            function,
            machine_plan_backend::plan_owner_for_static_owner(
                &producer.ir,
                Some(instance.owner),
                &format!("producer function `{}`", link.call.canonical_function),
            )?,
            link.mode,
            instance.invocation_source.map(|source| SourceId(source.0)),
            machine_plan_backend::producer_function_ownership_seed(&producer.ir, instance.owner)?,
            ValueRef::Field(FieldId(instance.result_field.0)),
        )?;
        if instance.arguments.len() != instance_plan.arguments.len() {
            return Err(PlanError::new(format!(
                    "producer function `{}` executable parameter count differs from its boundary signature",
                    link.call.canonical_function
                ))
            .into());
        }
        let context = contexts
            .get_mut(&function.producer_role)
            .expect("producer context");
        for argument in &instance.arguments {
            let planned = instance_plan
                .arguments
                .iter()
                .find(|planned| planned.name == argument.name)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "producer function `{}` has no boundary argument `{}`",
                        link.call.canonical_function, argument.name
                    ))
                })?;
            if argument.input_expressions.is_empty() {
                return Err(PlanError::new(format!(
                    "producer function `{}` argument `{}` has no executable input occurrence",
                    link.call.canonical_function, argument.name
                ))
                .into());
            }
            for expression in &argument.input_expressions {
                bind_executable_expression_ref(
                    &mut context.expression_refs,
                    *expression,
                    ValueRef::DistributedImport(planned.import_id),
                )?;
            }
        }
        context.producer_function_instances.push(instance_plan);
    }
    for context in contexts.values_mut() {
        context
            .producer_function_instances
            .sort_by_key(|instance| instance.call_site_id);
    }

    let mut compiled = BTreeMap::new();
    for role in [
        ProgramRole::Server,
        ProgramRole::Session,
        ProgramRole::Client,
    ] {
        let program = lowered.get(&role).expect("lowered role");
        let compile_started = Instant::now();
        let plan = machine_plan_backend::compile_typed_program_with_distributed_context(
            &program.ir,
            target_profile,
            role,
            &program.request.application,
            program.request.schema_version,
            &program.request.migration_predecessors,
            contexts.get(&role).expect("distributed context"),
        )?;
        let compile_ms = elapsed_ms(compile_started);
        compiled.insert(
            role,
            CompiledMachinePlanFromSource {
                ir: program.ir.clone(),
                plan,
                profile: CompileProfile {
                    source_unit_count: program.source_unit_count,
                    expression_count: program.ir.expression_count,
                    graph_node_count: program.ir.graph_node_count,
                    parse_ms: program.parse_ms,
                    lower_ms: program.lower_ms,
                    verify_ms: program.verify_ms,
                    compile_ms,
                    total_ms: program.parse_ms + program.lower_ms + program.verify_ms + compile_ms,
                },
            },
        );
    }
    let mut value_exports = BTreeMap::<ExportId, DistributedValueExportPlan>::new();
    let mut event_exports = BTreeMap::<ExportId, DistributedEventExportPlan>::new();
    let origin_scoped_server_values = origin_scoped_server_values(
        compiled.get(&ProgramRole::Server).expect("compiled Server"),
        value_links.values(),
        &call_links,
    )?;
    for link in value_links.values() {
        if match link.flow {
            DistributedReferenceFlow::Current => value_exports.contains_key(&link.export_id),
            DistributedReferenceFlow::Event => event_exports.contains_key(&link.export_id),
        } {
            continue;
        }
        let producer = compiled
            .get(&link.producer_role)
            .expect("compiled producer");
        let local_values = machine_plan_backend::distributed_exportable_values(&producer.ir);
        let (producer_flow, _) = local_values.get(&link.local_path).cloned().ok_or_else(|| {
            PlanError::new(format!(
                "distributed value `{}` has no executable producer value",
                link.canonical_path
            ))
        })?;
        let value_ref = link.producer_value_ref.clone();
        if producer_flow.mode == FlowMode::Absent {
            return Err(PlanError::new(format!(
                "distributed value `{}` is always absent",
                link.canonical_path
            ))
            .into());
        }
        let expected_flow = distributed_flow_for_value_ref(&value_ref);
        if expected_flow != link.flow {
            return Err(PlanError::new(format!(
                "distributed value `{}` changed flow during executable lowering",
                link.canonical_path
            ))
            .into());
        }
        let endpoint_id = endpoints
            .get(&link.producer_role)
            .expect("producer endpoint")
            .endpoint_id;
        let revision = lowered
            .get(&link.producer_role)
            .expect("producer")
            .request
            .revision;
        match link.flow {
            DistributedReferenceFlow::Current => {
                let export = DistributedValueExportPlan::new(
                    graph_identity.graph_id,
                    endpoint_id,
                    link.export_stable_identity,
                    revision,
                    link.producer_role,
                    link.producer_role == ProgramRole::Server
                        && origin_scoped_server_values.contains(&value_ref),
                    value_ref,
                    link.data_type.clone(),
                )?;
                if export.export_id != link.export_id {
                    return Err(PlanError::new(
                        "distributed value export ID changed during linking",
                    )
                    .into());
                }
                value_exports.insert(export.export_id, export);
            }
            DistributedReferenceFlow::Event => {
                let source_id = match value_ref {
                    ValueRef::Source(source_id) | ValueRef::SourcePayload { source_id, .. } => {
                        source_id
                    }
                    _ => {
                        return Err(PlanError::new(format!(
                            "distributed event `{}` is not backed by a SOURCE value",
                            link.canonical_path
                        ))
                        .into());
                    }
                };
                let export = DistributedEventExportPlan::new(
                    graph_identity.graph_id,
                    endpoint_id,
                    link.export_stable_identity,
                    revision,
                    link.producer_role,
                    source_id,
                    link.event_payload_field.clone(),
                    link.data_type.clone(),
                )?;
                if export.export_id != link.export_id {
                    return Err(PlanError::new(
                        "distributed event export ID changed during linking",
                    )
                    .into());
                }
                event_exports.insert(export.export_id, export);
            }
        }
    }

    let mut value_imports = BTreeMap::<ProgramRole, Vec<DistributedValueImportPlan>>::new();
    let mut event_imports = BTreeMap::<ProgramRole, Vec<DistributedEventImportPlan>>::new();
    for link in value_links.values() {
        let already_linked =
            match link.flow {
                DistributedReferenceFlow::Current => value_imports
                    .get(&link.consumer_role)
                    .is_some_and(|imports| {
                        imports
                            .iter()
                            .any(|import| import.import_id == link.import_id)
                    }),
                DistributedReferenceFlow::Event => event_imports
                    .get(&link.consumer_role)
                    .is_some_and(|imports| {
                        imports
                            .iter()
                            .any(|import| import.import_id == link.import_id)
                    }),
            };
        if already_linked {
            continue;
        }
        let endpoint_id = endpoints
            .get(&link.consumer_role)
            .expect("consumer endpoint")
            .endpoint_id;
        let revision = lowered
            .get(&link.consumer_role)
            .expect("consumer")
            .request
            .revision;
        match link.flow {
            DistributedReferenceFlow::Current => {
                let import = DistributedValueImportPlan::new(
                    graph_identity.graph_id,
                    endpoint_id,
                    link.import_stable_identity,
                    revision,
                    link.consumer_role,
                    value_exports
                        .get(&link.export_id)
                        .expect("linked value export"),
                )?;
                if import.import_id != link.import_id {
                    return Err(PlanError::new(
                        "distributed value import ID changed during linking",
                    )
                    .into());
                }
                value_imports
                    .entry(link.consumer_role)
                    .or_default()
                    .push(import);
            }
            DistributedReferenceFlow::Event => {
                let import = DistributedEventImportPlan::new(
                    graph_identity.graph_id,
                    endpoint_id,
                    link.import_stable_identity,
                    revision,
                    link.consumer_role,
                    event_exports
                        .get(&link.export_id)
                        .expect("linked event export"),
                    link.event_source_id.expect("event link source"),
                )?;
                if import.import_id != link.import_id {
                    return Err(PlanError::new(
                        "distributed event import ID changed during linking",
                    )
                    .into());
                }
                event_imports
                    .entry(link.consumer_role)
                    .or_default()
                    .push(import);
            }
        }
    }

    let mut remote_calls = BTreeMap::<ProgramRole, Vec<RemoteCallSitePlan>>::new();
    for link in &call_links {
        let function = function_exports
            .get(&link.function_export_id)
            .expect("linked function export");
        let context = contexts
            .get(&link.consumer_role)
            .expect("consumer context")
            .clone();
        let consumer = compiled
            .get_mut(&link.consumer_role)
            .expect("compiled call consumer");
        let arguments = link
            .call
            .arguments
            .iter()
            .map(|argument| {
                Ok((
                    argument.name.clone(),
                    machine_plan_backend::lower_distributed_root_expression(
                        &consumer.ir,
                        &link.owner_path,
                        argument.value,
                        &mut consumer.plan.row_expressions,
                        &mut consumer.plan.constants,
                        &context,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let invocation_arms = link
            .call
            .invocation_arms
            .iter()
            .map(|arm| {
                let trigger = match arm.cause {
                    boon_ir::EventCause::Source(source) => ValueRef::Source(SourceId(source.0)),
                    boon_ir::EventCause::State(state) => {
                        ValueRef::State(boon_plan::StateId(state.0))
                    }
                };
                Ok(DistributedInvocationArmPlan {
                    gate: machine_plan_backend::lower_distributed_invocation_gate(
                        &consumer.ir,
                        &link.owner_path,
                        arm.output_expression_id,
                        link.call.expression,
                        &trigger,
                        &mut consumer.plan.row_expressions,
                        &mut consumer.plan.constants,
                        &context,
                    )?,
                    trigger,
                })
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let row_bindings = distributed_call_row_bindings(&consumer.ir, link.call.owner)?;
        let call = RemoteCallSitePlan::new(
            graph_identity.graph_id,
            endpoints
                .get(&link.consumer_role)
                .expect("consumer endpoint")
                .endpoint_id,
            link.stable_identity,
            lowered
                .get(&link.consumer_role)
                .expect("consumer")
                .request
                .revision,
            link.consumer_role,
            link.owner.clone(),
            function,
            arguments,
            row_bindings,
            link.mode,
            link.result.invocation_source().map(|(source, _)| source),
            invocation_arms,
        )?;
        if call.result != link.result {
            return Err(PlanError::new("remote call result lane changed during linking").into());
        }
        remote_calls
            .entry(link.consumer_role)
            .or_default()
            .push(call);
    }

    let mut endpoint_contracts = Vec::new();
    for role in [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ] {
        let endpoint = endpoints.get(&role).expect("endpoint identity");
        let contract = DistributedEndpointContractPlan::new(
            &graph_identity,
            endpoint.stable_identity,
            lowered.get(&role).expect("role").request.revision,
            role,
            value_exports
                .values()
                .filter(|export| export.producer_role == role)
                .cloned()
                .collect(),
            value_imports.remove(&role).unwrap_or_default(),
            event_exports
                .values()
                .filter(|export| export.producer_role == role)
                .cloned()
                .collect(),
            event_imports.remove(&role).unwrap_or_default(),
            function_exports
                .values()
                .filter(|export| export.producer_role == role)
                .cloned()
                .collect(),
            remote_calls.remove(&role).unwrap_or_default(),
        )?;
        endpoint_contracts.push(contract);
    }
    let graph = DistributedGraphPlan::new(&application, graph_identity, endpoint_contracts)?;

    for (role, program) in &mut compiled {
        program.plan.distributed_endpoint = Some(DistributedEndpointPlan::new(
            &program.plan.application.identity,
            &graph,
            *role,
        )?);
        machine_plan_backend::finalize_machine_plan_row_expressions(&mut program.plan)?;
        program.plan.capability_summary.constant_count = program.plan.constants.len();
    }

    validate_distributed_immediate_cycles(&compiled, &call_links)?;

    for (role, program) in &compiled {
        let verification = verify_plan(&program.plan)?;
        if verification.status != "pass" {
            let failed = verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .map(|check| format!("{}: {}", check.id, check.detail))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(PlanError::new(format!(
                "{} distributed machine plan failed verification: {failed}",
                role.as_str()
            ))
            .into());
        }
    }

    Ok(CompiledDistributedMachinePlans {
        graph,
        programs: [
            ProgramRole::Client,
            ProgramRole::Session,
            ProgramRole::Server,
        ]
        .into_iter()
        .map(|role| (role, compiled.remove(&role).expect("compiled role")))
        .collect(),
    })
}

fn bind_checked_expression_refs(
    refs: &mut BTreeMap<boon_ir::ExecutableExprId, ValueRef>,
    program: &ErasedProgram,
    checked_expr_id: usize,
    value: ValueRef,
) -> Result<(), PlanError> {
    let checked_expr_id = boon_typecheck::CheckedExprId(checked_expr_id as u32);
    let matches = program
        .executable
        .expressions
        .iter()
        .filter(|expression| expression.checked_expr_id == checked_expr_id)
        .map(|expression| expression.id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(PlanError::new(format!(
            "distributed checked expression {} has no executable identity",
            checked_expr_id.0
        )));
    }
    for expression in matches {
        if let Some(previous) = refs.insert(expression, value.clone())
            && previous != value
        {
            return Err(PlanError::new(format!(
                "distributed executable expression {} resolves to conflicting values",
                expression.0
            )));
        }
    }
    Ok(())
}

fn bind_executable_expression_ref(
    refs: &mut BTreeMap<boon_ir::ExecutableExprId, ValueRef>,
    expression: boon_ir::ExecutableExprId,
    value: ValueRef,
) -> Result<(), PlanError> {
    if let Some(previous) = refs.insert(expression, value.clone())
        && previous != value
    {
        return Err(PlanError::new(format!(
            "distributed executable expression {} resolves to conflicting values",
            expression.0
        )));
    }
    Ok(())
}

fn origin_scoped_server_values<'a>(
    server: &CompiledMachinePlanFromSource,
    value_links: impl Iterator<Item = &'a ValueLink>,
    call_links: &[CallLink],
) -> Result<BTreeSet<ValueRef>, PlanError> {
    let mut scoped = value_links
        .filter(|link| {
            link.flow == DistributedReferenceFlow::Current
                && link.consumer_role == ProgramRole::Server
                && link.producer_role == ProgramRole::Session
        })
        .map(|link| ValueRef::DistributedImport(link.import_id))
        .chain(
            call_links
                .iter()
                .filter(|link| link.consumer_role == ProgramRole::Server)
                .map(|link| link.result.value_ref()),
        )
        .collect::<BTreeSet<_>>();
    for op in server.plan.regions.iter().flat_map(|region| &region.ops) {
        let boon_plan::PlanOpKind::DerivedValue {
            expression: Some(expression),
            ..
        } = &op.kind
        else {
            continue;
        };
        let mut has_session_info = false;
        expression.visit_intrinsics(&server.plan.row_expressions, &mut |_| {
            has_session_info = true
        })?;
        if has_session_info {
            if let Some(output) = &op.output {
                scoped.insert(output.clone());
            }
        }
    }
    loop {
        let mut changed = false;
        for op in server.plan.regions.iter().flat_map(|region| &region.ops) {
            if !matches!(
                op.kind,
                boon_plan::PlanOpKind::DerivedValue { .. }
                    | boon_plan::PlanOpKind::ListProjection { .. }
                    | boon_plan::PlanOpKind::DependencyEdge
            ) || !op.inputs.iter().any(|input| scoped.contains(input))
            {
                continue;
            }
            if let Some(output) = &op.output {
                changed |= scoped.insert(output.clone());
            }
        }
        if !changed {
            return Ok(scoped);
        }
    }
}

fn validate_distributed_immediate_cycles(
    compiled: &BTreeMap<ProgramRole, CompiledMachinePlanFromSource>,
    call_links: &[CallLink],
) -> Result<(), PlanError> {
    type Node = (ProgramRole, ValueRef);

    let mut labels = BTreeMap::<Node, String>::new();
    for (role, program) in compiled {
        for (path, (_, value)) in machine_plan_backend::distributed_exportable_values(&program.ir) {
            labels
                .entry((*role, value))
                .or_insert_with(|| format!("{}/{path}", role.namespace()));
        }
    }

    let mut edges = BTreeMap::<Node, BTreeSet<Node>>::new();
    for (role, program) in compiled {
        for op in program.plan.regions.iter().flat_map(|region| &region.ops) {
            if !matches!(
                op.kind,
                boon_plan::PlanOpKind::DerivedValue { .. }
                    | boon_plan::PlanOpKind::ListProjection { .. }
                    | boon_plan::PlanOpKind::DependencyEdge
            ) {
                continue;
            }
            let Some(output) = &op.output else {
                continue;
            };
            for input in &op.inputs {
                edges
                    .entry((*role, output.clone()))
                    .or_default()
                    .insert((*role, input.clone()));
            }
        }
        for (reference_index, reference) in program
            .ir
            .distributed_references
            .value_references
            .iter()
            .enumerate()
        {
            let producer_path =
                strip_role_value_prefix(&reference.canonical_path, reference.producer_role)?;
            let reads = program
                .ir
                .scope_index
                .reads
                .iter()
                .filter(|read| {
                    matches!(
                        read.target,
                        boon_ir::ErasedReadTarget::ExternalValue { reference }
                            if reference == reference_index
                    )
                })
                .map(|read| read.id)
                .collect::<BTreeSet<_>>();
            for use_site in program
                .ir
                .scope_index
                .dependencies
                .iter()
                .filter(|use_site| {
                    matches!(
                        use_site.target,
                        boon_ir::ErasedDependencyTarget::ExternalRead { read }
                            if reads.contains(&read)
                    )
                })
                .filter(|use_site| {
                    matches!(use_site.timing, boon_ir::ErasedDependencyTiming::Immediate)
                })
            {
                let dependent = program
                    .ir
                    .scope_index
                    .bindings
                    .get(use_site.dependent.as_usize())
                    .filter(|binding| binding.id == use_site.dependent)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "distributed dependency references missing erased binding {}",
                            use_site.dependent
                        ))
                    })?;
                let dependent = distributed_cycle_binding_value(dependent)?;
                let producer = compiled
                    .get(&reference.producer_role)
                    .and_then(|program| {
                        machine_plan_backend::distributed_exportable_values(&program.ir)
                            .get(&producer_path)
                            .map(|(_, value)| value.clone())
                    })
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "distributed cycle dependency `{}/{producer_path}` has no exact producer value",
                            reference.producer_role.namespace()
                        ))
                    })?;
                edges
                    .entry((*role, dependent))
                    .or_default()
                    .insert((reference.producer_role, producer));
            }
        }
    }

    for (caller_role, caller) in compiled {
        let endpoint = caller.plan.distributed_endpoint.as_ref().ok_or_else(|| {
            PlanError::new(format!(
                "{} machine has no distributed endpoint during cycle validation",
                caller_role.namespace()
            ))
        })?;
        for call in endpoint
            .endpoint
            .remote_call_sites
            .iter()
            .filter(|call| call.mode == DistributedCallMode::Current)
        {
            let call_link = call_links
                .iter()
                .find(|link| {
                    link.consumer_role == *caller_role && link.call_site_id == call.call_site_id
                })
                .ok_or_else(|| {
                    PlanError::new("current distributed call has no canonical call metadata")
                })?;
            let result_import = call.result.current_import_id().ok_or_else(|| {
                PlanError::new(format!(
                    "current distributed function `{}` has no current result import",
                    call_link.call.canonical_function
                ))
            })?;
            let producer = compiled.get(&call.callee_role).ok_or_else(|| {
                PlanError::new(format!(
                    "current distributed function `{}` targets missing {} machine",
                    call_link.call.canonical_function,
                    call.callee_role.namespace()
                ))
            })?;
            let instance = producer
                .plan
                .producer_function_instances
                .iter()
                .find(|instance| instance.call_site_id == call.call_site_id)
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "current distributed function `{}` has no exact producer instance",
                        call_link.call.canonical_function
                    ))
                })?;
            if instance.mode != DistributedCallMode::Current
                || instance.function_export_id != call.function_export_id
            {
                return Err(PlanError::new(format!(
                    "current distributed function `{}` differs from its producer instance",
                    call_link.call.canonical_function
                )));
            }

            let result_node = (*caller_role, ValueRef::DistributedImport(result_import));
            let producer_result = (call.callee_role, instance.result.clone());
            labels.entry(result_node.clone()).or_insert_with(|| {
                let declaration = call_link
                    .owner_path
                    .split_once('@')
                    .map_or(call_link.owner_path.as_str(), |(path, _)| path);
                format!(
                    "{}/{declaration} call {}",
                    caller_role.namespace(),
                    call_link.call.canonical_function
                )
            });
            labels
                .entry(producer_result.clone())
                .or_insert_with(|| format!("{} result", call_link.call.canonical_function));
            edges
                .entry(result_node)
                .or_default()
                .insert(producer_result);

            for argument in &call.arguments {
                let producer_argument = instance
                    .arguments
                    .iter()
                    .find(|candidate| candidate.argument_id == argument.argument_id)
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "current distributed function `{}` argument `{}` has no producer import",
                            call_link.call.canonical_function, argument.name
                        ))
                    })?;
                let argument_node = (
                    call.callee_role,
                    ValueRef::DistributedImport(producer_argument.import_id),
                );
                labels.entry(argument_node.clone()).or_insert_with(|| {
                    format!(
                        "{} argument `{}`",
                        call_link.call.canonical_function, argument.name
                    )
                });
                let mut dependencies = BTreeSet::new();
                caller
                    .plan
                    .row_expressions
                    .visit_inputs(argument.value, &mut |input| {
                        dependencies.insert(input);
                    })?;
                for dependency in dependencies {
                    edges
                        .entry(argument_node.clone())
                        .or_default()
                        .insert((*caller_role, dependency));
                }
            }
        }
    }

    let mut states = BTreeMap::<Node, u8>::new();
    let mut stack = Vec::<Node>::new();
    let nodes = edges.keys().cloned().collect::<Vec<_>>();
    for node in nodes {
        if states.get(&node).copied().unwrap_or(0) != 0 {
            continue;
        }
        if let Some(cycle) = distributed_cycle_from(&node, &edges, &mut states, &mut stack) {
            let detail = cycle
                .iter()
                .map(|node @ (role, _)| {
                    labels
                        .get(node)
                        .cloned()
                        .unwrap_or_else(|| format!("{} internal value", role.namespace()))
                })
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(PlanError::new(format!(
                "distributed combinational cycle requires a SOURCE, HOLD, or asynchronous effect boundary: {detail}"
            )));
        }
    }
    Ok(())
}

fn distributed_cycle_from(
    node: &(ProgramRole, ValueRef),
    edges: &BTreeMap<(ProgramRole, ValueRef), BTreeSet<(ProgramRole, ValueRef)>>,
    states: &mut BTreeMap<(ProgramRole, ValueRef), u8>,
    stack: &mut Vec<(ProgramRole, ValueRef)>,
) -> Option<Vec<(ProgramRole, ValueRef)>> {
    states.insert(node.clone(), 1);
    stack.push(node.clone());
    for dependency in edges.get(node).into_iter().flatten() {
        match states.get(dependency).copied().unwrap_or(0) {
            0 => {
                if let Some(cycle) = distributed_cycle_from(dependency, edges, states, stack) {
                    return Some(cycle);
                }
            }
            1 => {
                let start = stack
                    .iter()
                    .position(|candidate| candidate == dependency)
                    .unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                cycle.push(dependency.clone());
                return Some(cycle);
            }
            _ => {}
        }
    }
    stack.pop();
    states.insert(node.clone(), 2);
    None
}

fn distributed_cycle_binding_value(
    binding: &boon_ir::ErasedBinding,
) -> Result<ValueRef, PlanError> {
    match binding.target {
        boon_ir::ErasedBindingTarget::Value { row: Some(row), .. } => {
            Ok(ValueRef::List(boon_plan::ListId(row.list.as_usize())))
        }
        boon_ir::ErasedBindingTarget::Value {
            field: Some(field), ..
        } => Ok(ValueRef::Field(boon_plan::FieldId(field.as_usize()))),
        boon_ir::ErasedBindingTarget::Source { runtime, .. } => {
            Ok(ValueRef::Source(SourceId(runtime.as_usize())))
        }
        boon_ir::ErasedBindingTarget::State { runtime, .. } => {
            Ok(ValueRef::State(boon_plan::StateId(runtime.as_usize())))
        }
        boon_ir::ErasedBindingTarget::Value { .. } => Err(PlanError::new(format!(
            "distributed cycle dependency binding {} has no exact machine storage",
            binding.id
        ))),
    }
}

fn erased_external_call_owner_path(
    program: &ErasedProgram,
    reference: usize,
    static_owner: Option<boon_ir::StaticOwnerId>,
) -> Result<String, PlanError> {
    let owners = program
        .scope_index
        .dependencies
        .iter()
        .filter_map(|use_site| {
            matches!(
                use_site.target,
                boon_ir::ErasedDependencyTarget::ExternalCall {
                    reference: candidate,
                } if candidate == reference
            )
            .then_some(use_site.dependent)
        })
        .collect::<BTreeSet<_>>();
    let owners = owners
        .into_iter()
        .map(|owner| {
            program
                .scope_index
                .bindings
                .get(owner.as_usize())
                .filter(|binding| binding.id == owner)
                .map(|binding| binding.diagnostic_path.clone())
                .ok_or_else(|| {
                    PlanError::new(format!(
                        "external call dependency references missing erased binding {owner}"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if owners.is_empty() {
        return Err(PlanError::new(format!(
            "distributed call reference {reference} has no exact erased owner"
        )));
    }
    let binding_path = owners.join("&");
    let Some(static_owner) = static_owner else {
        return Ok(binding_path);
    };

    let mut ordinals = Vec::new();
    let mut next = Some(static_owner);
    let mut visited = BTreeSet::new();
    let mut row_owned = false;
    while let Some(owner) = next {
        if !visited.insert(owner) {
            return Err(PlanError::new(format!(
                "distributed call reference {reference} has cyclic static ownership at {owner}"
            )));
        }
        let definition = program
            .scope_index
            .owners
            .iter()
            .find(|definition| definition.id == owner)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "distributed call reference {reference} has missing static owner {owner}"
                ))
            })?;
        row_owned |= definition.authority_row.is_some();
        ordinals.push(definition.child_ordinal);
        next = definition.parent;
    }
    ordinals.reverse();
    let owner_kind = if row_owned { "row-owner" } else { "owner" };
    let owner_path = ordinals
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    Ok(format!("{binding_path}@{owner_kind}:{owner_path}"))
}

fn distributed_call_row_bindings(
    program: &ErasedProgram,
    owner: Option<boon_ir::StaticOwnerId>,
) -> Result<Vec<DistributedCallRowBindingPlan>, PlanError> {
    let mut ancestry = Vec::new();
    let mut next = owner;
    let mut visited = BTreeSet::new();
    while let Some(owner) = next {
        if !visited.insert(owner) {
            return Err(PlanError::new(format!(
                "distributed call has cyclic static ownership at {owner}"
            )));
        }
        let definition = program
            .scope_index
            .owners
            .iter()
            .find(|definition| definition.id == owner)
            .ok_or_else(|| {
                PlanError::new(format!(
                    "distributed call references missing static owner {owner}"
                ))
            })?;
        ancestry.push(owner);
        next = definition.parent;
    }
    ancestry.reverse();

    let mut bindings = Vec::new();
    for owner in ancestry {
        for local in program
            .scope_index
            .locals
            .iter()
            .filter(|local| local.owner == owner)
        {
            let Some(row) = local.row else {
                continue;
            };
            bindings.push(DistributedCallRowBindingPlan {
                owner: PlanStaticOwnerId(owner.as_usize()),
                local: PlanLocalId(local.local.0 as usize),
                list: ListId(row.list.0),
            });
        }
    }
    bindings.sort();
    if bindings
        .windows(2)
        .any(|pair| pair[0].owner == pair[1].owner)
    {
        return Err(PlanError::new(
            "distributed call owner chain contains multiple contextual rows for one owner",
        ));
    }
    Ok(bindings)
}

fn find_function_signature<'a>(
    program: &'a ErasedProgram,
    local_function: &str,
) -> Result<&'a FunctionTypeEntry, PlanError> {
    let matches = program
        .function_types
        .entries
        .iter()
        .filter(|entry| {
            entry.name == local_function
                || local_function
                    .rsplit_once('/')
                    .is_some_and(|(_, suffix)| suffix == entry.name)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [signature] => Ok(*signature),
        [] => Err(PlanError::new(format!(
            "distributed function `{local_function}` has no checked local signature"
        ))),
        _ => Err(PlanError::new(format!(
            "distributed function `{local_function}` has an ambiguous local signature"
        ))),
    }
}

fn strip_role_value_prefix(path: &str, role: ProgramRole) -> Result<String, PlanError> {
    path.strip_prefix(&format!("{}/", role_namespace(role)))
        .map(str::to_owned)
        .ok_or_else(|| {
            PlanError::new(format!(
                "qualified value `{path}` has the wrong role prefix"
            ))
        })
}

fn strip_role_function_prefix(function: &str, role: ProgramRole) -> Result<String, PlanError> {
    function
        .strip_prefix(&format!("{}/", role_namespace(role)))
        .map(str::to_owned)
        .ok_or_else(|| {
            PlanError::new(format!(
                "qualified function `{function}` has the wrong role prefix"
            ))
        })
}

fn role_namespace(role: ProgramRole) -> &'static str {
    role.namespace()
}

fn type_to_data_plan(ty: &Type) -> Option<DataTypePlan> {
    match ty {
        Type::Text => Some(DataTypePlan::Text),
        Type::Number => Some(DataTypePlan::Number),
        Type::Bytes(boon_typecheck::BytesType::Dynamic) => {
            Some(DataTypePlan::Bytes { fixed_len: None })
        }
        Type::Bytes(boon_typecheck::BytesType::Fixed(length)) => Some(DataTypePlan::Bytes {
            fixed_len: u64::try_from(*length).ok(),
        }),
        Type::Object(shape) if !shape.open => Some(DataTypePlan::Record {
            fields: object_fields(shape)?,
            open: false,
        }),
        Type::List(item) => Some(DataTypePlan::List {
            item: Box::new(type_to_data_plan(item)?),
        }),
        Type::VariantSet(variants)
            if boon_typecheck::variants_use_boolean_runtime_representation(variants) =>
        {
            Some(DataTypePlan::Bool)
        }
        Type::VariantSet(variants) => Some(DataTypePlan::Variant {
            variants: variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => Some(DataVariantPlan {
                        tag: tag.clone(),
                        fields: Vec::new(),
                        open: false,
                    }),
                    Variant::Tagged { tag, fields } if !fields.open => Some(DataVariantPlan {
                        tag: tag.clone(),
                        fields: object_fields(fields)?,
                        open: false,
                    }),
                    Variant::Tagged { .. } => None,
                })
                .collect::<Option<Vec<_>>>()?,
        }),
        Type::Object(_)
        | Type::Skip
        | Type::RenderContract
        | Type::Function { .. }
        | Type::UnresolvedShape { .. }
        | Type::Var(_)
        | Type::Unknown => None,
    }
}

fn object_fields(shape: &ObjectShape) -> Option<Vec<DataTypeFieldPlan>> {
    shape
        .fields
        .iter()
        .map(|(name, ty)| {
            Some(DataTypeFieldPlan {
                name: name.clone(),
                data_type: type_to_data_plan(ty)?,
            })
        })
        .collect()
}
