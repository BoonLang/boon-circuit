#[cfg(test)]
use boon_parser::ParsedProgram;
use boon_semantic::program_core::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(all(test, not(target_arch = "wasm32")))]
use std::time::Instant;
#[cfg(all(test, target_arch = "wasm32"))]
use web_time::Instant;

/// Opaque executable IR whose complete provenance is fixed at the sole
/// verification-gated lowering boundary.
///
/// External crates cannot construct the wrapper:
///
/// ```compile_fail
/// use boon_ir::ErasedProgram;
/// use boon_semantic::program_core::CanonicalProgramCoreV1;
///
/// fn forge(fields: CanonicalProgramCoreV1) -> ErasedProgram {
///     ErasedProgram {
///         fields,
///         source_bundle_digest_v1: todo!(),
///         semantic_program_digest: todo!(),
///         verification_manifest_digest: todo!(),
///     }
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErasedProgram {
    #[serde(flatten)]
    fields: CanonicalProgramCoreV1,
    source_bundle_digest_v1: boon_contract::SourceBundleDigestV1,
    semantic_program_digest: boon_semantic::SemanticProgramDigestV1,
    verification_manifest_digest: boon_verify::VerificationManifestDigestV1,
}

/// Opaque result of atomically erasing one verified Client/Session/Server
/// semantic bundle.
///
/// The role programs cannot be supplied independently: construction consumes
/// one [`boon_verify::ContractVerifiedBundle`] and checks every frozen
/// cross-role occurrence against the executable references emitted for its
/// consumer role.
pub struct ErasedBundle {
    role_programs: [(boon_checked::ProgramRole, ErasedProgram); 3],
    bundle_semantic_program_digest: boon_semantic::BundleSemanticProgramDigestV1,
    bundle_verification_manifest_digest: boon_verify::BundleVerificationManifestDigestV1,
}

impl ErasedBundle {
    pub const fn bundle_semantic_program_digest(
        &self,
    ) -> boon_semantic::BundleSemanticProgramDigestV1 {
        self.bundle_semantic_program_digest
    }

    pub const fn bundle_verification_manifest_digest(
        &self,
    ) -> boon_verify::BundleVerificationManifestDigestV1 {
        self.bundle_verification_manifest_digest
    }

    pub fn role_program(&self, role: boon_checked::ProgramRole) -> Option<&ErasedProgram> {
        self.role_programs
            .iter()
            .find_map(|(candidate, program)| (*candidate == role).then_some(program))
    }

    pub fn into_role_programs(self) -> [(boon_checked::ProgramRole, ErasedProgram); 3] {
        self.role_programs
    }
}

impl std::ops::Deref for ErasedProgram {
    type Target = CanonicalProgramCoreV1;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl ErasedProgram {
    pub const fn source_bundle_digest_v1(&self) -> boon_contract::SourceBundleDigestV1 {
        self.source_bundle_digest_v1
    }

    pub const fn executable(&self) -> &ExecutableProgram {
        &self.fields.executable
    }

    pub const fn scope_index(&self) -> &ErasedScopeIndex {
        &self.fields.scope_index
    }

    pub const fn expression_count(&self) -> usize {
        self.fields.expression_count
    }

    pub const fn role_references(&self) -> &DistributedReferences {
        &self.fields.distributed_references
    }

    pub fn producer_function_instances(&self) -> &[ProducerFunctionInstance] {
        &self.fields.producer_function_instances
    }

    pub fn debug_source_units(&self) -> &[SemanticSourceUnit] {
        &self.fields.debug_source_units
    }

    pub fn debug_fields(&self) -> &[SemanticFieldEntry] {
        &self.fields.debug_fields
    }

    pub const fn graph_node_count(&self) -> usize {
        self.fields.graph_node_count
    }

    pub fn sources(&self) -> &[SourcePort] {
        &self.fields.sources
    }

    pub fn host_ports(&self) -> &[HostPortDeclaration] {
        &self.fields.host_ports
    }

    pub fn state_cells(&self) -> &[StateCell] {
        &self.fields.state_cells
    }

    pub fn activations(&self) -> &[ActivationSite] {
        &self.fields.activations
    }

    pub fn pulse_batches(&self) -> &[PulseBatch] {
        &self.fields.pulse_batches
    }

    pub fn lists(&self) -> &[ListMemory] {
        &self.fields.lists
    }

    pub fn semantic_memory(&self) -> &[SemanticMemory] {
        &self.fields.semantic_memory
    }

    pub fn migration_edges(&self) -> &[MigrationEdge] {
        &self.fields.migration_edges
    }

    pub fn transient_collections(&self) -> &[TransientCollection] {
        &self.fields.transient_collections
    }

    pub fn output_values(&self) -> &[OutputRootValue] {
        &self.fields.output_values
    }

    pub fn derived_values(&self) -> &[DerivedValue] {
        &self.fields.derived_values
    }

    pub fn dependencies(&self) -> &[DependencyEdge] {
        &self.fields.dependencies
    }

    pub fn state_updates(&self) -> &[StateUpdateArm] {
        &self.fields.state_update_arms
    }

    pub fn list_mutations(&self) -> &[ListMutation] {
        &self.fields.list_mutations
    }

    pub fn list_projections(&self) -> &[ListProjection] {
        &self.fields.list_projections
    }

    pub fn materializations(&self) -> &[ContextualMaterialization] {
        &self.fields.materializations
    }

    pub fn view_bindings(&self) -> &[ViewBinding] {
        &self.fields.view_bindings
    }

    pub const fn expression_types(&self) -> &boon_checked::ExprTypeTable {
        &self.fields.expression_types
    }

    pub const fn function_types(&self) -> &boon_checked::FunctionTypeTable {
        &self.fields.function_types
    }

    pub const fn named_value_types(&self) -> &boon_checked::NamedValueTypeTable {
        &self.fields.named_value_types
    }

    pub const fn semantic_program_digest(&self) -> boon_semantic::SemanticProgramDigestV1 {
        self.semantic_program_digest
    }

    pub const fn verification_manifest_digest(&self) -> boon_verify::VerificationManifestDigestV1 {
        self.verification_manifest_digest
    }
}

/// The sole production semantic-to-executable lowering boundary.
///
/// `ContractVerifiedProgram` has private construction in `boon_verify`, so a
/// checked or semantic program cannot reach executable IR without first
/// passing the completeness-checked verification manifest.
pub fn erase_and_lower(
    verified: boon_verify::ContractVerifiedProgram,
) -> Result<ErasedProgram, String> {
    let (semantic, verification_manifest) = verified.into_lowering_parts();
    erase_semantic_program(semantic, verification_manifest)
}

/// Atomically erases the exact three-role semantic bundle owned by one
/// verification token.
///
/// No role artifact is returned unless all roles lower successfully and the
/// complete frozen call/value crossing sets map one-to-one to their executable
/// consumer references.
pub fn erase_and_lower_bundle(
    verified: boon_verify::ContractVerifiedBundle,
) -> Result<ErasedBundle, String> {
    let (semantic_bundle, role_verifications, bundle_manifest) = verified.into_lowering_parts();
    // ContractVerifiedBundle is a non-forgeable ownership token constructed
    // only after the semantic bundle and every verification manifest validate.
    // Keep the exact digest joins below without repeating those full graph
    // traversals after ownership has transferred.
    if semantic_bundle.digest() != bundle_manifest.bundle_semantic_program_digest {
        return Err("verified bundle semantic digest differs from its bundle manifest".to_owned());
    }

    let bundle_semantic_program_digest = semantic_bundle.digest();
    let bundle_verification_manifest_digest = bundle_manifest.manifest_digest;
    let call_crossings = semantic_bundle.call_crossings().to_vec();
    let value_crossings = semantic_bundle.value_crossings().to_vec();
    let mut manifests = role_verifications.into_iter().collect::<BTreeMap<_, _>>();
    if manifests.len() != 3 {
        return Err(format!(
            "verified bundle carries {} role manifests, expected 3",
            manifests.len()
        ));
    }

    let mut lowered = Vec::with_capacity(3);
    for semantic in semantic_bundle.into_role_programs() {
        let role = semantic.role();
        let manifest = manifests.remove(&role).ok_or_else(|| {
            format!(
                "verified bundle is missing the {} role manifest",
                role.namespace()
            )
        })?;
        if manifest.requirements.semantic_program_digest != semantic.digest() {
            return Err(format!(
                "verified {} role manifest does not match its semantic program",
                role.namespace()
            ));
        }
        let mut program = erase_semantic_program(semantic, manifest)?;
        validate_erased_bundle_role_crossings(
            role,
            &mut program,
            &call_crossings,
            &value_crossings,
        )?;
        lowered.push((role, program));
    }
    if !manifests.is_empty() {
        return Err("verified bundle contains manifests for unsupported roles".to_owned());
    }
    let role_programs: [(boon_checked::ProgramRole, ErasedProgram); 3] =
        lowered.try_into().map_err(|programs: Vec<_>| {
            format!(
                "verified bundle erased {} role programs, expected 3",
                programs.len()
            )
        })?;
    let expected_roles = [
        boon_checked::ProgramRole::Client,
        boon_checked::ProgramRole::Session,
        boon_checked::ProgramRole::Server,
    ];
    if role_programs
        .iter()
        .map(|(role, _)| *role)
        .ne(expected_roles)
    {
        return Err("erased bundle role programs are not in canonical order".to_owned());
    }
    Ok(ErasedBundle {
        role_programs,
        bundle_semantic_program_digest,
        bundle_verification_manifest_digest,
    })
}

fn erase_semantic_program(
    semantic: boon_semantic::SemanticProgram,
    verification_manifest: boon_verify::VerificationManifestV1,
) -> Result<ErasedProgram, String> {
    // The private ContractVerifiedProgram constructor has already validated
    // both artifacts. The digest join is retained as the erasure boundary's
    // exact ownership check; rescanning the same immutable semantic graph here
    // would not add a new trust boundary.
    if verification_manifest.requirements.semantic_program_digest != semantic.digest() {
        return Err(
            "verification manifest semantic digest differs from its semantic program".to_owned(),
        );
    }
    let verification_manifest_digest = verification_manifest.manifest_digest;
    let pulse_fusion_decisions = verification_manifest.pulse_fusion_decisions;
    let (source_bundle_digest_v1, mut fields, semantic_program_digest, _dependency_manifest_digest) =
        semantic.into_lowering_parts();
    bind_verified_pulse_fusion(&mut fields, &pulse_fusion_decisions)?;
    let erased = ErasedProgram {
        fields,
        source_bundle_digest_v1,
        semantic_program_digest,
        verification_manifest_digest,
    };
    verify_erased_scope_index(&erased)?;
    verify_static_schedule(&erased)?;
    verify_hidden_identity(&erased)?;
    Ok(erased)
}

#[cfg(test)]
fn lower(program: &ParsedProgram) -> Result<ErasedProgram, String> {
    lower_with_typecheck(program, &boon_checked::ExternalTypeEnvironment::default())
}

#[cfg(test)]
fn lower_with_typecheck(
    program: &ParsedProgram,
    external_types: &boon_checked::ExternalTypeEnvironment,
) -> Result<ErasedProgram, String> {
    let trace_lower = std::env::var_os("BOON_IR_LOWER_TRACE").is_some();
    let trace_phase = |phase: &str, elapsed_ms: f64| {
        if trace_lower {
            eprintln!("boon_ir lower {phase}: {elapsed_ms:.3}ms");
        }
    };
    let typecheck_started = Instant::now();
    if trace_lower {
        eprintln!("boon_ir lower typecheck:start");
    }
    let check_output =
        boon_typecheck::check_runtime_program_profiled_with_external_types(program, external_types)
            .0;
    let typecheck_report = check_output.report;
    let typecheck_ms = lower_elapsed_ms(typecheck_started);
    trace_phase("typecheck", typecheck_ms);
    if typecheck_report.has_errors() {
        let mut failures = typecheck_report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == boon_checked::DiagnosticSeverity::Error)
            .map(|diagnostic| {
                let location = program
                    .files
                    .iter()
                    .filter(|file| file.start_line <= diagnostic.line)
                    .max_by_key(|file| file.start_line)
                    .map_or_else(
                        || format!("line {}", diagnostic.line),
                        |file| {
                            format!(
                                "{}:{}",
                                file.path,
                                diagnostic
                                    .line
                                    .saturating_sub(file.start_line)
                                    .saturating_add(1)
                            )
                        },
                    );
                format!("{location}: {}", diagnostic.message)
            })
            .collect::<Vec<_>>();
        failures.extend(
            typecheck_report
                .render_slot_table
                .slots
                .iter()
                .flat_map(|slot| {
                    slot.diagnostics
                        .iter()
                        .filter(|diagnostic| {
                            diagnostic.severity == boon_checked::DiagnosticSeverity::Error
                        })
                        .map(|diagnostic| {
                            format!(
                                "render slot `{}` at line {}: {}",
                                slot.slot_name, diagnostic.line, diagnostic.message
                            )
                        })
                }),
        );
        let messages = failures.join("; ");
        return Err(format!(
            "typecheck failed with {} error diagnostic(s): {messages}",
            failures.len(),
        ));
    }
    let checked_program = check_output
        .program
        .ok_or_else(|| "typecheck produced no CheckedProgram for valid source".to_owned())?;
    let semantic =
        boon_semantic::elaborate(checked_program, &[]).map_err(|error| error.to_string())?;
    let verified =
        boon_verify::verify_explicit_contracts(semantic).map_err(|error| error.to_string())?;
    erase_and_lower(verified)
}

fn bind_verified_pulse_fusion(
    fields: &mut CanonicalProgramCoreV1,
    decisions: &[boon_verify::VerifiedPulseFusionDecisionV1],
) -> Result<(), String> {
    if decisions.len() != fields.pulse_batches.len() {
        return Err(format!(
            "verification manifest has {} pulse-fusion decisions for {} canonical pulse batches",
            decisions.len(),
            fields.pulse_batches.len()
        ));
    }
    for (index, (batch, decision)) in fields.pulse_batches.iter_mut().zip(decisions).enumerate() {
        if batch.id != PulseBatchId(index)
            || decision.pulse_batch.as_usize() != index
            || decision.semantic_slice_digest.0 != batch.semantic_slice_digest
            || batch.fusion != PulseFusionEligibility::PendingVerification
        {
            return Err(format!(
                "pulse-fusion decision {} does not bind pending canonical pulse batch {} and its exact slice digest",
                decision.pulse_batch, batch.id
            ));
        }
        batch.fusion = match &decision.status {
            boon_verify::VerifiedPulseFusionStatusV1::Eligible { fact } => {
                if fact.count_policy
                    != boon_verify::VerifiedPulseFusionCountPolicyV1::FrozenAndRuntimeTargetGuardedBeforeFirstMicroturn
                    || fact.elision_policy
                        != boon_verify::VerifiedPulseFusionElisionPolicyV1::ElideOnlyUnobservedRecurrenceStateRouting
                {
                    return Err(format!(
                        "verified pulse-fusion fact for batch {} uses an unsupported proof policy",
                        batch.id
                    ));
                }
                let proof = match (fact.trace_policy, batch.list_mutations.is_empty()) {
                    (
                        boon_verify::VerifiedPulseFusionTracePolicyV1::PreserveCommittedStateDeltasAndEmissionRoutes,
                        true,
                    ) => PulseFusionProof::FrozenRuntimeTargetGuardedFullTraceEmptySideLanes,
                    (
                        boon_verify::VerifiedPulseFusionTracePolicyV1::PreserveCommittedStateAndListDeltasAndEmissionRoutes,
                        false,
                    ) => PulseFusionProof::FrozenRuntimeTargetGuardedFullTracePreservedListMutations,
                    _ => {
                        return Err(format!(
                            "verified pulse-fusion fact for batch {} changed its list-mutation trace policy",
                            batch.id
                        ));
                    }
                };
                let activation = ActivationId(fact.activation.as_usize());
                let state = StateId(fact.state.as_usize());
                if batch.enclosing_activation != Some(activation)
                    || batch.state != Some(state)
                    || fields
                        .activations
                        .get(activation.as_usize())
                        .is_none_or(|candidate| {
                            candidate.id != activation || !candidate.states.contains(&state)
                        })
                    || fields
                        .state_cells
                        .get(state.as_usize())
                        .is_none_or(|candidate| candidate.id != state)
                {
                    return Err(format!(
                        "verified pulse-fusion fact for batch {} changed its activation-local state",
                        batch.id
                    ));
                }
                let update = fields
                    .state_update_arms
                    .get(fact.state_update_arm.as_usize())
                    .filter(|update| update.state == state)
                    .ok_or_else(|| {
                        format!(
                            "verified pulse-fusion fact for batch {} references missing update arm {}",
                            batch.id, fact.state_update_arm
                        )
                    })?;
                let state_update_arm_index = batch
                    .state_update_arms
                    .iter()
                    .position(|candidate| candidate == update)
                    .ok_or_else(|| {
                        format!(
                            "verified pulse-fusion fact for batch {} lost update arm {}",
                            batch.id, fact.state_update_arm
                        )
                    })?;
                PulseFusionEligibility::VerifiedActivationLocalRecurrence {
                    activation,
                    state,
                    state_update_arm_index,
                    proof,
                }
            }
            boon_verify::VerifiedPulseFusionStatusV1::Ineligible { reasons } => {
                PulseFusionEligibility::Ineligible {
                    diagnostics: reasons
                        .iter()
                        .map(|reason| reason.diagnostic().to_owned())
                        .collect(),
                }
            }
        };
    }
    Ok(())
}

fn validate_erased_bundle_role_crossings(
    role: boon_checked::ProgramRole,
    program: &mut ErasedProgram,
    call_crossings: &[boon_semantic::BundleSemanticCallCrossingV1],
    value_crossings: &[boon_semantic::BundleSemanticValueCrossingV1],
) -> Result<(), String> {
    let expected_calls = call_crossings
        .iter()
        .filter(|crossing| crossing.consumer_role == role)
        .collect::<Vec<_>>();
    if expected_calls.len() != program.role_references().calls.len() {
        return Err(format!(
            "{} verified bundle has {} call crossings but erasure emitted {} distributed calls",
            role.namespace(),
            expected_calls.len(),
            program.role_references().calls.len()
        ));
    }
    let mut matched_calls = BTreeSet::new();
    for crossing in expected_calls {
        let expression = exact_erased_expression(program, crossing.consumer_expression.as_usize())?;
        let matches = program
            .role_references()
            .calls
            .iter()
            .enumerate()
            .filter(|(_, call)| call.expression == expression)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [index] = matches.as_slice() else {
            return Err(format!(
                "{} bundle call crossing `{}` maps to {} executable calls",
                role.namespace(),
                crossing.occurrence_path,
                matches.len()
            ));
        };
        if !matched_calls.insert(*index) {
            return Err(format!(
                "{} executable distributed call {} is claimed by multiple verified crossings",
                role.namespace(),
                index
            ));
        }
        // The frozen verified bundle, not a role-local path reconstruction,
        // owns the cross-role occurrence identity.
        program.fields.distributed_references.calls[*index].occurrence_path =
            crossing.occurrence_path.clone();
        let call = &program.fields.distributed_references.calls[*index];
        let expected_arguments = crossing
            .arguments
            .iter()
            .filter_map(|argument| match &argument.binding {
                boon_semantic::BundleSemanticCallArgumentBindingV1::Explicit {
                    expression,
                    flow_type,
                    ..
                } => Some(
                    exact_erased_expression(program, expression.as_usize())
                        .map(|value| (argument.name.as_str(), value, flow_type)),
                ),
                boon_semantic::BundleSemanticCallArgumentBindingV1::Omitted => None,
            })
            .collect::<Result<Vec<_>, _>>()?;
        let arguments_match = call.arguments.len() == expected_arguments.len()
            && call.arguments.iter().zip(&expected_arguments).all(
                |(actual, (name, value, flow_type))| {
                    actual.name == *name
                        && actual.value == *value
                        && actual.flow_type == **flow_type
                },
            );
        let mode_matches = match crossing.mode {
            boon_semantic::ProducerMaterializationMode::Current => call.invocation_arms.is_empty(),
            boon_semantic::ProducerMaterializationMode::Invocation => {
                !call.invocation_arms.is_empty()
            }
        };
        if call.owner != crossing.owner
            || call.occurrence_path != crossing.occurrence_path
            || call.canonical_function != crossing.canonical_function
            || call.producer_role != crossing.producer_role
            || call.result != crossing.result
            || call.effect != crossing.effect
            || !arguments_match
            || !mode_matches
        {
            return Err(format!(
                "{} executable distributed call {} differs from verified crossing `{}`",
                role.namespace(),
                index,
                crossing.occurrence_path
            ));
        }
    }

    let expected_values = value_crossings
        .iter()
        .filter(|crossing| crossing.consumer_role == role)
        .collect::<Vec<_>>();
    if expected_values.len() != program.role_references().value_references.len() {
        return Err(format!(
            "{} verified bundle has {} value crossings but erasure emitted {} distributed values",
            role.namespace(),
            expected_values.len(),
            program.role_references().value_references.len()
        ));
    }
    let mut matched_values = BTreeSet::new();
    for crossing in expected_values {
        let expression = exact_erased_expression(program, crossing.consumer_expression.as_usize())?;
        let executable = program
            .executable
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
            .ok_or_else(|| {
                format!(
                    "{} bundle value crossing `{}` maps to missing executable expression {expression}",
                    role.namespace(),
                    crossing.occurrence_path
                )
            })?;
        if executable.checked_expr_id != crossing.checked_expression {
            return Err(format!(
                "{} bundle value crossing `{}` changed checked expression identity during erasure",
                role.namespace(),
                crossing.occurrence_path
            ));
        }
        let matches = program
            .role_references()
            .value_references
            .iter()
            .enumerate()
            .filter(|(_, reference)| {
                reference.expr_id == ExprId(crossing.checked_expression.0 as usize)
                    && reference.canonical_path == crossing.canonical_path
                    && reference.producer_role == crossing.producer_role
            })
            .collect::<Vec<_>>();
        let [(index, reference)] = matches.as_slice() else {
            return Err(format!(
                "{} bundle value crossing `{}` maps to {} executable value references",
                role.namespace(),
                crossing.occurrence_path,
                matches.len()
            ));
        };
        if !matched_values.insert(*index) {
            return Err(format!(
                "{} executable distributed value {} is claimed by multiple verified crossings",
                role.namespace(),
                index
            ));
        }
        if reference.flow_mode != crossing.flow_type.mode
            || reference.value_type != crossing.flow_type.ty
        {
            return Err(format!(
                "{} executable distributed value {} differs from verified crossing `{}`",
                role.namespace(),
                index,
                crossing.occurrence_path
            ));
        }
        if matches!(
            crossing.delivery,
            boon_semantic::BundleSemanticValueDeliveryV1::Event { .. }
                | boon_semantic::BundleSemanticValueDeliveryV1::RelayedEvent { .. }
        ) {
            ensure_erased_distributed_ingress_source(
                program,
                &distributed_event_source_path(&crossing.canonical_path),
            )?;
        }
    }
    verify_erased_scope_index(program)?;
    verify_static_schedule(program)?;
    verify_hidden_identity(program)?;
    Ok(())
}

fn exact_erased_expression(
    program: &ErasedProgram,
    semantic_index: usize,
) -> Result<ExecutableExprId, String> {
    let expression = ExecutableExprId(semantic_index);
    program
        .executable
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .map(|_| expression)
        .ok_or_else(|| {
            format!(
                "semantic expression {semantic_index} has no exact expression in the erased dense arena"
            )
        })
}

fn ensure_erased_distributed_ingress_source(
    program: &mut ErasedProgram,
    path: &str,
) -> Result<SourceId, String> {
    let matches = program
        .fields
        .sources
        .iter()
        .filter(|source| source.path == path)
        .map(|source| source.id)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [source] => return Ok(*source),
        [] => {}
        _ => {
            return Err(format!(
                "distributed ingress `{path}` resolves to {} source lanes",
                matches.len()
            ));
        }
    }
    let id = SourceId(program.fields.sources.len());
    program.fields.sources.push(SourcePort {
        id,
        path: path.to_owned(),
        binding_path: path.to_owned(),
        executable_source_id: None,
        static_owner: None,
        source_expr_id: None,
        source_line: 0,
        scoped: false,
        scope_id: None,
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
        },
    });
    program.fields.scope_index.sources.push(ErasedSourceDef {
        source: id,
        static_owner: None,
        owner_ancestry: Vec::new(),
        origin: ErasedSourceOrigin::DistributedImport,
    });
    Ok(id)
}

fn executable_statement_parents(
    executable: &ExecutableProgram,
) -> BTreeMap<ExecutableStatementId, ExecutableStatementId> {
    executable
        .statements
        .iter()
        .flat_map(|parent| parent.children.iter().map(move |child| (*child, parent.id)))
        .collect()
}

fn direct_erased_storage_statements(
    executable: &ExecutableProgram,
) -> BTreeSet<ExecutableStatementId> {
    let parents = executable_statement_parents(executable);
    executable
        .statements
        .iter()
        .filter(|statement| {
            if statement.declaration.is_none()
                && matches!(statement.kind, ExecutableStatementKind::List { .. })
            {
                return false;
            }
            let Some(parent) = parents.get(&statement.id) else {
                return true;
            };
            executable
                .statements
                .iter()
                .find(|candidate| candidate.id == *parent)
                .is_some_and(|parent| {
                    parent.declaration.is_some()
                        && matches!(parent.kind, ExecutableStatementKind::Field { .. })
                })
        })
        .map(|statement| statement.id)
        .collect()
}

fn verify_erased_scope_index(program: &ErasedProgram) -> Result<(), String> {
    for (index, owner) in program.scope_index.owners.iter().enumerate() {
        if owner.id != StaticOwnerId(index) {
            return Err(format!(
                "erased owner at index {index} has non-dense ID {}",
                owner.id
            ));
        }
        if owner.authority_row != owner.target_row.or(owner.source_row) {
            return Err(format!(
                "erased owner {} has inconsistent authority row",
                owner.id
            ));
        }
    }
    for (index, field) in program.scope_index.fields.iter().enumerate() {
        if field.id != FieldId(index) {
            return Err(format!(
                "erased field at index {index} has non-dense ID {}",
                field.id
            ));
        }
        if let Some(row) = field.row {
            verify_erased_row(program, row, &format!("FieldId {}", field.id))?;
        }
        let structural_row_path =
            erased_field_structural_row_path(&program.scope_index.fields, field)?;
        if field.row.is_none() && !field.row_path.is_empty() {
            return Err(format!(
                "erased FieldId {} without row storage has row-member path `{}`",
                field.id,
                field.row_path.join(".")
            ));
        }
        if field.row_path.iter().any(String::is_empty)
            || (!field.row_path.is_empty() && field.row_path.last() != Some(&field.name))
        {
            return Err(format!(
                "erased FieldId {} has malformed row-member path `{}`",
                field.id,
                field.row_path.join(".")
            ));
        }
        if !structural_row_path.is_empty() && field.row_path != structural_row_path {
            return Err(format!(
                "erased FieldId {} has row-member path `{}`, expected structural path `{}`",
                field.id,
                field.row_path.join("."),
                structural_row_path.join(".")
            ));
        }
        if field.role == ErasedFieldRole::Capture && !field.row_path.is_empty() {
            return Err(format!(
                "capture FieldId {} exposes a row-member path",
                field.id
            ));
        }
        if field.role == ErasedFieldRole::ListAuthority
            && ((field.row.is_none() && field.parent.is_none())
                || field.declaration.is_some()
                || field.producer.is_some())
        {
            return Err(format!(
                "list authority FieldId {} must be hidden authority storage with a row or parent and without a declaration or producer",
                field.id
            ));
        }
        if field.role == ErasedFieldRole::Capture
            && (field.row.is_none()
                || field.declaration.is_some()
                || field.producer.is_some()
                || field.static_owner.is_some())
        {
            return Err(format!(
                "capture FieldId {} must be hidden row storage without a declaration, producer, or semantic owner",
                field.id
            ));
        }
    }
    for local in &program.scope_index.locals {
        if let Some(row) = local.row {
            verify_erased_row(
                program,
                row,
                &format!("owner {} local {}", local.owner, local.local.0),
            )?;
        }
        if program
            .executable
            .expressions
            .get(local.source.as_usize())
            .is_none_or(|expression| expression.id != local.source)
        {
            return Err(format!(
                "owner {} local {} references missing source {}",
                local.owner, local.local.0, local.source
            ));
        }
        let authority_row = program
            .scope_index
            .owners
            .get(local.owner.as_usize())
            .filter(|owner| owner.id == local.owner)
            .and_then(|owner| owner.authority_row);
        let mut capture_identities = BTreeSet::new();
        for capture in &local.captures {
            if !capture_identities.insert((
                capture.source_owner,
                capture.source_local,
                capture.projection.clone(),
            )) {
                return Err(format!(
                    "owner {} local {} has a duplicate detached capture",
                    local.owner, local.local.0
                ));
            }
            if !program.scope_index.locals.iter().any(|source| {
                source.owner == capture.source_owner && source.local == capture.source_local
            }) {
                return Err(format!(
                    "owner {} local {} capture references missing source owner {} local {}",
                    local.owner, local.local.0, capture.source_owner, capture.source_local.0
                ));
            }
            if !program
                .scope_index
                .owner_descends_from(local.owner, capture.source_owner)?
            {
                return Err(format!(
                    "owner {} local {} capture source owner {} is not an ancestor",
                    local.owner, local.local.0, capture.source_owner
                ));
            }
            let field = program
                .scope_index
                .fields
                .get(capture.field.as_usize())
                .filter(|field| field.id == capture.field)
                .ok_or_else(|| {
                    format!(
                        "owner {} local {} capture references missing FieldId {}",
                        local.owner, local.local.0, capture.field
                    )
                })?;
            if field.role != ErasedFieldRole::Capture || field.row != authority_row {
                return Err(format!(
                    "owner {} local {} capture FieldId {} is not hidden storage on its authority row",
                    local.owner, local.local.0, capture.field
                ));
            }
        }
        let list_name = local
            .row
            .and_then(|row| program.lists.get(row.list.as_usize()))
            .filter(|list| local.row.is_some_and(|row| list.id == row.list))
            .map(|list| list.name.as_str());
        let relative_path = |path: &str| {
            list_name
                .and_then(|list| path.strip_prefix(list))
                .and_then(|suffix| suffix.strip_prefix('.'))
                .map(|suffix| suffix.split('.').map(str::to_owned).collect::<Vec<_>>())
        };
        let mut paths = BTreeSet::new();
        for member in &local.members {
            if member.path.is_empty()
                || member.path.iter().any(String::is_empty)
                || !paths.insert(member.path.clone())
            {
                return Err(format!(
                    "owner {} local {} contains an empty or duplicate member path `{}`",
                    local.owner,
                    local.local.0,
                    member.path.join(".")
                ));
            }
            match &member.target {
                ErasedLocalMemberTarget::Field(field) => {
                    let field = program
                        .scope_index
                        .fields
                        .get(field.as_usize())
                        .filter(|candidate| candidate.id == *field)
                        .ok_or_else(|| {
                            format!(
                                "owner {} local {} member `{}` references missing FieldId {field}",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            )
                        })?;
                    let forwarded_path = match member.forwarded_from.as_ref() {
                        Some(ErasedLocalMemberForwarding::Row { row, path })
                            if Some(*row) == local.row =>
                        {
                            path
                        }
                        None => {
                            return Err(format!(
                                "owner {} local {} scalar member `{}` has no exact row forwarding",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            ));
                        }
                        Some(ErasedLocalMemberForwarding::Row { .. }) => {
                            return Err(format!(
                                "owner {} local {} scalar member `{}` has inconsistent row forwarding",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            ));
                        }
                        Some(ErasedLocalMemberForwarding::Local { .. }) => {
                            return Err(format!(
                                "owner {} local {} scalar member `{}` forwards from another local",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            ));
                        }
                    };
                    if field.row != local.row
                        || *forwarded_path != field.row_path
                        || forwarded_path.last() != Some(&field.name)
                    {
                        return Err(format!(
                            "owner {} local {} member `{}` forwarded as `{}` is inconsistent with FieldId {}",
                            local.owner,
                            local.local.0,
                            member.path.join("."),
                            forwarded_path.join("."),
                            field.id
                        ));
                    }
                }
                ErasedLocalMemberTarget::Sources(sources) => {
                    if sources.is_empty() || sources.windows(2).any(|pair| pair[0] >= pair[1]) {
                        return Err(format!(
                            "owner {} local {} member `{}` has an empty or noncanonical source set",
                            local.owner,
                            local.local.0,
                            member.path.join(".")
                        ));
                    }
                    if let Some(ErasedLocalMemberForwarding::Local {
                        owner,
                        local: upstream_local,
                        path,
                    }) = member.forwarded_from.as_ref()
                    {
                        let upstream = program
                            .scope_index
                            .locals
                            .iter()
                            .find(|candidate| {
                                candidate.owner == *owner
                                    && candidate.local == *upstream_local
                            })
                            .ok_or_else(|| {
                                format!(
                                    "owner {} local {} member `{}` forwards from missing local {}:{}",
                                    local.owner,
                                    local.local.0,
                                    member.path.join("."),
                                    owner,
                                    upstream_local.0
                                )
                            })?;
                        let upstream_members = upstream
                            .members
                            .iter()
                            .filter(|candidate| {
                                candidate.path == *path && candidate.target == member.target
                            })
                            .count();
                        if upstream_members != 1 {
                            return Err(format!(
                                "owner {} local {} member `{}` forwards its source set from {} exact upstream members",
                                local.owner,
                                local.local.0,
                                member.path.join("."),
                                upstream_members
                            ));
                        }
                    }
                    for source_id in sources {
                        let source = program
                            .sources
                            .get(source_id.as_usize())
                            .filter(|candidate| candidate.id == *source_id)
                            .ok_or_else(|| {
                                format!(
                                    "owner {} local {} member `{}` references missing SourceId {source_id}",
                                    local.owner, local.local.0, member.path.join(".")
                                )
                            })?;
                        match member.forwarded_from.as_ref() {
                            Some(ErasedLocalMemberForwarding::Local { .. }) => {}
                            Some(ErasedLocalMemberForwarding::Row { row, path }) => {
                                let list = program
                                    .lists
                                    .get(row.list.as_usize())
                                    .filter(|list| {
                                        list.id == row.list
                                            && list.row_scope_id == Some(row.scope)
                                    })
                                    .ok_or_else(|| {
                                        format!(
                                            "owner {} local {} member `{}` forwards from missing row {row:?}",
                                            local.owner,
                                            local.local.0,
                                            member.path.join(".")
                                        )
                                    })?;
                                let relative = source
                                    .path
                                    .strip_prefix(&list.name)
                                    .and_then(|suffix| suffix.strip_prefix('.'))
                                    .map(|suffix| {
                                        suffix.split('.').map(str::to_owned).collect::<Vec<_>>()
                                    });
                                let target_projection = local.row.is_some_and(|target_row| {
                                    program.scope_index.row_source_projections.iter().any(
                                        |projection| {
                                            projection.row == target_row
                                                && projection.path == member.path
                                                && projection.source == source.id
                                        },
                                    )
                                });
                                if local.row == Some(*row)
                                    || path != &member.path
                                    || source.scope_id != Some(row.scope)
                                    || relative.as_ref() != Some(path)
                                    || !target_projection
                                {
                                    return Err(format!(
                                        "owner {} local {} member `{}` has invalid row forwarding from {row:?} for source `{}`",
                                        local.owner,
                                        local.local.0,
                                        member.path.join("."),
                                        source.path
                                    ));
                                }
                            }
                            None if source.scope_id != local.row.map(|row| row.scope)
                                || relative_path(&source.path)
                                    .is_none_or(|path| path != member.path) =>
                            {
                                return Err(format!(
                                    "owner {} local {} member `{}` is inconsistent with source `{}`",
                                    local.owner,
                                    local.local.0,
                                    member.path.join("."),
                                    source.path
                                ));
                            }
                            None => {}
                        }
                    }
                }
                ErasedLocalMemberTarget::State(state) => {
                    if member.forwarded_from.is_some() {
                        return Err(format!(
                            "owner {} local {} state member `{}` has unsupported forwarding metadata",
                            local.owner,
                            local.local.0,
                            member.path.join(".")
                        ));
                    }
                    let state = program
                        .state_cells
                        .get(state.as_usize())
                        .filter(|candidate| candidate.id == *state)
                        .ok_or_else(|| {
                            format!(
                                "owner {} local {} member `{}` references missing StateId {state}",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            )
                        })?;
                    if state.scope_id != local.row.map(|row| row.scope)
                        || relative_path(&state.path).is_none_or(|path| path != member.path)
                    {
                        return Err(format!(
                            "owner {} local {} member `{}` is inconsistent with state `{}`",
                            local.owner,
                            local.local.0,
                            member.path.join("."),
                            state.path
                        ));
                    }
                }
            }
        }
    }
    let mut local_forwarding = BTreeMap::<
        (StaticOwnerId, MaterializationLocalId, Vec<String>, SourceId),
        (StaticOwnerId, MaterializationLocalId, Vec<String>, SourceId),
    >::new();
    for local in &program.scope_index.locals {
        for member in &local.members {
            let ErasedLocalMemberTarget::Sources(sources) = &member.target else {
                continue;
            };
            let Some(ErasedLocalMemberForwarding::Local {
                owner,
                local: upstream_local,
                path,
            }) = member.forwarded_from.as_ref()
            else {
                continue;
            };
            for source in sources {
                local_forwarding.insert(
                    (local.owner, local.local, member.path.clone(), *source),
                    (*owner, *upstream_local, path.clone(), *source),
                );
            }
        }
    }
    for start in local_forwarding.keys() {
        let mut current = start;
        let mut visited = BTreeSet::new();
        while let Some(next) = local_forwarding.get(current) {
            if !visited.insert(current.clone()) {
                return Err(format!(
                    "resource identity forwarding forms a local cycle at owner {} local {} member `{}` source {}",
                    current.0,
                    current.1.0,
                    current.2.join("."),
                    current.3
                ));
            }
            current = next;
        }
    }
    for (index, binding) in program.scope_index.bindings.iter().enumerate() {
        if binding.id != ErasedBindingId(index) {
            return Err(format!(
                "storage binding at index {index} has non-dense ID {}",
                binding.id
            ));
        }
        if binding.owner_ancestry.last().copied() != binding.static_owner {
            return Err(format!(
                "storage binding {} has owner {:?} but ancestry {:?}",
                binding.id, binding.static_owner, binding.owner_ancestry
            ));
        }
        for (depth, owner) in binding.owner_ancestry.iter().copied().enumerate() {
            let definition = program
                .scope_index
                .owners
                .get(owner.as_usize())
                .filter(|definition| definition.id == owner)
                .ok_or_else(|| {
                    format!(
                        "storage binding {} references missing owner {owner}",
                        binding.id
                    )
                })?;
            let expected_parent = depth
                .checked_sub(1)
                .map(|parent| binding.owner_ancestry[parent]);
            if definition.parent != expected_parent {
                return Err(format!(
                    "storage binding {} owner ancestry is not structural at {owner}",
                    binding.id
                ));
            }
        }
        let expression = program
            .executable
            .expressions
            .get(binding.producer.as_usize())
            .filter(|expression| expression.id == binding.producer)
            .ok_or_else(|| {
                format!(
                    "storage binding {} references missing producer {}",
                    binding.id, binding.producer
                )
            })?;
        if expression.flow_type != binding.flow_type {
            return Err(format!(
                "storage binding {} declaration {} (`{}`) type {:?} differs from producer {} checked {} owner {:?} kind {:?} type {:?}",
                binding.id,
                binding.declaration.0,
                binding.diagnostic_path,
                binding.flow_type,
                binding.producer,
                expression.checked_expr_id.0,
                expression.owner,
                expression.kind,
                expression.flow_type,
            ));
        }
        match binding.target {
            ErasedBindingTarget::Value { field, row } => {
                if let Some(field) = field {
                    let field = program
                        .scope_index
                        .fields
                        .iter()
                        .find(|value| value.id == field)
                        .ok_or_else(|| {
                            format!(
                                "erased binding {} references missing FieldId {field}",
                                binding.id
                            )
                        })?;
                    if field.resource_only {
                        return Err(format!(
                            "erased value binding {} targets resource-only FieldId {}",
                            binding.id, field.id
                        ));
                    }
                }
                if let Some(row) = row {
                    verify_erased_row(program, row, &format!("binding {}", binding.id))?;
                }
            }
            ErasedBindingTarget::Source {
                executable,
                runtime,
            } => {
                let executable_source = program
                    .executable
                    .sources
                    .iter()
                    .find(|source| source.id == executable);
                let declaration_matches = executable_source.is_some_and(|source| {
                    source.declaration == binding.declaration
                        || matches!(
                            &expression.kind,
                            ExecutableExpressionKind::CanonicalRead {
                                target,
                                projection,
                                ..
                            } if *target == source.declaration && projection.is_empty()
                        )
                });
                if !declaration_matches
                    || !program.sources.iter().any(|source| {
                        source.id == runtime && source.executable_source_id == Some(executable)
                    })
                {
                    return Err(format!(
                        "storage binding {} has an invalid source allocation",
                        binding.id
                    ));
                }
            }
            ErasedBindingTarget::State {
                executable,
                runtime,
                ..
            } => {
                if !program
                    .executable
                    .states
                    .iter()
                    .any(|state| state.id == executable && state.declaration == binding.declaration)
                    || !program.state_cells.iter().any(|state| {
                        state.id == runtime && state.executable_state_id == Some(executable)
                    })
                {
                    return Err(format!(
                        "storage binding {} has an invalid state allocation",
                        binding.id
                    ));
                }
            }
        }
    }
    for (index, source) in program.scope_index.sources.iter().enumerate() {
        let expected_id = SourceId(index);
        if source.source != expected_id {
            return Err(format!(
                "erased source at index {index} has non-dense SourceId {}",
                source.source
            ));
        }
        let runtime = program
            .sources
            .get(index)
            .filter(|candidate| candidate.id == source.source)
            .ok_or_else(|| format!("erased source {} has no runtime source", source.source))?;
        if source.static_owner != runtime.static_owner
            || source.owner_ancestry.last().copied() != source.static_owner
        {
            return Err(format!(
                "erased source {} has inconsistent structural ownership",
                source.source
            ));
        }
        for (depth, owner) in source.owner_ancestry.iter().copied().enumerate() {
            let definition = program
                .scope_index
                .owners
                .get(owner.as_usize())
                .filter(|definition| definition.id == owner)
                .ok_or_else(|| {
                    format!(
                        "erased source {} references missing owner {owner}",
                        source.source
                    )
                })?;
            let expected_parent = depth
                .checked_sub(1)
                .map(|parent| source.owner_ancestry[parent]);
            if definition.parent != expected_parent {
                return Err(format!(
                    "erased source {} owner ancestry is not structural at {owner}",
                    source.source
                ));
            }
        }
        match source.origin {
            ErasedSourceOrigin::Executable {
                executable,
                binding,
            } => {
                let valid_binding = program
                    .scope_index
                    .bindings
                    .get(binding.as_usize())
                    .is_some_and(|candidate| {
                        candidate.id == binding
                            && matches!(
                                candidate.target,
                                ErasedBindingTarget::Source {
                                    executable: candidate_executable,
                                    runtime: candidate_runtime,
                                } if candidate_executable == executable
                                    && candidate_runtime == source.source
                            )
                    });
                if runtime.executable_source_id != Some(executable) || !valid_binding {
                    return Err(format!(
                        "erased source {} has an invalid executable origin",
                        source.source
                    ));
                }
            }
            ErasedSourceOrigin::DistributedImport => {
                if runtime.executable_source_id.is_some()
                    || !runtime.path.starts_with("@distributed/")
                    || source.static_owner.is_some()
                {
                    return Err(format!(
                        "erased source {} has an invalid distributed ingress origin",
                        source.source
                    ));
                }
            }
        }
    }
    let direct_storage_statements = direct_erased_storage_statements(&program.executable);
    for statement in &program.executable.statements {
        if !direct_storage_statements.contains(&statement.id) {
            continue;
        }
        let Some(declaration) = statement.declaration else {
            continue;
        };
        if direct_list_alias_target(&program.executable, statement).is_some() {
            continue;
        }
        let Some(flow_type) = &statement.flow_type else {
            return Err(format!(
                "executable declaration {} statement {} has no final checked type",
                declaration.0, statement.id
            ));
        };
        if flow_type.mode != boon_checked::FlowMode::Continuous
            || !matches!(
                &flow_type.ty,
                boon_checked::Type::List(item)
                    if matches!(item.as_ref(), boon_checked::Type::Object(_))
            )
        {
            continue;
        }
        let matches = program
            .scope_index
            .bindings
            .iter()
            .filter(|binding| {
                binding.declaration == declaration
                    && matches!(
                        binding.target,
                        ErasedBindingTarget::Value { row: Some(_), .. }
                    )
            })
            .count();
        if matches != 1 {
            let candidate_bindings = program
                .scope_index
                .bindings
                .iter()
                .filter(|binding| binding.declaration == declaration)
                .map(|binding| {
                    format!(
                        "{} producer {} path `{}` target {:?}",
                        binding.id, binding.producer, binding.diagnostic_path, binding.target
                    )
                })
                .collect::<Vec<_>>();
            return Err(format!(
                "typed list declaration {} at statement {} ({:?}, value {:?}) must have one \
                 ListId storage binding, found {matches}; declaration bindings: \
                 {candidate_bindings:?}",
                declaration.0, statement.id, statement.kind, statement.value
            ));
        }
    }
    let mut read_expressions = BTreeSet::new();
    for (index, read) in program.scope_index.reads.iter().enumerate() {
        if read.id != ErasedReadId(index) {
            return Err(format!(
                "erased read at index {index} has non-dense ID {}",
                read.id
            ));
        }
        let expression = program
            .executable
            .expressions
            .get(read.expression.as_usize())
            .filter(|expression| expression.id == read.expression)
            .ok_or_else(|| {
                format!(
                    "erased read {} references missing expression {}",
                    read.id, read.expression
                )
            })?;
        if !matches!(
            expression.kind,
            ExecutableExpressionKind::CanonicalRead { .. }
                | ExecutableExpressionKind::LocalRead { .. }
                | ExecutableExpressionKind::ExternalRead { .. }
                | ExecutableExpressionKind::Drain { .. }
                | ExecutableExpressionKind::MaterializationLocal { .. }
                | ExecutableExpressionKind::FunctionParameter { .. }
                | ExecutableExpressionKind::ElementState { .. }
        ) {
            return Err(format!(
                "erased read {} targets non-read expression {}",
                read.id, read.expression
            ));
        }
        if !read_expressions.insert(read.expression) {
            return Err(format!(
                "executable expression {} has multiple erased read targets",
                read.expression
            ));
        }
        match &read.target {
            ErasedReadTarget::Binding {
                binding,
                projection,
            } => {
                let binding = program
                    .scope_index
                    .bindings
                    .get(binding.as_usize())
                    .filter(|candidate| candidate.id == *binding)
                    .ok_or_else(|| {
                        format!("erased read {} references missing {binding}", read.id)
                    })?;
                if !projection.is_empty()
                    && matches!(
                        binding.target,
                        ErasedBindingTarget::Source { .. } | ErasedBindingTarget::State { .. }
                    )
                {
                    return Err(format!(
                        "erased read {} leaves a projection on a source/state binding",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::SourcePayload {
                binding,
                source,
                field,
                ..
            } => {
                if !program.scope_index.bindings.iter().any(|candidate| {
                    candidate.id == *binding
                        && matches!(
                            candidate.target,
                            ErasedBindingTarget::Source { runtime, .. } if runtime == *source
                        )
                }) {
                    return Err(format!(
                        "erased read {} has mismatched source binding {binding}",
                        read.id
                    ));
                }
                let source = program
                    .sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == *source)
                    .ok_or_else(|| {
                        format!(
                            "erased read {} references missing SourceId {source}",
                            read.id
                        )
                    })?;
                if !source.payload_schema.fields.contains(field) {
                    return Err(format!(
                        "erased read {} references absent payload field {field:?} on `{}`",
                        read.id, source.path
                    ));
                }
            }
            ErasedReadTarget::StateProjection {
                binding,
                state,
                fields,
            } => {
                if fields.is_empty() {
                    return Err(format!(
                        "erased read {} has an empty state projection",
                        read.id
                    ));
                }
                if !program
                    .state_cells
                    .iter()
                    .any(|candidate| candidate.id == *state)
                {
                    return Err(format!(
                        "erased read {} references missing StateId {state}",
                        read.id
                    ));
                }
                if !program.scope_index.bindings.iter().any(|candidate| {
                    candidate.id == *binding
                        && matches!(
                            candidate.target,
                            ErasedBindingTarget::State { runtime, .. } if runtime == *state
                        )
                }) {
                    return Err(format!(
                        "erased read {} has mismatched state binding {binding}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::Expression {
                expression: target, ..
            } => {
                if *target == read.expression {
                    return Err(format!(
                        "erased read {} recursively targets itself",
                        read.id
                    ));
                }
                if !program
                    .executable
                    .expressions
                    .iter()
                    .any(|candidate| candidate.id == *target)
                {
                    return Err(format!(
                        "erased read {} references missing expression {target}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::Local { value, .. } => {
                if !program
                    .executable
                    .expressions
                    .iter()
                    .any(|candidate| candidate.id == *value)
                {
                    return Err(format!(
                        "erased read {} references missing local expression {value}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::ExternalValue { reference } => {
                if program
                    .role_references()
                    .value_references
                    .get(*reference)
                    .is_none()
                {
                    return Err(format!(
                        "erased read {} references missing external value {reference}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::ElementState {
                context,
                projection,
            } => {
                if !matches!(
                    &expression.kind,
                    ExecutableExpressionKind::ElementState {
                        context: expression_context,
                        projection: expression_projection,
                    } if expression_context == context && expression_projection == projection
                ) {
                    return Err(format!(
                        "erased read {} element-state target differs from expression {}",
                        read.id, read.expression
                    ));
                }
            }
            ErasedReadTarget::MaterializationLocal { owner, local, .. } => {
                if !program
                    .scope_index
                    .locals
                    .iter()
                    .any(|candidate| candidate.owner == *owner && candidate.local == *local)
                {
                    return Err(format!(
                        "erased read {} references missing owner {owner} local {}",
                        read.id, local.0
                    ));
                }
            }
            ErasedReadTarget::FunctionParameter { parameter, .. } => {
                let producer_parameter = program.executable.functions.iter().any(|function| {
                    function.id == parameter.function
                        && function
                            .parameters
                            .iter()
                            .any(|candidate| candidate.id == *parameter)
                });
                let ordinary_parameter =
                    program
                        .executable
                        .ordinary_functions
                        .iter()
                        .any(|function| {
                            function.id == parameter.function
                                && function
                                    .parameters
                                    .iter()
                                    .any(|candidate| candidate.id == *parameter)
                        });
                if !producer_parameter && !ordinary_parameter {
                    return Err(format!(
                        "erased read {} references missing function parameter {:?}",
                        read.id, parameter
                    ));
                }
            }
        }
    }
    let mut row_value_keys = BTreeSet::new();
    for row_value in &program.scope_index.row_values {
        if program
            .executable
            .expressions
            .get(row_value.expression.as_usize())
            .is_none_or(|expression| expression.id != row_value.expression)
        {
            return Err(format!(
                "erased row value references missing expression {}",
                row_value.expression
            ));
        }
        if row_value.projection.iter().any(String::is_empty)
            || !row_value_keys.insert((
                row_value.expression,
                row_value.projection.clone(),
                row_value.row,
            ))
        {
            return Err(format!(
                "erased row value expression {} has an empty projection or duplicate row identity `{}` at {:?}",
                row_value.expression,
                row_value.projection.join("."),
                row_value.row,
            ));
        }
        verify_erased_row(
            program,
            row_value.row,
            &format!(
                "row value {} projection `{}`",
                row_value.expression,
                row_value.projection.join(".")
            ),
        )?;
    }
    for dependency in &program.scope_index.dependencies {
        let _dependent = program
            .scope_index
            .bindings
            .get(dependency.dependent.as_usize())
            .filter(|binding| binding.id == dependency.dependent)
            .ok_or_else(|| {
                format!(
                    "erased dependency references missing binding {}",
                    dependency.dependent
                )
            })?;
        let expression = program
            .executable
            .expressions
            .get(dependency.expression.as_usize())
            .filter(|expression| expression.id == dependency.expression)
            .ok_or_else(|| {
                format!(
                    "erased dependency references missing expression {}",
                    dependency.expression
                )
            })?;
        match dependency.target {
            ErasedDependencyTarget::ExternalRead { read: read_id } => {
                let read = program
                    .scope_index
                    .reads
                    .get(read_id.as_usize())
                    .filter(|read| read.id == read_id && read.expression == dependency.expression)
                    .ok_or_else(|| {
                        format!(
                            "erased dependency expression {} references a missing read",
                            dependency.expression
                        )
                    })?;
                if !matches!(read.target, ErasedReadTarget::ExternalValue { .. }) {
                    return Err(format!(
                        "erased dependency expression {} references a non-external read",
                        dependency.expression
                    ));
                }
            }
            ErasedDependencyTarget::ExternalCall { reference } => {
                let call = program
                    .role_references()
                    .calls
                    .get(reference)
                    .ok_or_else(|| {
                        format!(
                            "erased dependency expression {} references missing external call {reference}",
                            dependency.expression
                        )
                    })?;
                if !matches!(
                    &expression.kind,
                    ExecutableExpressionKind::Call {
                        callable_kind: ExecutableCallableKind::External,
                        name,
                        ..
                    } if name == &call.canonical_function
                ) {
                    return Err(format!(
                        "erased dependency expression {} does not match external call {reference}",
                        dependency.expression
                    ));
                }
            }
        }
        if let ErasedDependencyTiming::After { boundaries } = &dependency.timing {
            if boundaries.is_empty() {
                return Err(format!(
                    "erased dependency expression {} has an empty temporal boundary set",
                    dependency.expression
                ));
            }
            let unique = boundaries.iter().copied().collect::<BTreeSet<_>>();
            if unique.len() != boundaries.len() {
                return Err(format!(
                    "erased dependency expression {} repeats a temporal boundary",
                    dependency.expression
                ));
            }
            for boundary in boundaries {
                let exists = match boundary {
                    ErasedTemporalBoundary::Source(source) => program
                        .sources
                        .get(source.as_usize())
                        .is_some_and(|candidate| candidate.id == *source),
                    ErasedTemporalBoundary::State(state) => program
                        .state_cells
                        .get(state.as_usize())
                        .is_some_and(|candidate| candidate.id == *state),
                    ErasedTemporalBoundary::Pulse(pulse) => program
                        .pulse_batches
                        .get(pulse.as_usize())
                        .is_some_and(|candidate| candidate.id == *pulse),
                };
                if !exists {
                    return Err(format!(
                        "erased dependency expression {} references missing temporal boundary {boundary:?}",
                        dependency.expression
                    ));
                }
            }
        }
    }
    Ok(())
}

fn erased_field_structural_row_path(
    fields: &[ErasedFieldDef],
    field: &ErasedFieldDef,
) -> Result<Vec<String>, String> {
    let Some(row) = field.row else {
        return Ok(Vec::new());
    };
    let Some(parent) = field.parent else {
        return Ok(Vec::new());
    };
    let mut parent = fields
        .get(parent.as_usize())
        .filter(|candidate| candidate.id == parent)
        .ok_or_else(|| {
            format!(
                "erased FieldId {} references missing parent FieldId {parent}",
                field.id
            )
        })?;
    if parent.row != Some(row) {
        return Ok(Vec::new());
    }

    let mut reversed = vec![field.name.clone()];
    let mut remaining = fields.len().saturating_add(1);
    loop {
        if remaining == 0 {
            return Err(format!(
                "erased FieldId {} has cyclic structural row ancestry",
                field.id
            ));
        }
        remaining -= 1;
        let Some(grandparent_id) = parent.parent else {
            break;
        };
        let grandparent = fields
            .get(grandparent_id.as_usize())
            .filter(|candidate| candidate.id == grandparent_id)
            .ok_or_else(|| {
                format!(
                    "erased FieldId {} ancestry references missing FieldId {grandparent_id}",
                    field.id
                )
            })?;
        if grandparent.row != Some(row) {
            break;
        }
        reversed.push(parent.name.clone());
        parent = grandparent;
    }
    reversed.reverse();
    Ok(reversed)
}

fn verify_erased_row(
    program: &ErasedProgram,
    row: ErasedRowBinding,
    context: &str,
) -> Result<(), String> {
    let list = program
        .lists
        .get(row.list.as_usize())
        .filter(|list| list.id == row.list)
        .ok_or_else(|| format!("{context} references missing ListId {}", row.list))?;
    if list.row_scope_id != Some(row.scope) {
        return Err(format!(
            "{context} row scope {} differs from ListId {} scope {:?}",
            row.scope, row.list, list.row_scope_id
        ));
    }
    Ok(())
}

fn distributed_function_role(function: &str) -> Option<boon_checked::ProgramRole> {
    function
        .split_once('/')
        .and_then(|(namespace, _)| distributed_role(namespace))
}

fn distributed_role(namespace: &str) -> Option<boon_checked::ProgramRole> {
    Some(match boon_syntax::program_role_root(namespace)? {
        boon_syntax::ProgramRoleRoot::Client => boon_checked::ProgramRole::Client,
        boon_syntax::ProgramRoleRoot::Session => boon_checked::ProgramRole::Session,
        boon_syntax::ProgramRoleRoot::Server => boon_checked::ProgramRole::Server,
    })
}

fn ensure_distributed_value_flow_is_closed(
    flow_type: &boon_checked::FlowType,
    context: &str,
) -> Result<(), String> {
    if flow_type.mode == boon_checked::FlowMode::Absent {
        return Err(format!("{context} is always absent"));
    }
    ensure_distributed_type_is_closed(&flow_type.ty, context)
}

fn ensure_distributed_type_is_closed(
    data_type: &boon_checked::Type,
    context: &str,
) -> Result<(), String> {
    if distributed_type_is_closed(data_type) {
        Ok(())
    } else {
        Err(format!(
            "{context} does not have a closed value type: {data_type:?}"
        ))
    }
}

fn distributed_type_is_closed(data_type: &boon_checked::Type) -> bool {
    match data_type {
        boon_checked::Type::Text
        | boon_checked::Type::Number
        | boon_checked::Type::Bytes(_)
        | boon_checked::Type::Bits { .. } => true,
        boon_checked::Type::Object(shape) => {
            !shape.open && shape.fields.values().all(distributed_type_is_closed)
        }
        boon_checked::Type::List(item) => distributed_type_is_closed(item),
        boon_checked::Type::Map { key, value } => {
            distributed_type_is_closed(key) && distributed_type_is_closed(value)
        }
        boon_checked::Type::Set(item) => distributed_type_is_closed(item),
        boon_checked::Type::Union(members) => {
            !members.is_empty() && members.iter().all(distributed_type_is_closed)
        }
        boon_checked::Type::VariantSet(variants) => variants.iter().all(|variant| match variant {
            boon_checked::Variant::Tag(_) => true,
            boon_checked::Variant::Tagged { fields, .. } => {
                !fields.open && fields.fields.values().all(distributed_type_is_closed)
            }
        }),
        boon_checked::Type::Absent
        | boon_checked::Type::RenderContract
        | boon_checked::Type::Function { .. }
        | boon_checked::Type::UnresolvedShape { .. }
        | boon_checked::Type::Var(_)
        | boon_checked::Type::Unknown => false,
    }
}

#[cfg(test)]
fn lower_elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub fn verify_hidden_identity(program: &ErasedProgram) -> Result<(), String> {
    if program.lists.iter().any(|list| !list.has_generation) {
        return Err("all list memories must carry generation guards".to_owned());
    }
    verify_identity_clean_identifiers(program)?;
    Ok(())
}

fn verify_list_initializer_inputs(
    program: &ErasedProgram,
    list: &ListMemory,
) -> Result<(), String> {
    let row = list.row_scope_id.map(|scope| ErasedRowBinding {
        list: list.id,
        scope,
    });
    let mut previous_name: Option<&str> = None;
    let mut inputs = BTreeMap::new();
    for input in &list.initializer_inputs {
        if input.name.is_empty()
            || previous_name.is_some_and(|previous| previous >= input.name.as_str())
        {
            return Err(format!(
                "list `{}` initializer inputs are empty, duplicated, or not canonically ordered at `{}`",
                list.name, input.name
            ));
        }
        previous_name = Some(&input.name);
        let field = program
            .scope_index
            .fields
            .get(input.field.as_usize())
            .filter(|field| field.id == input.field)
            .ok_or_else(|| {
                format!(
                    "list `{}` initializer input `{}` references missing FieldId {}",
                    list.name, input.name, input.field
                )
            })?;
        if field.row != row || row.is_none() {
            return Err(format!(
                "list `{}` initializer input `{}` FieldId {} does not belong to its exact row",
                list.name, input.name, input.field
            ));
        }
        inputs.insert(input.name.as_str(), input.field);
    }

    let ListInitializer::RecordLiteral { rows } = &list.initializer else {
        return Ok(());
    };
    for initial_row in rows {
        for field in &initial_row.fields {
            if field.value == InitialValue::ResourceOnly {
                continue;
            }
            if !inputs.contains_key(field.name.as_str()) {
                return Err(format!(
                    "list `{}` initial field `{}` has no verified initializer FieldId",
                    list.name, field.name
                ));
            }
            if let InitialValue::RowInitialField { path } = &field.value {
                let source = path.split('.').next().filter(|name| !name.is_empty());
                if source.is_none_or(|source| !inputs.contains_key(source)) {
                    return Err(format!(
                        "list `{}` initial field `{}` references row initializer `{path}` without an exact FieldId",
                        list.name, field.name
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn verify_static_schedule(program: &ErasedProgram) -> Result<(), String> {
    if program.graph_node_count != program.executable.expressions.len() {
        return Err(format!(
            "graph_node_count {} does not match {} canonical executable expressions",
            program.graph_node_count,
            program.executable.expressions.len()
        ));
    }
    if let Some(batch) = program
        .pulse_batches
        .iter()
        .find(|batch| matches!(batch.fusion, PulseFusionEligibility::PendingVerification))
    {
        return Err(format!(
            "pulse batch {} reached executable validation before verification",
            batch.id
        ));
    }
    verify_distributed_reference_schedule(program)?;
    verify_executable_schedule(program)?;

    let mut source_paths = BTreeSet::new();
    let mut sources_by_path = BTreeMap::<&str, Vec<&SourcePort>>::new();
    for source in &program.sources {
        if source.path.trim().is_empty() {
            return Err("source port has empty path".to_owned());
        }
        source_paths.insert(source.path.as_str());
        sources_by_path
            .entry(source.path.as_str())
            .or_default()
            .push(source);
    }
    for (path, sources) in sources_by_path {
        let Some(first) = sources.first() else {
            continue;
        };
        if sources.len() > 1 {
            let projection_set = |source: SourceId| {
                program
                    .scope_index
                    .row_source_projections
                    .iter()
                    .filter(|projection| projection.source == source)
                    .map(|projection| (projection.row, projection.path.clone()))
                    .collect::<BTreeSet<_>>()
            };
            let expected = projection_set(first.id);
            if !first.scoped
                || first.scope_id.is_none()
                || expected.is_empty()
                || sources.iter().any(|source| {
                    !source.scoped
                        || source.scope_id != first.scope_id
                        || source.payload_schema != first.payload_schema
                        || projection_set(source.id) != expected
                })
            {
                return Err(format!(
                    "duplicate source port `{path}` is not one canonical scoped alternative set"
                ));
            }
        }
    }
    for (index, source) in program.sources.iter().enumerate() {
        if source.id.as_usize() != index {
            return Err(format!(
                "source port `{}` has SourceId {}, expected {index}",
                source.path, source.id
            ));
        }
        if source.scoped && source.scope_id.is_none() {
            return Err(format!(
                "scoped source port `{}` has no typed ScopeId",
                source.path
            ));
        }
    }
    let state_paths = unique_strings(
        "state cell",
        program.state_cells.iter().map(|cell| cell.path.as_str()),
    )?;
    for (index, cell) in program.state_cells.iter().enumerate() {
        if cell.id.as_usize() != index {
            return Err(format!(
                "state cell `{}` has StateId {}, expected {index}",
                cell.path, cell.id
            ));
        }
    }
    let list_names = unique_strings("list", program.lists.iter().map(|list| list.name.as_str()))?;
    let mut row_scope_ids = BTreeSet::new();
    for (index, list) in program.lists.iter().enumerate() {
        if list.id.as_usize() != index {
            return Err(format!(
                "list memory `{}` has ListId {}, expected {index}",
                list.name, list.id
            ));
        }
        if let Some(scope_id) = list.row_scope_id
            && !row_scope_ids.insert(scope_id)
        {
            return Err(format!(
                "list memory `{}` reuses row ScopeId {}",
                list.name, scope_id
            ));
        }
        verify_list_initializer_inputs(program, list)?;
    }
    for (index, scope_id) in row_scope_ids.iter().copied().enumerate() {
        if scope_id.as_usize() != index {
            return Err(format!(
                "row ScopeId {} is not canonical at index {index}",
                scope_id
            ));
        }
    }
    let derived_paths = unique_strings(
        "derived value",
        program
            .derived_values
            .iter()
            .map(|value| value.path.as_str()),
    )?;
    let mut derived_field_ids = BTreeSet::new();
    for value in &program.derived_values {
        if !derived_field_ids.insert(value.id) {
            return Err(format!(
                "derived value `{}` reuses FieldId {}",
                value.path, value.id
            ));
        }
        let field = program
            .scope_index
            .fields
            .iter()
            .find(|field| field.id == value.id)
            .ok_or_else(|| {
                format!(
                    "derived value `{}` references missing FieldId {}",
                    value.path, value.id
                )
            })?;
        if field.resource_only {
            return Err(format!(
                "derived value `{}` targets resource-only FieldId {}",
                value.path, value.id
            ));
        }
        if value.kind == DerivedValueKind::ListView {
            let list_id = value.materialized_list_id.ok_or_else(|| {
                format!(
                    "typed list view `{}` has no materialized ListId",
                    value.path
                )
            })?;
            let row_scope_id = value.materialized_row_scope_id.ok_or_else(|| {
                format!(
                    "typed list view `{}` has no materialized row ScopeId",
                    value.path
                )
            })?;
            let list = program.lists.get(list_id.as_usize()).ok_or_else(|| {
                format!(
                    "typed list view `{}` references missing materialized ListId {}",
                    value.path, list_id
                )
            })?;
            if list.id != list_id
                || list.name != value.path
                || list.row_scope_id != Some(row_scope_id)
            {
                return Err(format!(
                    "typed list view `{}` storage metadata does not match ListId {} and ScopeId {}",
                    value.path, list_id, row_scope_id
                ));
            }
        }
    }
    for target in typed_derived_list_targets(&program.executable)? {
        let binding = program
            .scope_index
            .bindings
            .iter()
            .find(|binding| {
                binding.declaration == target.declaration && binding.producer == target.producer
            })
            .ok_or_else(|| {
                format!(
                    "typed list declaration `{}` has no exact storage binding",
                    target.path
                )
            })?;
        let ErasedBindingTarget::Value {
            row:
                Some(ErasedRowBinding {
                    list,
                    scope: row_scope,
                }),
            ..
        } = binding.target
        else {
            return Err(format!(
                "typed list declaration `{}` did not receive keyed ListId/ScopeId storage",
                target.path
            ));
        };
        let statement = program
            .executable
            .statements
            .iter()
            .find(|statement| statement.id == target.statement)
            .ok_or_else(|| {
                format!(
                    "typed list declaration `{}` references missing statement {}",
                    target.path, target.statement
                )
            })?;
        if matches!(statement.kind, ExecutableStatementKind::Field { .. }) {
            let value = program
                .derived_values
                .iter()
                .find(|value| value.executable_statement_id == target.statement);
            let Some(value) = value else {
                let producer = program
                    .executable
                    .expressions
                    .get(target.producer.as_usize())
                    .filter(|expression| expression.id == target.producer)
                    .ok_or_else(|| {
                        format!(
                            "typed list field `{}` references missing producer {}",
                            target.path, target.producer
                        )
                    })?;
                if matches!(producer.kind, ExecutableExpressionKind::List { .. }) {
                    continue;
                }
                return Err(format!(
                    "computed typed list field `{}` (statement {}, declaration {}, producer {}) has no derived storage value; available {:?}",
                    target.path,
                    target.statement,
                    target.declaration.0,
                    target.producer,
                    program
                        .derived_values
                        .iter()
                        .map(|value| (&value.path, value.executable_statement_id))
                        .collect::<Vec<_>>()
                ));
            };
            if value.kind != DerivedValueKind::ListView
                || value.materialized_list_id != Some(list)
                || value.materialized_row_scope_id != Some(row_scope)
            {
                return Err(format!(
                    "typed derived list field `{}` does not target its exact keyed storage",
                    target.path
                ));
            }
        }
    }
    for (index, binding) in program.view_bindings.iter().enumerate() {
        if binding.id.as_usize() != index {
            return Err(format!(
                "view binding `{}.{}` has ViewBindingId {}, expected {index}",
                binding.node_kind, binding.attr, binding.id
            ));
        }
        let node_expression = program
            .executable
            .expressions
            .get(binding.node_expression.as_usize())
            .filter(|candidate| candidate.id == binding.node_expression)
            .ok_or_else(|| {
                format!(
                    "view binding `{}.{}` references missing retained-view node expression {}",
                    binding.node_kind, binding.attr, binding.node_expression
                )
            })?;
        if !matches!(&node_expression.kind, ExecutableExpressionKind::Call { .. }) {
            return Err(format!(
                "view binding `{}.{}` node expression {} is not an exact constructor call",
                binding.node_kind, binding.attr, binding.node_expression
            ));
        }
        if program
            .executable
            .expressions
            .get(binding.argument_expression.as_usize())
            .is_none_or(|candidate| candidate.id != binding.argument_expression)
        {
            return Err(format!(
                "view binding `{}.{}` references missing constructor argument expression {}",
                binding.node_kind, binding.attr, binding.argument_expression
            ));
        }
        if let Some(scope_id) = binding.scope_id
            && !row_scope_ids.contains(&scope_id)
        {
            return Err(format!(
                "view binding `{}.{}` references missing ScopeId {}",
                binding.node_kind,
                binding.attr,
                scope_id.as_usize()
            ));
        }
        match &binding.target {
            ViewBindingTarget::Read { read, .. } => {
                if program
                    .scope_index
                    .reads
                    .get(read.as_usize())
                    .is_none_or(|candidate| candidate.id != *read)
                {
                    return Err(format!(
                        "view binding `{}.{}` references missing erased read {read}",
                        binding.node_kind, binding.attr
                    ));
                }
            }
            ViewBindingTarget::Source { source } => {
                if program
                    .sources
                    .get(source.as_usize())
                    .is_none_or(|candidate| candidate.id != *source)
                {
                    return Err(format!(
                        "view binding `{}.{}` references missing source {source}",
                        binding.node_kind, binding.attr
                    ));
                }
            }
        }
        match binding.kind {
            ViewBindingKind::Source => {
                let ViewBindingTarget::Source { source: source_id } = binding.target else {
                    return Err(format!(
                        "view source binding `{}.{}` has no exact source target",
                        binding.node_kind, binding.attr
                    ));
                };
                if source_id.as_usize() >= program.sources.len()
                    || program.sources[source_id.as_usize()].path != binding.path
                {
                    return Err(format!(
                        "view source binding `{}.{}` does not match source {source_id}",
                        binding.node_kind, binding.attr
                    ));
                }
            }
            ViewBindingKind::Data | ViewBindingKind::Target => {
                if matches!(binding.target, ViewBindingTarget::Source { .. }) {
                    return Err(format!(
                        "view data binding `{}.{}` unexpectedly targets a source",
                        binding.node_kind, binding.attr
                    ));
                }
            }
        }
    }
    verify_scope_refs(
        "source",
        program.sources.iter().filter_map(|source| source.scope_id),
        &row_scope_ids,
    )?;
    verify_scope_refs(
        "state cell",
        program.state_cells.iter().filter_map(|cell| cell.scope_id),
        &row_scope_ids,
    )?;
    verify_scope_refs(
        "derived value",
        program
            .derived_values
            .iter()
            .filter_map(|value| value.scope_id),
        &row_scope_ids,
    )?;
    let store_list_names = program
        .lists
        .iter()
        .map(|list| format!("store.{}", list.name))
        .collect::<Vec<_>>();
    let source_payload_paths = program
        .sources
        .iter()
        .flat_map(|source| {
            source.payload_schema.fields.iter().flat_map(move |field| {
                let field = field.name();
                [
                    format!("{}.{}", source.path, field),
                    source
                        .path
                        .strip_prefix("store.")
                        .map(|path| format!("{path}.{field}"))
                        .unwrap_or_else(|| format!("{}.{}", source.path, field)),
                ]
            })
        })
        .collect::<Vec<_>>();
    let materialization_local_symbols = program
        .executable
        .expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            ExecutableExpressionKind::MaterializationLocal { projection, .. }
                if !projection.is_empty() =>
            {
                Some(format!("@local.{}", projection.join(".")))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let pulse_symbols = program
        .pulse_batches
        .iter()
        .map(|pulse| format!("$pulse.p{}", pulse.id.as_usize()))
        .collect::<Vec<_>>();
    let known_symbols = source_paths
        .iter()
        .chain(state_paths.iter())
        .chain(list_names.iter())
        .chain(derived_paths.iter())
        .copied()
        .chain(store_list_names.iter().map(String::as_str))
        .chain(source_payload_paths.iter().map(String::as_str))
        .chain(materialization_local_symbols.iter().map(String::as_str))
        .chain(pulse_symbols.iter().map(String::as_str))
        .chain(program.debug_fields.iter().map(|field| field.path.as_str()))
        .chain(
            program
                .role_references()
                .value_references
                .iter()
                .map(|reference| reference.canonical_path.as_str()),
        )
        .collect::<BTreeSet<_>>();
    for edge in &program.dependencies {
        require_known_symbol("dependency source", &edge.from, &known_symbols)?;
        require_known_symbol("dependency target", &edge.to, &known_symbols)?;
    }
    for arm in program.state_updates() {
        let state = program
            .state_cells
            .get(arm.state.as_usize())
            .filter(|state| state.id == arm.state)
            .ok_or_else(|| {
                format!(
                    "state update arm references missing target StateId {}",
                    arm.state
                )
            })?;
        let cause = event_cause_path_owned(
            arm.cause,
            &program.sources,
            &program.state_cells,
            &program.pulse_batches,
        )?;
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
                format!(
                    "state update `{}` from `{cause}` has stale gate {}",
                    state.path, arm.gate_expression_id
                )
            })?;
        program
            .executable
            .expressions
            .get(arm.output_expression_id.as_usize())
            .filter(|expression| expression.id == arm.output_expression_id)
            .ok_or_else(|| {
                format!(
                    "state update `{}` from `{cause}` gate {} has missing output {}",
                    state.path, gate.id, arm.output_expression_id
                )
            })?;
    }
    for mutation in &program.list_mutations {
        let Some(list) = program.lists.get(mutation.list_id.as_usize()) else {
            return Err(format!(
                "list mutation references missing ListId {}",
                mutation.list_id
            ));
        };
        if list.id != mutation.list_id {
            return Err(format!(
                "list mutation ListId {} resolves to inconsistent list `{}`",
                mutation.list_id, list.name
            ));
        }
        let cause = event_cause_path_owned(
            mutation.cause,
            &program.sources,
            &program.state_cells,
            &program.pulse_batches,
        )?;
        let verify_expression = |id: ExecutableExprId, role: &str| {
            program
                .executable
                .expressions
                .get(id.as_usize())
                .filter(|expression| expression.id == id)
                .ok_or_else(|| {
                    format!(
                        "list mutation {} from `{cause}` references missing {role} expression {id}",
                        mutation.list_id
                    )
                })
        };
        match mutation.kind {
            ListMutationKind::Append { gate, item } => {
                verify_expression(gate, "gate")?;
                verify_expression(item, "item")?;
            }
            ListMutationKind::Remove {
                gate,
                owner,
                row_local,
                predicate,
                ..
            } => {
                verify_expression(gate, "gate")?;
                verify_expression(predicate, "predicate")?;
                if !program.materializations.iter().any(|materialization| {
                    materialization.owner == owner
                        && materialization.row_local == row_local
                        && (materialization.source_list_id == Some(mutation.list_id)
                            || materialization.target_list_id == Some(mutation.list_id))
                }) {
                    return Err(format!(
                        "list mutation {} from `{cause}` has no exact contextual owner {} local {}; candidates={:?}",
                        mutation.list_id,
                        owner,
                        row_local.0,
                        program
                            .materializations
                            .iter()
                            .filter(|materialization| {
                                materialization.owner == owner
                                    && materialization.row_local == row_local
                            })
                            .map(|materialization| (
                                materialization.id,
                                materialization.operation,
                                materialization.source_list_id,
                                materialization.target_list_id,
                            ))
                            .collect::<Vec<_>>()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn verify_executable_schedule(program: &ErasedProgram) -> Result<(), String> {
    let expressions = &program.executable.expressions;
    for (index, expression) in expressions.iter().enumerate() {
        if expression.id.as_usize() != index {
            return Err(format!(
                "executable expression {} has id {}, expected {index}",
                expression.id, expression.id
            ));
        }
        if expression
            .owner
            .is_some_and(|owner| owner.as_usize() >= program.scope_index.owners.len())
        {
            return Err(format!(
                "executable expression {} references missing static owner {:?}",
                expression.id, expression.owner
            ));
        }
        for child in executable_expression_children(&expression.kind) {
            if child.as_usize() >= index {
                return Err(format!(
                    "executable expression {} has non-topological child {}",
                    expression.id, child
                ));
            }
        }
        if let ExecutableExpressionKind::Materialize { materialization } = expression.kind
            && materialization >= program.materializations.len()
        {
            return Err(format!(
                "executable expression {} references missing materialization {}",
                expression.id, materialization
            ));
        }
        if let ExecutableExpressionKind::Call { arguments, .. } = &expression.kind {
            let mut previous = None;
            for argument in arguments {
                if previous.is_some_and(|ordinal| ordinal >= argument.ordinal) {
                    return Err(format!(
                        "executable call {} has unordered or duplicate formal ordinal {}",
                        expression.id, argument.ordinal
                    ));
                }
                previous = Some(argument.ordinal);
            }
        }
    }

    let statement_ids = program
        .executable
        .statements
        .iter()
        .map(|statement| statement.id)
        .collect::<BTreeSet<_>>();
    for statement in &program.executable.statements {
        if statement
            .value
            .is_some_and(|value| value.as_usize() >= expressions.len())
        {
            return Err(format!(
                "executable statement {} references missing expression {:?}",
                statement.id, statement.value
            ));
        }
        if let Some(child) = statement
            .children
            .iter()
            .find(|child| !statement_ids.contains(child))
        {
            return Err(format!(
                "executable statement {} references missing child {}",
                statement.id, child
            ));
        }
    }

    let local_by_owner = program
        .materializations
        .iter()
        .map(|materialization| (materialization.owner, materialization.row_local))
        .collect::<BTreeMap<_, _>>();
    for (index, materialization) in program.materializations.iter().enumerate() {
        if materialization.id != index {
            return Err(format!(
                "contextual materialization {} is stored at index {index}",
                materialization.id
            ));
        }
        if materialization.owner.as_usize() >= program.scope_index.owners.len() {
            return Err(format!(
                "contextual materialization {} references missing static owner {}",
                materialization.id, materialization.owner
            ));
        }
        for (label, root) in [
            ("source", Some(materialization.source)),
            ("body", Some(materialization.body)),
            ("direction", materialization.direction),
        ]
        .into_iter()
        .filter_map(|(label, root)| root.map(|root| (label, root)))
        {
            if root.as_usize() >= expressions.len() {
                return Err(format!(
                    "contextual materialization {} {label} references missing expression {}",
                    materialization.id, root
                ));
            }
        }
        for (key_index, key) in materialization.inherited_order.iter().enumerate() {
            if !matches!(
                key.operation,
                ContextualOperationKind::SortBy | ContextualOperationKind::ThenBy
            ) {
                return Err(format!(
                    "contextual materialization {} inherited order key {key_index} has non-order operation {:?}",
                    materialization.id, key.operation
                ));
            }
            if key_index == 0 && key.operation != ContextualOperationKind::SortBy {
                return Err(format!(
                    "contextual materialization {} inherited order chain does not start with List/sort_by",
                    materialization.id
                ));
            }
            for (label, root) in [("body", key.body), ("direction", key.direction)] {
                if root.as_usize() >= expressions.len() {
                    return Err(format!(
                        "contextual materialization {} inherited order key {key_index} {label} references missing expression {}",
                        materialization.id, root
                    ));
                }
            }
        }
        if !materialization.inherited_order.is_empty()
            && materialization.operation != ContextualOperationKind::ThenBy
        {
            return Err(format!(
                "contextual materialization {} carries inherited order keys for non-then_by operation {:?}",
                materialization.id, materialization.operation
            ));
        }
        let mut ancestor_locals = BTreeSet::new();
        let mut ancestor = program.scope_index.owners[materialization.owner.as_usize()].parent;
        while let Some(owner) = ancestor {
            if let Some(local) = local_by_owner.get(&owner).copied() {
                ancestor_locals.insert((owner, local));
            }
            ancestor = program.scope_index.owners[owner.as_usize()].parent;
        }
        verify_materialization_locals(
            expressions,
            materialization.source,
            &ancestor_locals,
            materialization.id,
        )?;
        if let Some(direction) = materialization.direction {
            verify_materialization_locals(
                expressions,
                direction,
                &ancestor_locals,
                materialization.id,
            )?;
        }
        for key in &materialization.inherited_order {
            verify_materialization_locals(
                expressions,
                key.direction,
                &ancestor_locals,
                materialization.id,
            )?;
        }
        let mut body_locals = ancestor_locals;
        body_locals.insert((materialization.owner, materialization.row_local));
        verify_materialization_locals(
            expressions,
            materialization.body,
            &body_locals,
            materialization.id,
        )?;
        for key in &materialization.inherited_order {
            verify_materialization_locals(expressions, key.body, &body_locals, materialization.id)?;
        }
    }
    verify_runtime_executable_types(program)?;
    Ok(())
}

fn verify_runtime_executable_types(program: &ErasedProgram) -> Result<(), String> {
    let mut pending = Vec::new();
    pending.extend(
        program
            .executable
            .statements
            .iter()
            .filter_map(|statement| statement.value),
    );
    pending.extend(program.executable.roots.iter().map(|root| root.expression));
    pending.extend(
        program
            .executable
            .sources
            .iter()
            .map(|source| source.expression),
    );
    pending.extend(
        program
            .executable
            .states
            .iter()
            .map(|state| state.expression),
    );
    pending.extend(
        program
            .materializations
            .iter()
            .flat_map(ContextualMaterialization::expression_roots),
    );
    pending.extend(
        program
            .state_updates()
            .iter()
            .flat_map(|arm| [arm.gate_expression_id, arm.output_expression_id]),
    );
    pending.extend(program.derived_values.iter().flat_map(|derived| {
        derived
            .trigger_arms
            .iter()
            .flat_map(|arm| [arm.gate_expression_id, arm.output_expression_id])
            .chain(derived.default_roots.iter().copied())
    }));
    pending.extend(
        program
            .output_values
            .iter()
            .map(|output| output.value_expression_id),
    );
    pending.extend(
        program
            .view_bindings
            .iter()
            .map(|binding| binding.node_expression),
    );
    pending.extend(program.view_bindings.iter().filter_map(|binding| {
        match &binding.target {
            ViewBindingTarget::Read { read, .. } => program
                .scope_index
                .reads
                .get(read.as_usize())
                .filter(|candidate| candidate.id == *read)
                .map(|read| read.expression),
            _ => None,
        }
    }));

    for materialization in &program.materializations {
        for (label, ty) in [
            ("item", &materialization.item_type),
            ("result", &materialization.result_type),
        ] {
            if runtime_type_contains_var(ty) {
                return Err(format!(
                    "contextual materialization {} has unresolved runtime {label} type {ty:?}",
                    materialization.id
                ));
            }
        }
    }

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
                format!(
                    "runtime type verification reaches missing executable expression {expression_id}"
                )
        })?;
        if runtime_type_contains_var(&expression.flow_type.ty) {
            let statements = program
                .executable
                .statements
                .iter()
                .filter(|statement| statement.value == Some(expression_id))
                .map(|statement| {
                    (
                        statement.id,
                        statement.declaration,
                        statement.kind.clone(),
                        statement.flow_type.clone(),
                    )
                })
                .collect::<Vec<_>>();
            let parents = program
                .executable
                .expressions
                .iter()
                .filter(|candidate| {
                    executable_expression_children(&candidate.kind).contains(&expression_id)
                })
                .map(|candidate| {
                    (
                        candidate.id,
                        candidate.checked_expr_id,
                        candidate.flow_type.clone(),
                        candidate.kind.clone(),
                    )
                })
                .collect::<Vec<_>>();
            return Err(format!(
                "runtime executable expression {expression_id} checked {:?} has unresolved type \
                 {:?}; kind {:?}; owner {:?}; resource path {:?}; provenance {:?}; direct \
                 statements {statements:?}; parent expressions {parents:?}",
                expression.checked_expr_id,
                expression.flow_type.ty,
                expression.kind,
                expression.owner,
                expression.resource_binding_path,
                expression.provenance
            ));
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    Ok(())
}

fn runtime_type_contains_var(ty: &boon_checked::Type) -> bool {
    match ty {
        boon_checked::Type::Var(_) => true,
        boon_checked::Type::List(item) => runtime_type_contains_var(item),
        boon_checked::Type::Map { key, value } => {
            runtime_type_contains_var(key) || runtime_type_contains_var(value)
        }
        boon_checked::Type::Set(item) => runtime_type_contains_var(item),
        boon_checked::Type::Union(members) => members.iter().any(runtime_type_contains_var),
        boon_checked::Type::Function { args, result } => {
            args.iter().any(runtime_type_contains_var) || runtime_type_contains_var(&result.ty)
        }
        boon_checked::Type::Object(shape) => shape.fields.values().any(runtime_type_contains_var),
        boon_checked::Type::VariantSet(variants) => variants.iter().any(|variant| match variant {
            boon_checked::Variant::Tag(_) => false,
            boon_checked::Variant::Tagged { fields, .. } => {
                fields.fields.values().any(runtime_type_contains_var)
            }
        }),
        boon_checked::Type::Text
        | boon_checked::Type::Number
        | boon_checked::Type::Bytes(_)
        | boon_checked::Type::Bits { .. }
        | boon_checked::Type::Absent
        | boon_checked::Type::RenderContract
        | boon_checked::Type::UnresolvedShape { .. }
        | boon_checked::Type::Unknown => false,
    }
}

fn verify_materialization_locals(
    expressions: &[ExecutableExpression],
    root: ExecutableExprId,
    allowed: &BTreeSet<(StaticOwnerId, MaterializationLocalId)>,
    materialization: usize,
) -> Result<(), String> {
    let mut stack = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression) = stack.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let node = expressions.get(expression.as_usize()).ok_or_else(|| {
            format!(
                "contextual materialization {materialization} reaches missing expression {expression}"
            )
        })?;
        if let ExecutableExpressionKind::MaterializationLocal { owner, local, .. } = node.kind
            && !allowed.contains(&(owner, local))
        {
            return Err(format!(
                "contextual materialization {materialization} reads owner {} local {:?}, allowed {:?}",
                owner, local, allowed
            ));
        }
        stack.extend(executable_expression_children(&node.kind));
    }
    Ok(())
}

fn verify_distributed_reference_schedule(program: &ErasedProgram) -> Result<(), String> {
    let scheduled_expr_ids = program
        .executable
        .expressions
        .iter()
        .map(|expression| ExprId(expression.checked_expr_id.0 as usize))
        .collect::<BTreeSet<_>>();
    let mut reference_expr_ids = BTreeSet::new();
    for reference in &program.role_references().value_references {
        if !reference_expr_ids.insert(reference.expr_id) {
            return Err(format!(
                "distributed expression {} is represented more than once",
                reference.expr_id
            ));
        }
        require_scheduled_distributed_expr(reference.expr_id, &scheduled_expr_ids)?;
        if distributed_function_role(&reference.canonical_path) != Some(reference.producer_role) {
            return Err(format!(
                "distributed value `{}` does not match producer role {:?}",
                reference.canonical_path, reference.producer_role
            ));
        }
        verify_distributed_metadata_type(
            program,
            reference.expr_id,
            reference.flow_mode,
            &reference.value_type,
            &format!("distributed value `{}`", reference.canonical_path),
        )?;
    }

    let mut call_expressions = BTreeSet::new();
    for call in &program.role_references().calls {
        if !call_expressions.insert(call.expression) {
            return Err(format!(
                "distributed expression {} is represented more than once",
                call.expression
            ));
        }
        let expression = program
            .executable
            .expressions
            .get(call.expression.as_usize())
            .filter(|candidate| candidate.id == call.expression)
            .ok_or_else(|| {
                format!(
                    "distributed call `{}` references missing executable expression {}",
                    call.canonical_function, call.expression
                )
            })?;
        if expression.owner != call.owner {
            return Err(format!(
                "distributed call `{}` executable owner does not match its concrete metadata",
                call.canonical_function
            ));
        }
        if distributed_function_role(&call.canonical_function) != Some(call.producer_role) {
            return Err(format!(
                "distributed call `{}` does not match producer role {:?}",
                call.canonical_function, call.producer_role
            ));
        }
        let result_context = format!("distributed call `{}` result", call.canonical_function);
        ensure_distributed_value_flow_is_closed(&expression.flow_type, &result_context)?;
        ensure_distributed_value_flow_is_closed(&call.result, &result_context)?;
        if expression.flow_type != call.result {
            return Err(format!(
                "{result_context} executable type does not match its boundary type"
            ));
        }
        let mut names = BTreeSet::new();
        for argument in &call.arguments {
            if !names.insert(argument.name.as_str()) {
                return Err(format!(
                    "distributed call `{}` repeats argument `{}`",
                    call.canonical_function, argument.name
                ));
            }
            let context = format!(
                "distributed call `{}` argument `{}`",
                call.canonical_function, argument.name
            );
            ensure_distributed_value_flow_is_closed(&argument.flow_type, &context)?;
            let value = program
                .executable
                .expressions
                .get(argument.value.as_usize())
                .filter(|candidate| candidate.id == argument.value)
                .ok_or_else(|| {
                    format!(
                        "{context} references missing executable expression {}",
                        argument.value
                    )
                })?;
            ensure_distributed_value_flow_is_closed(&value.flow_type, &context)?;
            if value.flow_type != argument.flow_type {
                return Err(format!(
                    "{context} executable type does not match its boundary type"
                ));
            }
        }
    }
    Ok(())
}

fn distributed_expr_type(
    expression_types: &boon_checked::ExprTypeTable,
    expr_id: usize,
) -> Result<&boon_checked::FlowType, String> {
    expression_types
        .entries
        .iter()
        .find(|entry| entry.expr_id == expr_id)
        .map(|entry| &entry.flow_type)
        .ok_or_else(|| format!("distributed expression {expr_id} has no checked type"))
}

fn require_scheduled_distributed_expr(
    expr_id: ExprId,
    scheduled_expr_ids: &BTreeSet<ExprId>,
) -> Result<(), String> {
    if scheduled_expr_ids.contains(&expr_id) {
        Ok(())
    } else {
        Err(format!(
            "distributed expression {expr_id} is not in the static schedule"
        ))
    }
}

fn verify_distributed_metadata_type(
    program: &ErasedProgram,
    expr_id: ExprId,
    flow_mode: boon_checked::FlowMode,
    metadata_type: &boon_checked::Type,
    context: &str,
) -> Result<(), String> {
    ensure_distributed_type_is_closed(metadata_type, context)?;
    let checked = distributed_expr_type(&program.expression_types, expr_id.as_usize())?;
    if checked.mode != flow_mode {
        return Err(format!("{context} flow mode does not match its metadata"));
    }
    if &checked.ty != metadata_type {
        return Err(format!(
            "{context} metadata type does not match its checked expression type"
        ));
    }
    Ok(())
}

fn unique_strings<'a>(
    label: &str,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<&'a str>, String> {
    let mut set = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(format!("{label} has empty path"));
        }
        if !set.insert(value) {
            return Err(format!("duplicate {label} `{value}`"));
        }
    }
    Ok(set)
}

fn verify_scope_refs(
    label: &str,
    refs: impl IntoIterator<Item = ScopeId>,
    row_scope_ids: &BTreeSet<ScopeId>,
) -> Result<(), String> {
    for scope_id in refs {
        if !row_scope_ids.contains(&scope_id) {
            return Err(format!(
                "{label} references missing ScopeId {}",
                scope_id.as_usize()
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TypedDerivedListTarget {
    statement: ExecutableStatementId,
    declaration: boon_checked::DeclId,
    producer: ExecutableExprId,
    path: String,
    local_name: String,
    capacity: Option<usize>,
    item_type: boon_checked::Type,
    item_fields: Vec<String>,
}

fn executable_statement_name_path(kind: &ExecutableStatementKind) -> Option<(&str, &str)> {
    match kind {
        ExecutableStatementKind::Field { name, path } => Some((name, path)),
        ExecutableStatementKind::List {
            name: Some(name),
            path: Some(path),
            ..
        } => Some((name, path)),
        _ => None,
    }
}

fn direct_list_alias_target(
    executable: &ExecutableProgram,
    statement: &ExecutableStatement,
) -> Option<boon_checked::DeclId> {
    let value = statement.value?;
    let expression = executable
        .expressions
        .get(value.as_usize())
        .filter(|expression| expression.id == value)?;
    if !matches!(&expression.flow_type.ty, boon_checked::Type::List(_)) {
        return None;
    }
    match &expression.kind {
        ExecutableExpressionKind::CanonicalRead {
            target, projection, ..
        } if projection.is_empty() => Some(*target),
        _ => None,
    }
}

fn typed_derived_list_targets(
    executable: &ExecutableProgram,
) -> Result<Vec<TypedDerivedListTarget>, String> {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    let direct_storage_statements = direct_erased_storage_statements(executable);
    for statement in &executable.statements {
        if !direct_storage_statements.contains(&statement.id) {
            continue;
        }
        let Some((name, path)) = executable_statement_name_path(&statement.kind) else {
            continue;
        };
        let Some(value) = statement.value else {
            continue;
        };
        let expression = executable
            .expressions
            .get(value.as_usize())
            .ok_or_else(|| {
                format!("typed field `{path}` references missing executable expression {value}")
            })?;
        if direct_list_alias_target(executable, statement).is_some() {
            continue;
        }
        if expression.flow_type.mode == boon_checked::FlowMode::Absent {
            continue;
        }
        let boon_checked::Type::List(item_type) = &expression.flow_type.ty else {
            continue;
        };
        if !matches!(item_type.as_ref(), boon_checked::Type::Object(_)) {
            continue;
        }
        let Some(declaration) = statement.declaration else {
            return Err(format!(
                "typed list-valued statement {} has no checked declaration",
                statement.id
            ));
        };
        if !seen.insert(declaration) {
            return Err(format!(
                "checked declaration {} has more than one executable list storage target",
                declaration.0
            ));
        }
        let mut item_fields = typed_item_field_names(item_type);
        for field in executable_list_item_field_names(executable, value) {
            if !item_fields.contains(&field) {
                item_fields.push(field);
            }
        }
        targets.push(TypedDerivedListTarget {
            statement: statement.id,
            declaration,
            producer: value,
            path: path.to_owned(),
            local_name: name.to_owned(),
            capacity: match &statement.kind {
                ExecutableStatementKind::List { capacity, .. } => *capacity,
                _ => match expression.kind {
                    ExecutableExpressionKind::List { capacity, .. } => capacity,
                    _ => None,
                },
            },
            item_type: (**item_type).clone(),
            item_fields,
        });
    }
    Ok(targets)
}

fn executable_list_item_field_names(
    executable: &ExecutableProgram,
    root: ExecutableExprId,
) -> Vec<String> {
    let mut fields = Vec::new();
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let Some(expression) = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
        else {
            continue;
        };
        match &expression.kind {
            ExecutableExpressionKind::Object {
                fields: record_fields,
            }
            | ExecutableExpressionKind::TaggedObject {
                fields: record_fields,
                ..
            } => {
                for field in record_fields {
                    if !field.spread && !fields.contains(&field.name) {
                        fields.push(field.name.clone());
                    }
                }
            }
            ExecutableExpressionKind::Then { output, .. }
            | ExecutableExpressionKind::MatchArm { output, .. } => {
                pending.extend(output.iter().copied());
            }
            ExecutableExpressionKind::When { arms, .. } => {
                pending.extend(arms.iter().map(|arm| arm.output));
            }
            ExecutableExpressionKind::Latest { branches } => {
                pending.extend(branches.iter().copied());
            }
            ExecutableExpressionKind::Hold {
                initial, updates, ..
            } => {
                pending.push(*initial);
                pending.extend(updates.iter().copied());
            }
            ExecutableExpressionKind::List { items, .. } => {
                pending.extend(items.iter().copied());
            }
            ExecutableExpressionKind::Flush { payload: input }
            | ExecutableExpressionKind::FlushBoundary { input }
            | ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => {
                pending.push(*input);
            }
            ExecutableExpressionKind::Block { bindings, result } => {
                pending.extend(bindings.iter().map(|binding| binding.value));
                pending.push(*result);
            }
            ExecutableExpressionKind::Materialize { .. } => {}
            ExecutableExpressionKind::TextTemplate { .. } => {}
            ExecutableExpressionKind::CanonicalRead { .. }
            | ExecutableExpressionKind::LocalRead { .. }
            | ExecutableExpressionKind::ExternalRead { .. }
            | ExecutableExpressionKind::ElementState { .. }
            | ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::Text { .. }
            | ExecutableExpressionKind::Number { .. }
            | ExecutableExpressionKind::Bits { .. }
            | ExecutableExpressionKind::BytesByte { .. }
            | ExecutableExpressionKind::Absent
            | ExecutableExpressionKind::Tag { .. }
            | ExecutableExpressionKind::Source { .. }
            | ExecutableExpressionKind::Call { .. }
            | ExecutableExpressionKind::UserCall { .. }
            | ExecutableExpressionKind::Infix { .. }
            | ExecutableExpressionKind::MapEntry { .. }
            | ExecutableExpressionKind::Map { .. }
            | ExecutableExpressionKind::Set { .. }
            | ExecutableExpressionKind::Bytes { .. }
            | ExecutableExpressionKind::Delimiter
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. } => {}
        }
    }
    fields
}

fn typed_item_field_names(item_type: &boon_checked::Type) -> Vec<String> {
    let boon_checked::Type::Object(shape) = item_type else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    shape
        .field_order
        .iter()
        .chain(shape.fields.keys())
        .filter(|field| seen.insert((*field).clone()))
        .cloned()
        .collect()
}

fn require_known_symbol(
    context: &str,
    value: &str,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    if symbol_known(value, known_symbols) {
        Ok(())
    } else {
        Err(format!(
            "{context} `{value}` is not in the static schedule symbol table"
        ))
    }
}

fn symbol_known(value: &str, known_symbols: &BTreeSet<&str>) -> bool {
    known_symbols.contains(value)
        || known_symbols.iter().any(|known| {
            known.strip_prefix("@local.").map_or_else(
                || symbol_is_rooted_in(value, known),
                |projection| {
                    value
                        .split_once('.')
                        .is_some_and(|(_, suffix)| suffix == projection)
                },
            )
        })
}

fn symbol_is_rooted_in(value: &str, known: &str) -> bool {
    let mut candidate = known;
    loop {
        if value == candidate
            || value
                .strip_prefix(candidate)
                .is_some_and(|suffix| suffix.starts_with('.'))
        {
            return true;
        }
        let Some((_, suffix)) = candidate.split_once('.') else {
            return false;
        };
        candidate = suffix;
    }
}

fn verify_identity_clean_identifiers(program: &ErasedProgram) -> Result<(), String> {
    for source in &program.sources {
        reject_hidden_identity_identifier("source port", &source.path)?;
    }
    for cell in &program.state_cells {
        reject_hidden_identity_identifier("state cell", &cell.path)?;
        reject_hidden_identity_identifier("hold name", &cell.hold_name)?;
    }
    for list in &program.lists {
        reject_hidden_identity_identifier("list", &list.name)?;
        reject_list_initializer_identity(&list.initializer)?;
        for input in &list.initializer_inputs {
            reject_hidden_identity_identifier("list initializer input", &input.name)?;
        }
    }
    for value in &program.derived_values {
        reject_hidden_identity_identifier("derived value", &value.path)?;
        for source in &value.sources {
            reject_hidden_identity_identifier("derived value source", source)?;
        }
    }
    for edge in &program.dependencies {
        reject_hidden_identity_identifier("dependency source", &edge.from)?;
        reject_hidden_identity_identifier("dependency target", &edge.to)?;
    }
    for projection in &program.list_projections {
        reject_hidden_identity_identifier("list projection target", &projection.target)?;
        reject_hidden_identity_identifier("list projection list", &projection.list)?;
        debug_assert!(matches!(&projection.kind, ListProjectionKind::Chunk { .. }));
    }
    Ok(())
}

fn reject_initial_value_identity(value: &InitialValue) -> Result<(), String> {
    match value {
        InitialValue::RootInitialField { path } => {
            reject_hidden_identity_identifier("root initial field", path)
        }
        InitialValue::RowInitialField { path } => {
            reject_hidden_identity_identifier("row initial field", path)
        }
        InitialValue::Tag { name } => reject_hidden_identity_identifier("Tag name", name),
        InitialValue::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown initializer", summary)
        }
        InitialValue::ExpressionAuthority | InitialValue::ResourceOnly => Ok(()),
        InitialValue::Text { .. }
        | InitialValue::Number { .. }
        | InitialValue::Bytes { .. }
        | InitialValue::Data { .. } => Ok(()),
    }
}

fn reject_list_initializer_identity(value: &ListInitializer) -> Result<(), String> {
    match value {
        ListInitializer::RecordLiteral { rows } => {
            for row in rows {
                for field in &row.fields {
                    reject_hidden_identity_identifier("list initial field", &field.name)?;
                    reject_initial_value_identity(&field.value)?;
                }
            }
            Ok(())
        }
        ListInitializer::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown list initializer", summary)
        }
        ListInitializer::Range { .. } => Ok(()),
        ListInitializer::Empty => Ok(()),
    }
}

fn reject_hidden_identity_identifier(context: &str, value: &str) -> Result<(), String> {
    if let Some(token) = hidden_identity_token(value) {
        Err(format!(
            "IR exposes hidden runtime identity token `{token}` in {context} `{value}`"
        ))
    } else {
        Ok(())
    }
}

fn hidden_identity_token(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("$boon") {
        return Some("$boon");
    }
    let tokens = lower
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty());
    const FORBIDDEN: &[&str] = &[
        "runtime_key",
        "item_key",
        "row_key",
        "hidden_key",
        "hidden_keys",
        "hidden_generation",
        "target_key",
        "target_generation",
        "source_id",
        "bind_epoch",
        "listkey",
        "slot",
    ];
    tokens.into_iter().find_map(|token| {
        FORBIDDEN
            .iter()
            .copied()
            .find(|forbidden| token == *forbidden)
    })
}

fn event_cause_path_owned(
    cause: EventCause,
    sources: &[SourcePort],
    states: &[StateCell],
    pulse_batches: &[PulseBatch],
) -> Result<String, String> {
    match cause {
        EventCause::Source(source_id) => sources
            .get(source_id.as_usize())
            .filter(|source| source.id == source_id)
            .map(|source| source.path.clone())
            .ok_or_else(|| format!("state update arm references missing SourceId {source_id}")),
        EventCause::State(state_id) => states
            .get(state_id.as_usize())
            .filter(|state| state.id == state_id)
            .map(|state| state.path.clone())
            .ok_or_else(|| format!("state update arm references missing StateId {state_id}")),
        EventCause::Pulse(pulse_id) => pulse_batches
            .get(pulse_id.as_usize())
            .filter(|pulse| pulse.id == pulse_id)
            .map(|_| format!("$pulse.p{}", pulse_id.as_usize()))
            .ok_or_else(|| format!("state update arm references missing PulseBatchId {pulse_id}")),
    }
}

#[cfg(test)]
mod typed_derived_list_storage_tests {
    use super::*;

    fn storage_for(ir: &ErasedProgram, path: &str) -> (ListId, ScopeId) {
        let derived = ir
            .derived_values
            .iter()
            .find(|value| value.path == path)
            .unwrap_or_else(|| panic!("missing derived value `{path}`"));
        assert_eq!(derived.kind, DerivedValueKind::ListView);
        let list_id = derived
            .materialized_list_id
            .unwrap_or_else(|| panic!("missing materialized ListId for `{path}`"));
        let row_scope_id = derived
            .materialized_row_scope_id
            .unwrap_or_else(|| panic!("missing materialized row ScopeId for `{path}`"));
        let list = &ir.lists[list_id.as_usize()];
        assert_eq!(list.name, path);
        assert_eq!(list.row_scope_id, Some(row_scope_id));
        assert!(list.has_generation);
        (list_id, row_scope_id)
    }

    #[test]
    fn literal_list_field_uses_direct_keyed_storage_without_a_derived_recompute() {
        let parsed = boon_parser::parse_source(
            "typed-literal-list.bn",
            r#"
defaults:
    LIST {
        [name: TEXT { one }]
        [name: TEXT { two }]
    }
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("typed literal list must lower");
        assert!(
            ir.derived_values
                .iter()
                .all(|value| value.path != "defaults"),
            "a literal initializer is storage, not a recomputed list view"
        );
        let binding = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "defaults")
            .expect("literal list storage binding");
        let ErasedBindingTarget::Value {
            field: None,
            row:
                Some(ErasedRowBinding {
                    list,
                    scope: row_scope,
                }),
        } = binding.target
        else {
            panic!("literal list must own exact keyed storage: {binding:?}");
        };
        assert_eq!(ir.lists[list.as_usize()].row_scope_id, Some(row_scope));
        assert!(matches!(
            ir.lists[list.as_usize()].initializer,
            ListInitializer::RecordLiteral { .. }
        ));
    }

    #[test]
    fn contextual_map_reuses_constructor_authority_and_preserves_row_identity() {
        let parsed = boon_parser::parse_source(
            "typed-literal-map.bn",
            r#"
mapped:
    LIST {
        [value: 1]
        [value: 2]
    }
    |> List/map(item, new: [value: item.value + 1])
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("typed literal map must lower");
        let (list, _) = storage_for(&ir, "mapped");
        assert!(matches!(
            ir.lists[list.as_usize()].initializer,
            ListInitializer::RecordLiteral { ref rows } if rows.len() == 2
        ));
        let materialization = ir
            .materializations
            .iter()
            .find(|materialization| materialization.target_list_id == Some(list))
            .expect("mapped output materialization");
        assert_eq!(materialization.source_list_id, Some(list));
        assert_eq!(
            materialization.source_scope_id,
            materialization.target_scope_id
        );
        let local = ir
            .scope_index
            .locals
            .iter()
            .find(|local| {
                local.owner == materialization.owner && local.local == materialization.row_local
            })
            .expect("mapped local");
        assert_eq!(
            local.row,
            Some(ErasedRowBinding {
                list,
                scope: materialization.target_scope_id.expect("target scope"),
            })
        );
    }

    #[test]
    fn scalar_list_literals_remain_values_without_keyed_row_storage() {
        let parsed = boon_parser::parse_source(
            "typed-scalar-list.bn",
            r#"
selected: TEXT { alpha }
selected_ids: LIST { selected }
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("scalar list must lower as an ordinary value");
        let binding = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "selected_ids")
            .expect("scalar list binding");
        assert!(matches!(
            binding.target,
            ErasedBindingTarget::Value {
                field: Some(_),
                row: None,
            }
        ));
        assert!(ir.lists.iter().all(|list| list.name != "selected_ids"));
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "selected_ids"
                && value.materialized_list_id.is_none()
                && value.kind == DerivedValueKind::Pure
        }));
    }

    #[test]
    fn direct_record_list_alias_reuses_the_target_authority_without_new_keyed_storage() {
        let parsed = boon_parser::parse_source(
            "typed-record-list-alias.bn",
            r#"
store: [
    rows: LIST {
        [id: TEXT { a }]
        [id: TEXT { b }]
    }
    selected_rows:
        rows
        |> List/filter(item, if: item.id == TEXT { a })
    selected_signals: selected_rows
]
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("a direct record-list alias must reuse target authority");

        storage_for(&ir, "store.selected_rows");
        let alias = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "store.selected_signals")
            .expect("direct alias binding");
        assert!(matches!(
            alias.target,
            ErasedBindingTarget::Value {
                field: Some(_),
                row: None,
            }
        ));
        assert!(
            ir.lists
                .iter()
                .all(|list| list.name != "store.selected_signals")
        );
    }

    #[test]
    fn function_local_record_lists_are_values_not_global_storage() {
        let parsed = boon_parser::parse_source(
            "function-local-record-list.bn",
            r#"
store: component()

FUNCTION component() {
    [
        title: TEXT { component }
        rows: LIST {
            [name: TEXT { one }]
            [name: TEXT { two }]
        }
    ]
}
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("function-local lists must lower through call ownership");
        assert!(
            ir.lists.is_empty(),
            "function-template record lists must not allocate global ListId storage: {:?}",
            ir.lists
        );
        assert!(ir.executable.statements.iter().any(|statement| {
            matches!(
                &statement.kind,
                ExecutableStatementKind::Field { path, .. } if path == "store"
            )
        }));
    }

    #[test]
    fn list_constructor_authority_is_explicit_before_backend_lowering() {
        let parsed = boon_parser::parse_source(
            "typed-list-authority.bn",
            r#"
store: [
    add: SOURCE
    candidate:
        add |> THEN {
            entries
            |> List/any(item, if: item.id == add.text)
            |> WHEN {
                True => SKIP
                False => [
                    id: add.text
                ]
            }
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
        |> List/map(item, new: entry_view(entry: item))
]

FUNCTION entry_view(entry) {
    [
        id: entry.id
    ]
}
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("list authority must lower structurally");
        let list = ir
            .lists
            .iter()
            .find(|list| list.name == "store.entries")
            .expect("entries list");
        let fields = ir
            .scope_index
            .fields
            .iter()
            .filter(|field| field.row.map(|row| row.list) == Some(list.id))
            .collect::<Vec<_>>();
        let authority = fields
            .iter()
            .copied()
            .find(|field| field.name == "id" && field.role == ErasedFieldRole::ListAuthority)
            .unwrap_or_else(|| panic!("constructor authority field among {fields:#?}"));
        let value = fields
            .iter()
            .copied()
            .find(|field| field.name == "id" && field.role == ErasedFieldRole::Value)
            .expect("mapped value field");
        assert_ne!(authority.id, value.id);
        assert!(authority.declaration.is_none());
        assert!(authority.producer.is_none());
        assert!(authority.diagnostic_path.starts_with("@authority/"));
        assert!(
            ir.debug_fields.iter().all(|field| field.id != authority.id),
            "authority storage is not a user-visible semantic field"
        );

        let any = ir
            .materializations
            .iter()
            .find(|materialization| materialization.operation == ContextualOperationKind::Any)
            .expect("List/any materialization");
        let local = ir
            .scope_index
            .locals
            .iter()
            .find(|local| local.owner == any.owner && local.local == any.row_local)
            .expect("List/any local");
        assert_eq!(
            local
                .members
                .iter()
                .find(|member| member.path == ["id"])
                .map(|member| &member.target),
            Some(&ErasedLocalMemberTarget::Field(authority.id))
        );
    }

    #[test]
    fn top_level_computed_list_gets_a_typed_derived_storage_value() {
        let parsed = boon_parser::parse_source(
            "typed-top-level-computed-list.bn",
            r#"
rows:
    LIST {
        [value: 1]
        [value: 2]
    }
mapped:
    rows
    |> List/map(item, new: [value: item.value + 1])
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("top-level computed list must lower");
        let (list, row_scope) = storage_for(&ir, "mapped");
        let binding = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "mapped")
            .expect("computed list storage binding");
        assert!(matches!(
            binding.target,
            ErasedBindingTarget::Value {
                field: None,
                row: Some(ErasedRowBinding {
                    list: binding_list,
                    scope: binding_scope,
                }),
            } if binding_list == list && binding_scope == row_scope
        ));
    }

    #[test]
    fn direct_and_wrapped_map_filter_fields_get_distinct_keyed_storage() {
        let source = r#"
FUNCTION map_rows(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new
    )
}

FUNCTION filter_rows(list, entry: OUT, predicate) {
    list
    |> List/filter(
        item: entry
        if: predicate
    )
}

store: [
    rows: LIST {
        [value: 1]
        [value: 2]
    }
    direct_mapped:
        rows
        |> List/map(item, new: [value: item.value + 1])
    wrapped_mapped:
        rows
        |> map_rows(entry, new: [value: entry.value + 1])
    direct_filtered:
        rows
        |> List/filter(item, if: item.value > 0)
    wrapped_filtered:
        rows
        |> filter_rows(entry, predicate: entry.value > 0)
]
"#;
        let parsed = boon_parser::parse_source("typed-derived-map-filter.bn", source).unwrap();
        let ir = lower(&parsed).expect("typed direct and wrapped views must lower");
        let paths = [
            "store.direct_mapped",
            "store.wrapped_mapped",
            "store.direct_filtered",
            "store.wrapped_filtered",
        ];
        let storage = paths
            .iter()
            .map(|path| storage_for(&ir, path))
            .collect::<Vec<_>>();

        assert_eq!(
            storage
                .iter()
                .map(|(list, _)| *list)
                .collect::<BTreeSet<_>>()
                .len(),
            paths.len()
        );
        assert_eq!(
            storage
                .iter()
                .map(|(_, scope)| *scope)
                .collect::<BTreeSet<_>>()
                .len(),
            paths.len()
        );
        assert!(ir.lists.iter().any(|list| list.name == "store.rows"));
        assert!(ir.lists.iter().all(|list| list.name != "rows"));
        assert!(ir.lists.iter().all(|list| {
            !matches!(
                list.name.as_str(),
                "direct_mapped" | "wrapped_mapped" | "direct_filtered" | "wrapped_filtered"
            )
        }));
        assert!(
            ir.derived_values
                .iter()
                .filter(|value| value.kind == DerivedValueKind::ListView)
                .all(|value| value.materialized_list_id.is_some()
                    && value.materialized_row_scope_id.is_some())
        );
    }

    #[test]
    fn direct_and_wrapped_chunk_fields_get_deterministic_storage_without_parser_aliases() {
        let source = r#"
FUNCTION chunk_rows(list, size) {
    list |> List/chunk(size: size)
}

store: [
    rows: LIST {
        [value: 1]
        [value: 2]
        [value: 3]
    }
    direct_chunks: rows |> List/chunk(size: 2)
    wrapped_chunks: rows |> chunk_rows(size: 2)
]
"#;
        let parsed = boon_parser::parse_source("typed-derived-chunks.bn", source).unwrap();
        let first = lower(&parsed).expect("typed chunk views must lower");
        let second = lower(&parsed).expect("repeated lowering must be deterministic");
        let first_ids = [
            storage_for(&first, "store.direct_chunks"),
            storage_for(&first, "store.wrapped_chunks"),
        ];
        let second_ids = [
            storage_for(&second, "store.direct_chunks"),
            storage_for(&second, "store.wrapped_chunks"),
        ];

        assert_eq!(first_ids, second_ids);
        assert_ne!(first_ids[0].0, first_ids[1].0);
        assert_ne!(first_ids[0].1, first_ids[1].1);
        assert!(
            first
                .lists
                .iter()
                .all(|list| !matches!(list.name.as_str(), "direct_chunks" | "wrapped_chunks"))
        );
    }

    #[test]
    fn conditional_list_view_gets_keyed_storage_without_parser_aliases() {
        let source = r#"
store: [
    rows: LIST {
        [id: TEXT { a }]
        [id: TEXT { b }]
    }
    selected:
        True |> WHEN {
            True => rows |> List/filter(item, if: item.id == TEXT { a })
            False => rows
        }
    mapped:
        selected
        |> List/map(item, new: [label: item.id])
]
"#;
        let parsed = boon_parser::parse_source("typed-derived-conditional.bn", source).unwrap();
        let ir = lower(&parsed).expect("typed conditional list view must lower");
        let selected = storage_for(&ir, "store.selected");
        let mapped = storage_for(&ir, "store.mapped");

        assert_ne!(selected, mapped);
        assert!(ir.lists.iter().any(|list| list.name == "store.rows"));
        assert!(
            ir.lists
                .iter()
                .all(|list| !matches!(list.name.as_str(), "rows" | "selected" | "mapped"))
        );
    }
}
