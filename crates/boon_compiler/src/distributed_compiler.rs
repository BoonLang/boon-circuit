use super::{
    CompileProfile, CompiledMachinePlanFromSource, CompilerResult, CompilerSourceUnit,
    compiler_statement_ast_exprs, elapsed_ms, machine_plan_backend, parse_source_units,
};
use boon_ir::{DistributedPureCall, TypedProgram, verify_hidden_identity, verify_static_schedule};
use boon_plan::{
    ApplicationIdentity, DataTypeFieldPlan, DataTypePlan, DataVariantPlan, DistributedArgumentId,
    DistributedDeclarationId, DistributedEndpointContractPlan, DistributedEndpointId,
    DistributedGraphIdentityPlan, DistributedGraphPlan, DistributedPureFunctionExportPlan,
    DistributedValueExportPlan, DistributedValueImportPlan, ExportId, ImportId,
    MigrationPredecessorBinding, PlanError, ProgramRole, RemoteCallSitePlan, TargetProfile,
    ValueRef, verify_plan,
};
use boon_typecheck::{
    ExternalFunctionArgument, ExternalFunctionType, ExternalTypeEnvironment, FlowMode,
    FunctionTypeEntry, ObjectShape, Type, Variant,
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
    parsed: boon_parser::ParsedProgram,
    ir: TypedProgram,
    parse_ms: f64,
    lower_ms: f64,
    verify_ms: f64,
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
    data_type: DataTypePlan,
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
    stable_identity: DistributedDeclarationId,
    call: DistributedPureCall,
    function_export_id: ExportId,
    result_import_id: ImportId,
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

    let server = lower_role(
        requests
            .get(&ProgramRole::Server)
            .expect("validated server request"),
        &ExternalTypeEnvironment::empty(ProgramRole::Server),
    )?;
    let mut session_environment = ExternalTypeEnvironment::empty(ProgramRole::Session);
    extend_external_environment(&mut session_environment, ProgramRole::Server, &server.ir)?;
    let session = lower_role(
        requests
            .get(&ProgramRole::Session)
            .expect("validated session request"),
        &session_environment,
    )?;
    let mut client_environment = ExternalTypeEnvironment::empty(ProgramRole::Client);
    extend_external_environment(&mut client_environment, ProgramRole::Server, &server.ir)?;
    extend_external_environment(&mut client_environment, ProgramRole::Session, &session.ir)?;
    let client = lower_role(
        requests
            .get(&ProgramRole::Client)
            .expect("validated client request"),
        &client_environment,
    )?;

    let lowered = [client, session, server]
        .into_iter()
        .map(|program| (program.request.role, program))
        .collect::<BTreeMap<_, _>>();
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

fn lower_role(
    request: &DistributedCompilerProgram,
    external: &ExternalTypeEnvironment,
) -> CompilerResult<LoweredRole> {
    let parse_started = Instant::now();
    let parsed = parse_source_units(&request.source_label, &request.units)?;
    let parse_ms = elapsed_ms(parse_started);
    let lower_started = Instant::now();
    let ir = boon_ir::lower_runtime_with_external_types(&parsed, external)?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    Ok(LoweredRole {
        request: request.clone(),
        parsed,
        ir,
        parse_ms,
        lower_ms,
        verify_ms,
    })
}

fn extend_external_environment(
    environment: &mut ExternalTypeEnvironment,
    producer_role: ProgramRole,
    program: &TypedProgram,
) -> Result<(), PlanError> {
    let namespace = role_namespace(producer_role);
    for (path, (flow_type, _)) in machine_plan_backend::distributed_exportable_values(program) {
        if flow_type.mode != FlowMode::Continuous || type_to_data_plan(&flow_type.ty).is_none() {
            continue;
        }
        let canonical = format!("{namespace}.{path}");
        if environment
            .values
            .insert(canonical.clone(), flow_type)
            .is_some()
        {
            return Err(PlanError::new(format!(
                "distributed external value `{canonical}` is declared more than once"
            )));
        }
    }
    for function in &program.typecheck_report.function_type_table.entries {
        if function.args.len() != function.arg_types.len()
            || function.result.mode != FlowMode::Continuous
            || type_to_data_plan(&function.result.ty).is_none()
            || function
                .arg_types
                .iter()
                .any(|argument| type_to_data_plan(argument).is_none())
        {
            continue;
        }
        let canonical = format!("{namespace}/{}", function.name);
        let signature = ExternalFunctionType {
            args: function
                .args
                .iter()
                .cloned()
                .zip(function.arg_types.iter().cloned())
                .map(|(name, ty)| ExternalFunctionArgument { name, ty })
                .collect(),
            result: function.result.clone(),
            pure: true,
        };
        if environment
            .functions
            .insert(canonical.clone(), signature)
            .is_some()
        {
            return Err(PlanError::new(format!(
                "distributed external function `{canonical}` is declared more than once"
            )));
        }
    }
    Ok(())
}

fn link_lowered_roles(
    lowered: BTreeMap<ProgramRole, LoweredRole>,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledDistributedMachinePlans> {
    let client = lowered
        .get(&ProgramRole::Client)
        .expect("validated client role");
    let graph_revision = lowered
        .values()
        .map(|program| program.request.revision)
        .max()
        .unwrap_or(1);
    let graph_stable_identity = DistributedDeclarationId::from_semantic_path(
        &client.request.application.package_id,
        "Client+Session+Server",
    )?;
    let graph_identity = DistributedGraphIdentityPlan::new(
        &client.request.application,
        graph_stable_identity,
        graph_revision,
    )?;
    let endpoints = [
        ProgramRole::Client,
        ProgramRole::Session,
        ProgramRole::Server,
    ]
    .into_iter()
    .map(|role| {
        let stable_identity = DistributedDeclarationId::from_semantic_path(
            &client.request.application.package_id,
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

    for (consumer_role, consumer) in &lowered {
        for reference in &consumer.ir.distributed_references.value_references {
            let producer_role = reference.producer_role;
            let local_path = strip_role_value_prefix(&reference.canonical_path, producer_role)?;
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
                    boon_plan::DistributedExportKind::Value,
                    export_stable_identity,
                )?;
                let import_stable_identity = DistributedDeclarationId::from_semantic_path(
                    role_namespace(*consumer_role),
                    &format!("import:{}", reference.canonical_path),
                )?;
                let import_id = ImportId::from_value_identity(
                    graph_identity.graph_id,
                    consumer_endpoint.endpoint_id,
                    import_stable_identity,
                )?;
                let link = ValueLink {
                    consumer_role: *consumer_role,
                    producer_role,
                    canonical_path: reference.canonical_path.clone(),
                    local_path,
                    export_stable_identity,
                    export_id,
                    import_stable_identity,
                    import_id,
                    data_type,
                };
                value_links.insert(key, link.clone());
                link
            };
            let context = contexts.get_mut(consumer_role).expect("consumer context");
            context.expression_refs.insert(
                reference.expr_id.as_usize(),
                ValueRef::DistributedImport(link.import_id),
            );
            context.path_refs.insert(
                reference.canonical_path.clone(),
                ValueRef::DistributedImport(link.import_id),
            );
        }

        for call in &consumer.ir.distributed_references.pure_calls {
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
                    boon_plan::DistributedExportKind::PureFunction,
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
            let owner_path = distributed_root_owner(&consumer.ir, call.expr_id.as_usize())?;
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
            let result_import_id = ImportId::from_remote_call_result(call_site_id)?;
            contexts
                .get_mut(consumer_role)
                .expect("consumer context")
                .expression_refs
                .insert(
                    call.expr_id.as_usize(),
                    ValueRef::DistributedImport(result_import_id),
                );
            call_links.push(CallLink {
                consumer_role: *consumer_role,
                owner_path,
                stable_identity,
                call: call.clone(),
                function_export_id: function.export_id,
                result_import_id,
            });
        }
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
                parsed: program.parsed.clone(),
                ir: program.ir.clone(),
                plan,
                profile: CompileProfile {
                    source_unit_count: program.parsed.files.len(),
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
    for link in value_links.values() {
        if value_exports.contains_key(&link.export_id) {
            continue;
        }
        let producer = compiled
            .get(&link.producer_role)
            .expect("compiled producer");
        let local_values = machine_plan_backend::distributed_exportable_values(&producer.ir);
        let (_, value_ref) = local_values.get(&link.local_path).cloned().ok_or_else(|| {
            PlanError::new(format!(
                "distributed value `{}` has no executable producer value",
                link.canonical_path
            ))
        })?;
        let export = DistributedValueExportPlan::new(
            graph_identity.graph_id,
            endpoints
                .get(&link.producer_role)
                .expect("producer endpoint")
                .endpoint_id,
            link.export_stable_identity,
            lowered
                .get(&link.producer_role)
                .expect("producer")
                .request
                .revision,
            link.producer_role,
            value_ref,
            link.data_type.clone(),
        )?;
        if export.export_id != link.export_id {
            return Err(
                PlanError::new("distributed value export ID changed during linking").into(),
            );
        }
        value_exports.insert(export.export_id, export);
    }

    let mut function_exports = BTreeMap::<ExportId, DistributedPureFunctionExportPlan>::new();
    for function in function_links.values() {
        let parameters = function
            .signature
            .args
            .iter()
            .cloned()
            .zip(function.signature.arg_types.iter())
            .map(|(name, ty)| {
                type_to_data_plan(ty)
                    .map(|data_type| (name, data_type))
                    .ok_or_else(|| {
                        PlanError::new(format!(
                            "distributed function `{}` argument is not a closed boundary type",
                            function.canonical_function
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let parameter_ids = parameters
            .iter()
            .map(|(name, _)| {
                Ok((
                    name.clone(),
                    DistributedArgumentId::from_parameter_name(function.export_id, name)?,
                ))
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
        let producer = compiled
            .get_mut(&function.producer_role)
            .expect("compiled function producer");
        let body = machine_plan_backend::lower_distributed_pure_function_body(
            &producer.ir,
            &function.local_function,
            function.export_id,
            &parameter_ids,
            &mut producer.plan.constants,
        )?;
        let export = DistributedPureFunctionExportPlan::new(
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
            body,
        )?;
        if export.export_id != function.export_id {
            return Err(
                PlanError::new("distributed function export ID changed during linking").into(),
            );
        }
        function_exports.insert(export.export_id, export);
    }

    let mut value_imports = BTreeMap::<ProgramRole, Vec<DistributedValueImportPlan>>::new();
    for link in value_links.values() {
        if value_imports
            .get(&link.consumer_role)
            .is_some_and(|imports| {
                imports
                    .iter()
                    .any(|import| import.import_id == link.import_id)
            })
        {
            continue;
        }
        let source = value_exports
            .get(&link.export_id)
            .expect("linked value export");
        let import = DistributedValueImportPlan::new(
            graph_identity.graph_id,
            endpoints
                .get(&link.consumer_role)
                .expect("consumer endpoint")
                .endpoint_id,
            link.import_stable_identity,
            lowered
                .get(&link.consumer_role)
                .expect("consumer")
                .request
                .revision,
            link.consumer_role,
            source,
        )?;
        if import.import_id != link.import_id {
            return Err(
                PlanError::new("distributed value import ID changed during linking").into(),
            );
        }
        value_imports
            .entry(link.consumer_role)
            .or_default()
            .push(import);
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
                        argument.expr_id.as_usize(),
                        &mut consumer.plan.constants,
                        &context,
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, PlanError>>()?;
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
            function,
            arguments,
        )?;
        if call.result_import_id != link.result_import_id {
            return Err(
                PlanError::new("remote call result import ID changed during linking").into(),
            );
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
            function_exports
                .values()
                .filter(|export| export.producer_role == role)
                .cloned()
                .collect(),
            remote_calls.remove(&role).unwrap_or_default(),
        )?;
        endpoint_contracts.push(contract);
    }
    let graph = DistributedGraphPlan::new(
        &client.request.application,
        graph_identity,
        endpoint_contracts,
    )?;

    for (role, program) in &mut compiled {
        program.plan.distributed_endpoint = graph.endpoint_plan(*role);
        program.plan.capability_summary.constant_count = program.plan.constants.len();
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

fn distributed_root_owner(program: &TypedProgram, expr_id: usize) -> Result<String, PlanError> {
    let mut candidates = program
        .derived_values
        .iter()
        .filter(|derived| !derived.indexed && derived.scope_id.is_none())
        .filter(|derived| {
            compiler_statement_ast_exprs(&derived.statement, &program.expressions)
                .iter()
                .any(|expression| expression.id == expr_id)
        })
        .map(|derived| {
            (
                derived
                    .statement
                    .end
                    .saturating_sub(derived.statement.start),
                derived.path.clone(),
            )
        })
        .collect::<Vec<_>>();
    candidates.sort();
    match candidates.as_slice() {
        [(_, owner), ..]
            if candidates
                .get(1)
                .is_none_or(|candidate| candidate.0 > candidates[0].0) =>
        {
            Ok(owner.clone())
        }
        [] => Err(PlanError::new(format!(
            "remote call expression {expr_id} is not owned by one non-indexed root value; calls inside reusable functions, documents, or list rows need scheduled call-site identity"
        ))),
        _ => Err(PlanError::new(format!(
            "remote call expression {expr_id} has ambiguous scheduled ownership"
        ))),
    }
}

fn find_function_signature<'a>(
    program: &'a TypedProgram,
    local_function: &str,
) -> Result<&'a FunctionTypeEntry, PlanError> {
    let matches = program
        .typecheck_report
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
    path.strip_prefix(&format!("{}.", role_namespace(role)))
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
    match role {
        ProgramRole::Client => "Client",
        ProgramRole::Session => "Session",
        ProgramRole::Server => "Server",
    }
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
        Type::VariantSet(variants) if bool_variant_set(variants) => Some(DataTypePlan::Bool),
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
        | Type::List(_)
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

fn bool_variant_set(variants: &[Variant]) -> bool {
    let tags = variants
        .iter()
        .filter_map(|variant| match variant {
            Variant::Tag(tag) => Some(tag.as_str()),
            Variant::Tagged { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    tags == BTreeSet::from(["False", "True"]) && variants.len() == 2
}
