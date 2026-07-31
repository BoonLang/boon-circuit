#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DistributedProgramBundle {
    artifacts: Vec<ProgramArtifact>,
}

impl DistributedProgramBundle {
    pub fn new(mut artifacts: Vec<ProgramArtifact>) -> boon_runtime::RuntimeResult<Self> {
        if artifacts.len() != 3 {
            return Err(
                "distributed program requires exactly one client, one session, and one server artifact"
                    .into(),
            );
        }
        let package_id = artifacts[0].application().package_id.clone();
        let deployment_domain = artifacts[0].application().deployment_domain.clone();
        let mut roles = BTreeSet::new();
        let mut namespaces = BTreeSet::new();
        for artifact in &artifacts {
            if !roles.insert(artifact.role().as_str()) {
                return Err(
                    format!("program bundle repeats role `{}`", artifact.role().as_str()).into(),
                );
            }
            let application = artifact.application();
            if application.package_id != package_id
                || application.deployment_domain != deployment_domain
            {
                return Err(format!(
                    "{} program is outside bundle application `{package_id}` in deployment `{deployment_domain}`",
                    artifact.role().as_str()
                )
                .into());
            }
            if !namespaces.insert(application.state_namespace.clone()) {
                return Err(format!(
                    "program bundle repeats state namespace `{}`",
                    application.state_namespace
                )
                .into());
            }
        }
        let required_roles = BTreeSet::from([
            ProgramRole::Client.as_str(),
            ProgramRole::Session.as_str(),
            ProgramRole::Server.as_str(),
        ]);
        if roles != required_roles {
            return Err(
                "distributed program requires client, session, and server artifacts".into(),
            );
        }
        let graph_identity = artifacts[0]
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or("distributed artifact has no linked endpoint contract")?
            .graph
            .clone();
        let mut endpoint_contracts = Vec::with_capacity(artifacts.len());
        for artifact in &artifacts {
            let endpoint = artifact
                .plan()
                .distributed_endpoint
                .as_ref()
                .ok_or_else(|| {
                    format!(
                        "{} artifact has no linked distributed endpoint",
                        artifact.role().as_str()
                    )
                })?;
            if endpoint.graph != graph_identity || endpoint.endpoint.role != artifact.role() {
                return Err(format!(
                    "{} artifact does not match the bundle distributed graph",
                    artifact.role().as_str()
                )
                .into());
            }
            endpoint_contracts.push(endpoint.endpoint.clone());
        }
        boon_plan::DistributedGraphPlan::new(
            artifacts[0].application(),
            graph_identity,
            endpoint_contracts,
        )?;
        artifacts.sort_by_key(|artifact| program_role_rank(artifact.role()));
        Ok(Self { artifacts })
    }

    pub fn artifacts(&self) -> &[ProgramArtifact] {
        &self.artifacts
    }

    pub fn artifact(&self, role: ProgramRole) -> Option<&ProgramArtifact> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.role() == role)
    }
}

fn program_role_rank(role: ProgramRole) -> u8 {
    match role {
        ProgramRole::Client => 0,
        ProgramRole::Session => 1,
        ProgramRole::Server => 2,
    }
}

fn deterministic_program_session_id(artifact: &ProgramArtifact) -> ProgramSessionId {
    let application = artifact.application();
    let mut hasher = Sha256::new();
    hasher.update(b"boon.program-session.v1");
    for component in [
        application.package_id.as_str(),
        application.state_namespace.as_str(),
        application.deployment_domain.as_str(),
        artifact.role().as_str(),
    ] {
        hasher.update((component.len() as u64).to_be_bytes());
        hasher.update(component.as_bytes());
    }
    ProgramSessionId(format!("program-session:{:x}", hasher.finalize()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramCompletion {
    Activated {
        revision: u64,
    },
    Rejected {
        diagnostic: ProgramDiagnostic,
    },
    Stale {
        revision: u64,
        latest_requested_revision: u64,
    },
}

pub struct ProgramController {
    capability_profile: ProgramCapabilityProfile,
    latest_requested_revision: u64,
    active: Option<ProgramSession>,
    diagnostic: Option<ProgramDiagnostic>,
}

impl ProgramController {
    pub fn new(capability_profile: ProgramCapabilityProfile) -> Self {
        Self {
            capability_profile,
            latest_requested_revision: 0,
            active: None,
            diagnostic: None,
        }
    }

    pub fn request(&mut self, revision: u64) -> Result<(), ProgramDiagnostic> {
        if revision <= self.latest_requested_revision {
            return Err(ProgramDiagnostic::new(
                revision,
                ProgramDiagnosticPhase::Request,
                format!(
                    "revision must increase beyond {}",
                    self.latest_requested_revision
                ),
            ));
        }
        self.latest_requested_revision = revision;
        Ok(())
    }

    pub fn complete(
        &mut self,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> ProgramCompletion {
        let revision = match &result {
            Ok(artifact) => artifact.revision(),
            Err(diagnostic) => diagnostic.revision,
        };
        if revision != self.latest_requested_revision {
            return ProgramCompletion::Stale {
                revision,
                latest_requested_revision: self.latest_requested_revision,
            };
        }
        match result {
            Ok(artifact) if artifact.capability_profile() != self.capability_profile => self
                .reject(ProgramDiagnostic::new(
                    revision,
                    ProgramDiagnosticPhase::Capability,
                    format!(
                        "artifact profile `{}` does not match controller profile `{}`",
                        artifact.capability_profile().name(),
                        self.capability_profile.name()
                    ),
                )),
            Ok(artifact) => match ProgramSession::start(artifact) {
                Ok(session) => {
                    self.active = Some(session);
                    self.diagnostic = None;
                    ProgramCompletion::Activated { revision }
                }
                Err(diagnostic) => self.reject(diagnostic),
            },
            Err(diagnostic) => self.reject(diagnostic),
        }
    }

    pub fn latest_requested_revision(&self) -> u64 {
        self.latest_requested_revision
    }

    pub fn active(&self) -> Option<&ProgramSession> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut ProgramSession> {
        self.active.as_mut()
    }

    pub fn diagnostic(&self) -> Option<&ProgramDiagnostic> {
        self.diagnostic.as_ref()
    }

    fn reject(&mut self, diagnostic: ProgramDiagnostic) -> ProgramCompletion {
        self.diagnostic = Some(diagnostic.clone());
        ProgramCompletion::Rejected { diagnostic }
    }
}

fn validate_request(request: &ProgramCompileRequest) -> Result<(), ProgramDiagnostic> {
    let limits = program_limits(request.capability_profile);
    validate_request_with_source_limits(request, limits.max_source_units, limits.max_source_bytes)
}

fn validate_request_with_source_limits(
    request: &ProgramCompileRequest,
    max_source_units: usize,
    max_source_bytes: usize,
) -> Result<(), ProgramDiagnostic> {
    let required_profile = match request.role {
        ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
        ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
        ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
    };
    if request.capability_profile != required_profile {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "{} programs require capability profile `{}`, found `{}`",
                request.role.as_str(),
                required_profile.name(),
                request.capability_profile.name()
            ),
        ));
    }
    if request.revision == 0 {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            "revision zero is reserved for an uninitialized program",
        ));
    }
    if request.units.is_empty() || request.units.len() > max_source_units {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "source unit count {} is outside 1..={}",
                request.units.len(),
                max_source_units
            ),
        ));
    }
    let source_bytes = request
        .units
        .iter()
        .map(|unit| unit.path.len().saturating_add(unit.source.len()))
        .sum::<usize>();
    if source_bytes > max_source_bytes {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "source bundle uses {source_bytes} bytes, limit is {}",
                max_source_bytes
            ),
        ));
    }
    canonical_source_bundle(request)?;
    if !request.application.is_valid() {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            "application identity is invalid",
        ));
    }
    Ok(())
}

fn validate_plan(
    revision: u64,
    profile: ProgramCapabilityProfile,
    plan: &MachinePlan,
) -> Result<(), ProgramDiagnostic> {
    let limits = program_limits(profile);
    let capabilities = &plan.capability_summary;
    let document = plan.document_plan();
    let retained_outputs = plan
        .outputs
        .iter()
        .filter(|output| {
            matches!(
                output.contract,
                OutputContractKind::Document | OutputContractKind::Scene
            )
        })
        .count();
    let mut failures = Vec::new();
    let expected_role = match profile {
        ProgramCapabilityProfile::PublicClient => ProgramRole::Client,
        ProgramCapabilityProfile::TrustedSession => ProgramRole::Session,
        ProgramCapabilityProfile::TrustedServer => ProgramRole::Server,
    };
    if plan.program_role != expected_role {
        failures.push(format!(
            "profile `{}` requires ProgramRole::{}, compiled plan declares ProgramRole::{}",
            profile.name(),
            title_case_role(expected_role),
            title_case_role(plan.program_role)
        ));
    }
    if plan.target_profile != TargetProfile::SoftwareBounded {
        failures.push(format!(
            "profile `{}` requires target profile `software_bounded`, found `{}`",
            profile.name(),
            plan.target_profile.as_str()
        ));
    }
    if !plan.application.identity.is_valid() {
        failures.push("plan application identity is invalid".to_owned());
    }
    if !capabilities.typed_lowering_executable
        || !capabilities.executable
        || !capabilities.cpu_plan_executor_complete
    {
        let unsupported = boon_plan::cpu_plan_executor_unsupported_ops(plan)
            .into_iter()
            .take(16)
            .map(|op| format!("{}:{:?}", op.id.0, op.kind))
            .collect::<Vec<_>>()
            .join(", ");
        failures.push(format!(
            "plan is not executable for profile `{}` (unresolved refs {}, unknown ops {}, unsupported CPU ops {} [{}])",
            profile.name(),
            capabilities.unresolved_executable_ref_count,
            capabilities.unknown_plan_op_count,
            capabilities.cpu_plan_executor_unsupported_op_count,
            unsupported,
        ));
    }
    match profile {
        ProgramCapabilityProfile::PublicClient => {
            if document.is_none() {
                failures.push("program has no retained document or scene output".to_owned());
            }
            if retained_outputs != 1 {
                failures.push(format!(
                    "program must expose exactly one retained visual output, found {retained_outputs}"
                ));
            }
            let denied_effects = plan
                .effects
                .iter()
                .filter(|effect| {
                    !matches!(
                        effect.replay,
                        EffectReplay::ReadOnly | EffectReplay::ProcessScoped
                    ) || effect.barrier != EffectBarrier::None
                        || effect.schema.is_none()
                })
                .count();
            if denied_effects > 0 {
                failures.push(format!(
                    "profile `{}` forbids {denied_effects} host effect contract(s) that are not typed process-local operations without persistence barriers",
                    profile.name(),
                ));
            }
            if document.is_some_and(|document| {
                document.templates.iter().any(|template| {
                    matches!(
                        template.constructor,
                        DocumentConstructor::ElementProgram
                            | DocumentConstructor::SceneElementProgram
                    )
                })
            }) {
                failures.push(format!(
                    "profile `{}` forbids nested program hosts",
                    profile.name()
                ));
            }
        }
        ProgramCapabilityProfile::TrustedSession | ProgramCapabilityProfile::TrustedServer => {
            if document.is_some() || retained_outputs != 0 {
                failures.push(format!(
                    "{} program must not contain a retained visual output",
                    profile.name()
                ));
            }
            if plan
                .outputs
                .iter()
                .any(|output| !matches!(output.contract, OutputContractKind::HostValue { .. }))
            {
                failures.push(format!(
                    "{} program outputs must be typed host values",
                    profile.name()
                ));
            }
            if profile == ProgramCapabilityProfile::TrustedSession && !plan.host_ports.is_empty() {
                failures.push(
                    "trusted session programs cannot own process-level host ports".to_owned(),
                );
            }
        }
    }
    check_limit(
        &mut failures,
        "operations",
        capabilities.operation_count,
        limits.max_operations,
    );
    check_limit(
        &mut failures,
        "scalar slots",
        plan.storage_layout.scalar_slots.len(),
        limits.max_scalar_slots,
    );
    check_limit(
        &mut failures,
        "list slots",
        plan.storage_layout.list_slots.len(),
        limits.max_list_slots,
    );
    check_limit(
        &mut failures,
        "source routes",
        plan.source_routes.len(),
        limits.max_source_routes,
    );
    if matches!(
        profile,
        ProgramCapabilityProfile::TrustedSession | ProgramCapabilityProfile::TrustedServer
    ) {
        check_limit(
            &mut failures,
            "output roots",
            plan.outputs.len(),
            limits.max_output_roots,
        );
    }
    check_limit(
        &mut failures,
        "effect contracts",
        plan.effects.len(),
        limits.max_effect_contracts,
    );
    check_limit(
        &mut failures,
        "document expressions",
        document.map_or(0, |document| document.expressions.len()),
        limits.max_document_expressions,
    );
    check_limit(
        &mut failures,
        "document templates",
        document.map_or(0, |document| document.templates.len()),
        limits.max_document_templates,
    );
    check_limit(
        &mut failures,
        "document materializations",
        document.map_or(0, |document| document.materializations.len()),
        limits.max_document_materializations,
    );
    if let Some(capacity) = plan
        .storage_layout
        .list_slots
        .iter()
        .filter_map(|slot| slot.capacity)
        .find(|capacity| *capacity > limits.max_declared_list_capacity)
    {
        failures.push(format!(
            "declared list capacity {capacity} exceeds limit {}",
            limits.max_declared_list_capacity
        ));
    }
    match boon_plan::verify_plan(plan) {
        Ok(verification) => {
            let failed = verification
                .checks
                .iter()
                .filter(|check| !check.pass)
                .map(|check| check.id.as_str())
                .collect::<Vec<_>>();
            if verification.status != "pass" || verification.error_count != 0 || !failed.is_empty()
            {
                failures.push(format!("plan verification failed: {}", failed.join(", ")));
            }
        }
        Err(error) => failures.push(format!("plan verification failed: {error}")),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Capability,
            failures.join("; "),
        ))
    }
}

fn title_case_role(role: ProgramRole) -> &'static str {
    match role {
        ProgramRole::Client => "Client",
        ProgramRole::Session => "Session",
        ProgramRole::Server => "Server",
    }
}

fn check_limit(failures: &mut Vec<String>, label: &str, actual: usize, limit: usize) {
    if actual > limit {
        failures.push(format!("{label} count {actual} exceeds limit {limit}"));
    }
}

fn bounded_diagnostic(mut message: String) -> String {
    if message.len() <= MAX_DIAGNOSTIC_BYTES {
        return message;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message.push_str("...");
    message
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramRequestId(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProgramSessionId(pub String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramArtifactOwnership {
    pub owner: ContentArtifactOwnerId,
    pub retention: ContentArtifactRetention,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramHostRequest {
    pub request_id: ProgramRequestId,
    pub session: ProgramSessionId,
    pub host: DocumentNodeId,
    pub compile: ProgramCompileRequest,
    pub artifact_id: Option<ContentArtifactId>,
    pub artifact_ownership: Option<ProgramArtifactOwnership>,
}

impl ProgramHostRequest {
    pub const fn is_artifact_load(&self) -> bool {
        self.artifact_id.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramHostUpdate {
    pub patches: Vec<DocumentPatch>,
    pub requests: Vec<ProgramHostRequest>,
    pub rejections: Vec<ProgramRejection>,
    pub bootstrap: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramRejection {
    pub session: ProgramSessionId,
    pub diagnostic: ProgramDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramHostDiagnostic {
    pub session: ProgramSessionId,
    pub hosts: Vec<DocumentNodeId>,
    pub diagnostic: ProgramDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramHostCompletion {
    Program(ProgramCompletion),
    Superseded {
        session: ProgramSessionId,
        request_id: ProgramRequestId,
    },
    Removed {
        session: ProgramSessionId,
    },
}

#[derive(Clone)]
struct ProgramSourceRoute {
    session: ProgramSessionId,
    source_path: String,
    route: SourceRouteToken,
}

#[derive(Clone)]
struct ProgramMaterializationRoute {
    session: ProgramSessionId,
    materialization: u64,
}

#[derive(Clone)]
struct ProgramProjection {
    session: ProgramSessionId,
    descriptor: EmbeddedProgramDescriptor,
    mount: bool,
    parent_children: Vec<DocumentNodeId>,
    projected: Option<ProjectedProgram>,
}

#[derive(Clone)]
struct ProjectedProgram {
    frame: DocumentFrame,
    source_routes: BTreeMap<String, ProgramSourceRoute>,
    materialization_routes: BTreeMap<u64, ProgramMaterializationRoute>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreparedProgramPayloadIdentity {
    SourceBundle(SourceBundleDigestV1),
    ContentArtifact(ContentArtifactId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreparedProgramRevisionIdentity {
    revision: u64,
    role: ProgramRole,
    capability_profile: ProgramCapabilityProfile,
    artifact_retention: ProgramArtifactRetention,
    payload: PreparedProgramPayloadIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreparedEmbeddedProgramIdentity {
    current: PreparedProgramRevisionIdentity,
    bootstrap: Option<PreparedProgramRevisionIdentity>,
}

#[derive(Clone, Debug)]
struct PreparedProgramRevision {
    identity: PreparedProgramRevisionIdentity,
    artifact_id: Option<ContentArtifactId>,
    entry_path: String,
    units: Vec<RuntimeSourceUnit>,
}

#[derive(Clone, Debug)]
struct PreparedEmbeddedProgram {
    identity: PreparedEmbeddedProgramIdentity,
    current: PreparedProgramRevision,
    bootstrap: Option<PreparedProgramRevision>,
}

struct HostedProgram {
    controller: ProgramController,
    request_diagnostic: Option<ProgramDiagnostic>,
    latest_current_identity: Option<PreparedProgramRevisionIdentity>,
    latest_request_id: Option<ProgramRequestId>,
    latest_request_identity: Option<PreparedProgramRevisionIdentity>,
    latest_request_artifact_id: Option<ContentArtifactId>,
    latest_request_artifact_ownership: Option<ProgramArtifactOwnership>,
    equal_revision_bootstrap_current_identity: Option<PreparedProgramRevisionIdentity>,
    request_in_flight: bool,
    bootstrapping: bool,
}

fn reject_program_request(
    program: &mut HostedProgram,
    session: &ProgramSessionId,
    diagnostic: ProgramDiagnostic,
    rejections: &mut Vec<ProgramRejection>,
) {
    if program.request_diagnostic.as_ref() != Some(&diagnostic) {
        rejections.push(ProgramRejection {
            session: session.clone(),
            diagnostic: diagnostic.clone(),
        });
    }
    program.request_diagnostic = Some(diagnostic);
    program.latest_request_id = None;
    program.latest_request_identity = None;
    program.latest_request_artifact_id = None;
    program.latest_request_artifact_ownership = None;
    program.equal_revision_bootstrap_current_identity = None;
    program.request_in_flight = false;
    program.bootstrapping = false;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProgramDocumentHostStats {
    pub full_reconcile_count: u64,
    pub scoped_parent_patch_count: u64,
    pub scoped_projection_refresh_count: u64,
}

/// Owns restricted child Sessions and projects them into one retained document.
/// Compilation is deliberately caller-scheduled so no compiler work can block an
/// input or rendering transaction.
pub struct ProgramDocumentHost {
    parent_application: ApplicationIdentity,
    programs: BTreeMap<ProgramSessionId, HostedProgram>,
    projections: BTreeMap<DocumentNodeId, ProgramProjection>,
    frame: DocumentFrame,
    parent_focus: Option<DocumentNodeId>,
    parent_scroll_roots: BTreeMap<ScrollRootId, boon_document_model::ScrollState>,
    parent_materializations: BTreeSet<u64>,
    source_routes: BTreeMap<String, ProgramSourceRoute>,
    materialization_routes: BTreeMap<u64, ProgramMaterializationRoute>,
    stats: ProgramDocumentHostStats,
}

impl ProgramDocumentHost {
    pub fn mount(
        parent_application: ApplicationIdentity,
        parent: &DocumentFrame,
    ) -> (Self, Vec<ProgramHostRequest>) {
        let mut host = Self {
            parent_application,
            programs: BTreeMap::new(),
            projections: BTreeMap::new(),
            frame: parent.clone(),
            parent_focus: parent.focus.clone(),
            parent_scroll_roots: parent.scroll_roots.clone(),
            parent_materializations: frame_materializations(parent),
            source_routes: BTreeMap::new(),
            materialization_routes: BTreeMap::new(),
            stats: ProgramDocumentHostStats::default(),
        };
        let update = host.reconcile(parent);
        (host, update.requests)
    }

    pub fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    pub fn stats(&self) -> ProgramDocumentHostStats {
        self.stats
    }

    pub fn reconcile(&mut self, parent: &DocumentFrame) -> ProgramHostUpdate {
        self.reconcile_full(parent)
    }

    pub fn reconcile_with_parent_patches(
        &mut self,
        parent: &DocumentFrame,
        parent_patches: Vec<DocumentPatch>,
    ) -> ProgramHostUpdate {
        if parent_patches
            .iter()
            .any(|patch| !parent_patch_is_nonstructural(patch))
        {
            return self.reconcile_structural_parent_patches(parent, parent_patches);
        }
        self.stats.scoped_parent_patch_count = self
            .stats
            .scoped_parent_patch_count
            .saturating_add(parent_patches.len().try_into().unwrap_or(u64::MAX));

        self.parent_focus.clone_from(&parent.focus);
        self.parent_scroll_roots.clone_from(&parent.scroll_roots);
        if parent_patches
            .iter()
            .any(|patch| matches!(patch, DocumentPatch::SetListMaterialization { .. }))
        {
            self.parent_materializations = frame_materializations(parent);
        }
        let mut changed_projection_hosts = BTreeSet::new();
        let mut program_definition_changed = false;
        let touched = parent_patches
            .iter()
            .filter_map(parent_patch_target)
            .cloned()
            .collect::<BTreeSet<_>>();
        for id in touched {
            let Some(mut node) = parent.nodes.get(&id).cloned() else {
                return self.reconcile_full(parent);
            };
            if let Some(projection) = self.projections.get_mut(&id) {
                if node.kind != DocumentNodeKind::EmbeddedProgram {
                    return self.reconcile_full(parent);
                }
                let Some(descriptor) = node.embedded_program.clone() else {
                    return self.reconcile_full(parent);
                };
                let session = program_session_id(&id, &descriptor);
                let projection_identity_changed = projection.session != session
                    || projection.mount != descriptor.mount
                    || projection.descriptor.capability_profile != descriptor.capability_profile;
                let previous_program_identity = prepare_embedded_program(&projection.descriptor)
                    .map(|program| program.identity);
                let next_program_identity =
                    prepare_embedded_program(&descriptor).map(|program| program.identity);
                program_definition_changed |= projection_identity_changed
                    || previous_program_identity != next_program_identity;
                projection.session = session;
                projection.descriptor = descriptor.clone();
                projection.mount = descriptor.mount;
                projection.parent_children = node.children.clone();
                node.children.extend(projected_root_children(projection));
                if projection_identity_changed {
                    changed_projection_hosts.insert(id.clone());
                }
            }
            self.frame.nodes.insert(id, node);
        }

        let (requests, rejections) = if program_definition_changed {
            self.schedule_requests()
        } else {
            (Vec::new(), Vec::new())
        };
        let mut patches = parent_patches;
        if !changed_projection_hosts.is_empty() {
            patches.extend(self.refresh_projections(Some(&changed_projection_hosts)));
        }
        self.refresh_metadata_and_routes();
        ProgramHostUpdate {
            patches,
            requests,
            rejections,
            bootstrap: false,
        }
    }

    fn reconcile_structural_parent_patches(
        &mut self,
        parent: &DocumentFrame,
        parent_patches: Vec<DocumentPatch>,
    ) -> ProgramHostUpdate {
        if self.structural_patches_touch_projection(&parent_patches) {
            return self.reconcile_full(parent);
        }
        self.stats.scoped_parent_patch_count = self
            .stats
            .scoped_parent_patch_count
            .saturating_add(parent_patches.len().try_into().unwrap_or(u64::MAX));

        let mut touched = BTreeSet::new();
        let mut removed_roots = Vec::new();
        for patch in &parent_patches {
            match patch {
                DocumentPatch::UpsertNode(node) => {
                    touched.insert(node.id.clone());
                    if let Some(parent) = node.parent.as_ref() {
                        touched.insert(parent.clone());
                    }
                }
                DocumentPatch::RemoveNode { id } => {
                    removed_roots.push(id.clone());
                    if let Some(parent) = self
                        .frame
                        .nodes
                        .get(id)
                        .and_then(|node| node.parent.clone())
                    {
                        touched.insert(parent);
                    }
                }
                DocumentPatch::InsertChild { parent, child, .. }
                | DocumentPatch::RemoveChild { parent, child } => {
                    touched.insert(parent.clone());
                    touched.insert(child.clone());
                }
                DocumentPatch::MoveChild {
                    child, new_parent, ..
                } => {
                    if let Some(previous_parent) = self
                        .frame
                        .nodes
                        .get(child)
                        .and_then(|node| node.parent.clone())
                    {
                        touched.insert(previous_parent);
                    }
                    touched.insert(new_parent.clone());
                    touched.insert(child.clone());
                }
                patch => {
                    if let Some(id) = parent_patch_target(patch) {
                        touched.insert(id.clone());
                    }
                }
            }
        }

        for root in removed_roots {
            for node in frame_subtree_nodes(&self.frame, &root) {
                self.frame.nodes.remove(&node);
            }
        }
        for id in touched {
            match parent.nodes.get(&id).cloned() {
                Some(mut node) => {
                    if let Some(projection) = self.projections.get(&id) {
                        node.children.extend(projected_root_children(projection));
                    }
                    self.frame.nodes.insert(id, node);
                }
                None => {
                    self.frame.nodes.remove(&id);
                }
            }
        }
        self.parent_focus.clone_from(&parent.focus);
        self.parent_scroll_roots.clone_from(&parent.scroll_roots);
        self.frame.focus.clone_from(&parent.focus);
        self.frame.scroll_roots.clone_from(&parent.scroll_roots);
        self.parent_materializations = frame_materializations(parent);
        self.refresh_metadata_and_routes();
        ProgramHostUpdate {
            patches: parent_patches,
            requests: Vec::new(),
            rejections: Vec::new(),
            bootstrap: false,
        }
    }

    fn structural_patches_touch_projection(&self, patches: &[DocumentPatch]) -> bool {
        patches.iter().any(|patch| match patch {
            DocumentPatch::UpsertNode(node) => {
                node.kind == DocumentNodeKind::EmbeddedProgram
                    || self.projections.contains_key(&node.id)
            }
            DocumentPatch::RemoveNode { id } => frame_subtree_nodes(&self.frame, id)
                .iter()
                .any(|node| self.projections.contains_key(node)),
            DocumentPatch::InsertChild { parent, child, .. }
            | DocumentPatch::RemoveChild { parent, child } => {
                self.projections.contains_key(parent) || self.projections.contains_key(child)
            }
            DocumentPatch::MoveChild {
                child, new_parent, ..
            } => self.projections.contains_key(child) || self.projections.contains_key(new_parent),
            patch => parent_patch_target(patch).is_some_and(|id| self.projections.contains_key(id)),
        })
    }

    fn reconcile_full(&mut self, parent: &DocumentFrame) -> ProgramHostUpdate {
        self.stats.full_reconcile_count = self.stats.full_reconcile_count.saturating_add(1);
        let previous = self.frame.clone();
        self.parent_focus.clone_from(&parent.focus);
        self.parent_scroll_roots.clone_from(&parent.scroll_roots);
        self.parent_materializations = frame_materializations(parent);
        self.install_projections(parent);
        let (requests, rejections) = self.schedule_requests();
        self.rebuild_composed_frame(parent);
        ProgramHostUpdate {
            patches: boon_document::runtime::diff_frames(&previous, &self.frame),
            requests,
            rejections,
            bootstrap: false,
        }
    }

    fn install_projections(&mut self, parent: &DocumentFrame) {
        let mut projections = BTreeMap::new();
        for node in parent.nodes.values() {
            if node.kind != DocumentNodeKind::EmbeddedProgram {
                continue;
            }
            let Some(descriptor) = node.embedded_program.clone() else {
                continue;
            };
            let session = program_session_id(&node.id, &descriptor);
            projections.insert(
                node.id.clone(),
                ProgramProjection {
                    session,
                    descriptor: descriptor.clone(),
                    mount: descriptor.mount,
                    parent_children: node.children.clone(),
                    projected: None,
                },
            );
        }
        self.projections = projections;
    }

    fn schedule_requests(&mut self) -> (Vec<ProgramHostRequest>, Vec<ProgramRejection>) {
        let mut descriptors =
            BTreeMap::<ProgramSessionId, Vec<(DocumentNodeId, EmbeddedProgramDescriptor)>>::new();
        for (host, projection) in &self.projections {
            descriptors
                .entry(projection.session.clone())
                .or_default()
                .push((host.clone(), projection.descriptor.clone()));
        }
        self.programs
            .retain(|session, _| descriptors.contains_key(session));

        let mut requests = Vec::new();
        let mut rejections = Vec::new();
        for (session, descriptors) in descriptors {
            let (host, descriptor) = descriptors
                .first()
                .cloned()
                .expect("grouped embedded program descriptors are nonempty");
            let prepared = prepare_embedded_program(&descriptor);
            let conflict = prepared.as_ref().ok().and_then(|prepared| {
                descriptors
                    .iter()
                    .skip(1)
                    .find_map(|(conflicting_host, conflicting)| {
                        match prepare_embedded_program(conflicting) {
                            Ok(other) if other.identity == prepared.identity => None,
                            Ok(_) => Some((
                                conflicting_host.clone(),
                                ProgramDiagnostic::new(
                                    descriptor.revision.max(conflicting.revision),
                                    ProgramDiagnosticPhase::Request,
                                    format!(
                                        "logical session `{}` has conflicting descriptors at `{}` and `{}`",
                                        session.0, host.0, conflicting_host.0
                                    ),
                                ),
                            )),
                            Err(diagnostic) => Some((conflicting_host.clone(), diagnostic)),
                        }
                    })
            });
            let program = self
                .programs
                .entry(session.clone())
                .or_insert_with(|| HostedProgram {
                    controller: ProgramController::new(descriptor.capability_profile),
                    request_diagnostic: None,
                    latest_current_identity: None,
                    latest_request_id: None,
                    latest_request_identity: None,
                    latest_request_artifact_id: None,
                    latest_request_artifact_ownership: None,
                    equal_revision_bootstrap_current_identity: None,
                    request_in_flight: false,
                    bootstrapping: false,
                });
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(diagnostic) => {
                    reject_program_request(program, &session, diagnostic, &mut rejections);
                    continue;
                }
            };
            if let Some((_conflicting_host, diagnostic)) = conflict {
                reject_program_request(program, &session, diagnostic, &mut rejections);
                continue;
            }
            let current_identity = prepared.current.identity;
            if program.latest_current_identity.is_some_and(|latest| {
                current_identity.revision < latest.revision
                    || (current_identity.revision == latest.revision && current_identity != latest)
            }) {
                let latest_revision = program
                    .latest_current_identity
                    .expect("checked current identity")
                    .revision;
                reject_program_request(
                    program,
                    &session,
                    ProgramDiagnostic::new(
                        current_identity.revision,
                        ProgramDiagnosticPhase::Request,
                        format!(
                            "current program revision must increase beyond {latest_revision} before its exact identity changes"
                        ),
                    ),
                    &mut rejections,
                );
                continue;
            }
            program.latest_current_identity = Some(current_identity);
            let (request, bootstrapping) = match (
                program.controller.active().is_none(),
                prepared.bootstrap.as_ref(),
            ) {
                (true, Some(bootstrap)) => (bootstrap, true),
                _ => (&prepared.current, false),
            };
            let equal_revision_bootstrap_current_identity = (bootstrapping
                && request.identity.revision == prepared.current.identity.revision)
                .then_some(prepared.current.identity);
            let expected_application = child_application(&self.parent_application, &session);
            let request_id = program_request_id(
                &self.parent_application,
                &session,
                request.identity,
                equal_revision_bootstrap_current_identity,
            );
            if program.controller.active().is_some_and(|active| {
                prepared_revision_matches_artifact(
                    request.identity,
                    &expected_application,
                    active.artifact(),
                )
            }) {
                program.request_diagnostic = None;
                if request.identity.artifact_retention == ProgramArtifactRetention::Ephemeral {
                    program.latest_request_id = Some(request_id);
                    program.latest_request_identity = Some(request.identity);
                    program.latest_request_artifact_id = None;
                    program.latest_request_artifact_ownership = None;
                }
                program.equal_revision_bootstrap_current_identity = None;
                program.request_in_flight = false;
                program.bootstrapping = false;
                continue;
            }
            if program.latest_request_id.as_ref() == Some(&request_id)
                && program.controller.latest_requested_revision() >= request.identity.revision
            {
                continue;
            }
            program.bootstrapping = bootstrapping;
            match program.controller.request(request.identity.revision) {
                Ok(()) => {
                    program.controller.capability_profile = request.identity.capability_profile;
                    program.request_diagnostic = None;
                    let ownership_identity =
                        equal_revision_bootstrap_current_identity.unwrap_or(request.identity);
                    let artifact_ownership = program_artifact_ownership(
                        &self.parent_application,
                        &session,
                        &request_id,
                        ownership_identity.artifact_retention,
                    );
                    program.latest_request_id = Some(request_id.clone());
                    program.latest_request_identity = Some(request.identity);
                    program.latest_request_artifact_id = request.artifact_id;
                    program.latest_request_artifact_ownership = artifact_ownership;
                    program.equal_revision_bootstrap_current_identity =
                        equal_revision_bootstrap_current_identity;
                    program.request_in_flight = true;
                    requests.push(ProgramHostRequest {
                        request_id,
                        session: session.clone(),
                        host: host.clone(),
                        compile: ProgramCompileRequest {
                            revision: request.identity.revision,
                            role: request.identity.role,
                            entry_path: request.entry_path.clone(),
                            units: request.units.clone(),
                            application: expected_application,
                            capability_profile: request.identity.capability_profile,
                        },
                        artifact_id: request.artifact_id,
                        artifact_ownership,
                    });
                }
                Err(diagnostic) => {
                    reject_program_request(program, &session, diagnostic, &mut rejections);
                }
            }
        }
        (requests, rejections)
    }

    fn rebuild_composed_frame(&mut self, parent: &DocumentFrame) {
        self.frame = parent.clone();
        for projection in self.projections.values_mut() {
            projection.projected = None;
        }
        let hosts = self.projections.keys().cloned().collect::<Vec<_>>();
        for host in hosts {
            let projected = self.project_for_host(&host);
            self.install_projection(&host, projected);
        }
        self.refresh_metadata_and_routes();
    }

    fn refresh_projections(
        &mut self,
        only: Option<&BTreeSet<DocumentNodeId>>,
    ) -> Vec<DocumentPatch> {
        let hosts = self
            .projections
            .keys()
            .filter(|host| only.is_none_or(|only| only.contains(*host)))
            .cloned()
            .collect::<Vec<_>>();
        self.stats.scoped_projection_refresh_count = self
            .stats
            .scoped_projection_refresh_count
            .saturating_add(hosts.len().try_into().unwrap_or(u64::MAX));
        let mut patches = Vec::new();
        for host in hosts {
            let previous = self
                .projections
                .get(&host)
                .and_then(|projection| projection.projected.clone());
            let next = self.project_for_host(&host);
            let previous_frame = previous.as_ref().map_or_else(
                || empty_projection_frame(&host),
                |projected| projected.frame.clone(),
            );
            let next_frame = next.as_ref().map_or_else(
                || empty_projection_frame(&host),
                |projected| projected.frame.clone(),
            );
            let parent_child_count = self
                .projections
                .get(&host)
                .map_or(0, |projection| projection.parent_children.len());
            patches.extend(
                boon_document::runtime::diff_frames(&previous_frame, &next_frame)
                    .into_iter()
                    .map(|patch| offset_projection_root_patch(patch, &host, parent_child_count)),
            );
            self.install_projection(&host, next);
        }
        self.refresh_metadata_and_routes();
        patches
    }

    fn project_for_host(&self, host: &DocumentNodeId) -> Option<ProjectedProgram> {
        let projection = self.projections.get(host)?;
        if !projection.mount {
            return None;
        }
        let session = self
            .programs
            .get(&projection.session)?
            .controller
            .active()?;
        let frame = session.frame()?;
        let mut used_materializations = self.parent_materializations.clone();
        for (other_host, other) in &self.projections {
            if other_host == host {
                continue;
            }
            if let Some(projected) = &other.projected {
                used_materializations.extend(projected.materialization_routes.keys().copied());
            }
        }
        Some(project_program(
            host,
            &projection.session,
            frame,
            &mut used_materializations,
        ))
    }

    fn install_projection(&mut self, host: &DocumentNodeId, next: Option<ProjectedProgram>) {
        let Some(projection) = self.projections.get_mut(host) else {
            return;
        };
        if let Some(previous) = projection.projected.take() {
            for id in previous
                .frame
                .nodes
                .keys()
                .filter(|id| **id != previous.frame.root)
            {
                self.frame.nodes.remove(id);
            }
        }
        if let Some(projected) = &next {
            for (id, node) in &projected.frame.nodes {
                if *id != projected.frame.root {
                    self.frame.nodes.insert(id.clone(), node.clone());
                }
            }
        }
        projection.projected = next;
        if let Some(host_node) = self.frame.nodes.get_mut(host) {
            host_node.children = projection.parent_children.clone();
            host_node
                .children
                .extend(projected_root_children(projection));
        }
    }

    fn refresh_metadata_and_routes(&mut self) {
        self.frame.focus.clone_from(&self.parent_focus);
        self.frame
            .scroll_roots
            .clone_from(&self.parent_scroll_roots);
        self.source_routes.clear();
        self.materialization_routes.clear();
        for projection in self.projections.values() {
            let Some(projected) = &projection.projected else {
                continue;
            };
            self.source_routes.extend(projected.source_routes.clone());
            self.materialization_routes
                .extend(projected.materialization_routes.clone());
            self.frame
                .scroll_roots
                .extend(projected.frame.scroll_roots.clone());
            if projected.frame.focus.is_some() {
                self.frame.focus.clone_from(&projected.frame.focus);
            }
        }
    }

    pub fn complete(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> (ProgramHostCompletion, ProgramHostUpdate) {
        let expected_application = child_application(&self.parent_application, session);
        let Some(program) = self.programs.get_mut(session) else {
            return (
                ProgramHostCompletion::Removed {
                    session: session.clone(),
                },
                ProgramHostUpdate::default(),
            );
        };
        if program.latest_request_id.as_ref() != Some(request_id) || !program.request_in_flight {
            return (
                ProgramHostCompletion::Superseded {
                    session: session.clone(),
                    request_id: request_id.clone(),
                },
                ProgramHostUpdate::default(),
            );
        }
        let expected_identity = program
            .latest_request_identity
            .expect("current request IDs always carry a prepared identity");
        let equal_revision_current_identity =
            program.equal_revision_bootstrap_current_identity.take();
        let bootstrap = program.bootstrapping;
        program.request_in_flight = false;
        program.bootstrapping = false;
        let result = result
            .and_then(|artifact| {
                validate_prepared_artifact(expected_identity, &expected_application, artifact)
            })
            .and_then(|artifact| {
                let Some(current_identity) = equal_revision_current_identity else {
                    return Ok(artifact);
                };
                validate_prepared_artifact(current_identity, &expected_application, artifact)
                    .map_err(|_| {
                        ProgramDiagnostic::new(
                            current_identity.revision,
                            ProgramDiagnosticPhase::Artifact,
                            "equal-revision bootstrap artifact does not match the exact current program identity",
                        )
                    })
            });
        let completion = ProgramHostCompletion::Program(program.controller.complete(result));
        let hosts = self
            .projections
            .iter()
            .filter_map(|(host, projection)| {
                (projection.session == *session).then_some(host.clone())
            })
            .collect::<BTreeSet<_>>();
        let patches = self.refresh_projections(Some(&hosts));
        let (requests, rejections) = if bootstrap {
            self.schedule_requests()
        } else {
            (Vec::new(), Vec::new())
        };
        (
            completion,
            ProgramHostUpdate {
                patches,
                requests,
                rejections,
                bootstrap,
            },
        )
    }

    pub fn diagnostics(&self) -> Vec<ProgramHostDiagnostic> {
        self.programs
            .iter()
            .filter_map(|(session, program)| {
                program
                    .request_diagnostic
                    .as_ref()
                    .or_else(|| program.controller.diagnostic())
                    .cloned()
                    .map(|diagnostic| ProgramHostDiagnostic {
                        session: session.clone(),
                        hosts: self
                            .projections
                            .iter()
                            .filter_map(|(host, projection)| {
                                (projection.session == *session).then_some(host.clone())
                            })
                            .collect(),
                        diagnostic,
                    })
            })
            .collect()
    }

    pub fn active_artifact(&self, session: &ProgramSessionId) -> Option<&ProgramArtifact> {
        self.programs
            .get(session)?
            .controller
            .active()
            .map(ProgramSession::artifact)
    }

    pub fn request_artifact_ownership(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
    ) -> Option<ProgramArtifactOwnership> {
        self.programs.get(session).and_then(|program| {
            (program.latest_request_id.as_ref() == Some(request_id))
                .then_some(program.latest_request_artifact_ownership)
                .flatten()
        })
    }

    pub fn request_is_artifact_load(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
    ) -> bool {
        self.programs.get(session).is_some_and(|program| {
            program.latest_request_id.as_ref() == Some(request_id)
                && program.latest_request_artifact_id.is_some()
        })
    }

    pub fn request_is_current(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
    ) -> bool {
        self.programs
            .get(session)
            .is_some_and(|program| program.latest_request_id.as_ref() == Some(request_id))
    }

    pub fn validate_completion_artifact(
        &self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        artifact: &ProgramArtifact,
    ) -> Result<(), ProgramDiagnostic> {
        let Some(program) = self.programs.get(session) else {
            return Err(ProgramDiagnostic::artifact(
                artifact.revision(),
                "completed artifact belongs to a removed program session",
            ));
        };
        if program.latest_request_id.as_ref() != Some(request_id) || !program.request_in_flight {
            return Err(ProgramDiagnostic::artifact(
                artifact.revision(),
                "completed artifact request is no longer current",
            ));
        }
        let expected_application = child_application(&self.parent_application, session);
        let expected_identity = program
            .latest_request_identity
            .expect("current request IDs always carry a prepared identity");
        validate_prepared_artifact_ref(expected_identity, &expected_application, artifact)?;
        if let Some(current_identity) = program.equal_revision_bootstrap_current_identity {
            validate_prepared_artifact_ref(current_identity, &expected_application, artifact)
                .map_err(|_| {
                    ProgramDiagnostic::new(
                        current_identity.revision,
                        ProgramDiagnosticPhase::Artifact,
                        "equal-revision bootstrap artifact does not match the exact current program identity",
                    )
                })?;
        }
        Ok(())
    }

    pub fn lifecycle_source_paths(&self, session: &ProgramSessionId, intent: &str) -> Vec<String> {
        self.projections
            .iter()
            .filter(|(_, projection)| projection.session == *session)
            .filter_map(|(host, _)| self.frame.nodes.get(host))
            .flat_map(|node| node.source_bindings())
            .filter(|binding| binding.intent == intent)
            .map(|binding| binding.source_path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn source_route_token(&self, route: &str) -> Option<&SourceRouteToken> {
        self.source_routes.get(route).map(|route| &route.route)
    }

    pub fn dispatch(
        &mut self,
        sequence: u64,
        route: &str,
        source_route: SourceRouteToken,
        payload: SourcePayload,
    ) -> boon_runtime::RuntimeResult<(boon_runtime::RuntimeTurn, Vec<DocumentPatch>)> {
        let route = self
            .source_routes
            .get(route)
            .cloned()
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        if route.route != source_route {
            return Err(format!(
                "embedded source route `{}` is stale or belongs to another owner instance",
                route.source_path
            )
            .into());
        }
        let program = self
            .programs
            .get_mut(&route.session)
            .and_then(|program| program.controller.active_mut())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.session.0))?;
        let event = program
            .runtime()
            .source_event(sequence, source_route, payload)?;
        let turn = program.runtime_mut().dispatch(event)?;
        let hosts = self.hosts_for_session(&route.session);
        let patches = self.refresh_projections(Some(&hosts));
        Ok((turn, patches))
    }

    pub fn demand_document_window(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> boon_runtime::RuntimeResult<Vec<DocumentPatch>> {
        let route = self
            .materialization_routes
            .get(&materialization)
            .cloned()
            .ok_or_else(|| {
                format!("embedded program has no materialization `{materialization}`")
            })?;
        let program = self
            .programs
            .get_mut(&route.session)
            .and_then(|program| program.controller.active_mut())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.session.0))?;
        program.runtime_mut().demand_document_window_by_id(
            route.materialization,
            visible,
            overscan,
        )?;
        let hosts = self.hosts_for_session(&route.session);
        Ok(self.refresh_projections(Some(&hosts)))
    }

    pub fn owns_source_route(&self, route: &str) -> bool {
        self.source_routes.contains_key(route)
    }

    pub fn owns_materialization(&self, materialization: u64) -> bool {
        self.materialization_routes.contains_key(&materialization)
    }

    fn hosts_for_session(&self, session: &ProgramSessionId) -> BTreeSet<DocumentNodeId> {
        self.projections
            .iter()
            .filter_map(|(host, projection)| {
                (projection.session == *session).then_some(host.clone())
            })
            .collect()
    }
}

fn project_program(
    host: &DocumentNodeId,
    session: &ProgramSessionId,
    child: &DocumentFrame,
    used_materializations: &mut BTreeSet<u64>,
) -> ProjectedProgram {
    let mut frame = empty_projection_frame(host);
    let mut source_routes = BTreeMap::new();
    let mut materialization_routes = BTreeMap::new();
    let root_children = child
        .nodes
        .get(&child.root)
        .map(|root| root.children.clone())
        .unwrap_or_default();
    let child_ids = child
        .nodes
        .keys()
        .filter(|id| **id != child.root)
        .cloned()
        .collect::<Vec<_>>();

    for child_id in child_ids {
        let Some(mut node) = child.nodes.get(&child_id).cloned() else {
            continue;
        };
        node.id = namespaced_node(host, &node.id);
        node.parent = node.parent.as_ref().map(|parent_id| {
            if *parent_id == child.root {
                host.clone()
            } else {
                namespaced_node(host, parent_id)
            }
        });
        node.children = node
            .children
            .iter()
            .map(|child_id| namespaced_node(host, child_id))
            .collect();
        for binding in &mut node.source_bindings {
            let original_path = binding.source_path.clone();
            let route_key = source_route_key(host, &original_path);
            if let Some(route) = binding.route.clone() {
                source_routes.insert(
                    route_key.clone(),
                    ProgramSourceRoute {
                        session: session.clone(),
                        source_path: original_path,
                        route,
                    },
                );
            }
            binding.id =
                SourceBindingId(format!("embedded/{}/{}", namespace(&host.0), binding.id.0));
            binding.source_path = route_key;
        }
        for range in &mut node.materialized {
            let Some(original) = range.materialization else {
                continue;
            };
            let mapped = namespaced_materialization(host, original, used_materializations);
            materialization_routes.insert(
                mapped,
                ProgramMaterializationRoute {
                    session: session.clone(),
                    materialization: original,
                },
            );
            range.materialization = Some(mapped);
        }
        frame.nodes.insert(node.id.clone(), node);
    }

    if let Some(root) = frame.nodes.get_mut(&frame.root) {
        root.children = root_children
            .iter()
            .map(|child_id| namespaced_node(host, child_id))
            .collect();
    }
    frame.focus = child
        .focus
        .as_ref()
        .map(|focus| namespaced_node(host, focus));
    for (scroll_root, state) in &child.scroll_roots {
        frame.scroll_roots.insert(
            ScrollRootId(format!("embedded/{}/{}", namespace(&host.0), scroll_root.0)),
            *state,
        );
    }

    ProjectedProgram {
        frame,
        source_routes,
        materialization_routes,
    }
}

fn empty_projection_frame(host: &DocumentNodeId) -> DocumentFrame {
    DocumentFrame::empty(host.0.clone())
}

fn projected_root_children(projection: &ProgramProjection) -> Vec<DocumentNodeId> {
    projection
        .projected
        .as_ref()
        .and_then(|projected| projected.frame.nodes.get(&projected.frame.root))
        .map(|root| root.children.clone())
        .unwrap_or_default()
}

fn frame_materializations(frame: &DocumentFrame) -> BTreeSet<u64> {
    frame
        .nodes
        .values()
        .flat_map(|node| node.materialized.iter())
        .filter_map(|range| range.materialization)
        .collect()
}

fn frame_subtree_nodes(frame: &DocumentFrame, root: &DocumentNodeId) -> BTreeSet<DocumentNodeId> {
    let mut nodes = BTreeSet::new();
    let mut pending = vec![root.clone()];
    while let Some(node) = pending.pop() {
        if !nodes.insert(node.clone()) {
            continue;
        }
        if let Some(node) = frame.nodes.get(&node) {
            pending.extend(node.children.iter().rev().cloned());
        }
    }
    nodes
}

fn parent_patch_is_nonstructural(patch: &DocumentPatch) -> bool {
    matches!(
        patch,
        DocumentPatch::SetText { .. }
            | DocumentPatch::SetStyle { .. }
            | DocumentPatch::SetEmbeddedProgram { .. }
            | DocumentPatch::SetBinding { .. }
            | DocumentPatch::SetBindingAt { .. }
            | DocumentPatch::SetTextInputFocus { .. }
            | DocumentPatch::SetScroll { .. }
            | DocumentPatch::SetListMaterialization { .. }
    )
}

fn parent_patch_target(patch: &DocumentPatch) -> Option<&DocumentNodeId> {
    match patch {
        DocumentPatch::SetText { id, .. }
        | DocumentPatch::SetStyle { id, .. }
        | DocumentPatch::SetEmbeddedProgram { id, .. }
        | DocumentPatch::SetBinding { id, .. }
        | DocumentPatch::SetBindingAt { id, .. }
        | DocumentPatch::SetTextInputFocus { id, .. }
        | DocumentPatch::SetScroll { id, .. }
        | DocumentPatch::SetListMaterialization { id, .. } => Some(id),
        DocumentPatch::UpsertNode(_)
        | DocumentPatch::RemoveNode { .. }
        | DocumentPatch::InsertChild { .. }
        | DocumentPatch::RemoveChild { .. }
        | DocumentPatch::MoveChild { .. } => None,
    }
}

fn offset_projection_root_patch(
    patch: DocumentPatch,
    host: &DocumentNodeId,
    parent_child_count: usize,
) -> DocumentPatch {
    match patch {
        DocumentPatch::InsertChild {
            parent,
            child,
            index,
        } if parent == *host => DocumentPatch::InsertChild {
            parent,
            child,
            index: index.saturating_add(parent_child_count),
        },
        DocumentPatch::MoveChild {
            child,
            new_parent,
            index,
        } if new_parent == *host => DocumentPatch::MoveChild {
            child,
            new_parent,
            index: index.saturating_add(parent_child_count),
        },
        patch => patch,
    }
}

fn child_application(
    parent: &ApplicationIdentity,
    session: &ProgramSessionId,
) -> ApplicationIdentity {
    ApplicationIdentity::new(
        format!("{}.embedded", parent.package_id),
        format!("{}.{}", parent.state_namespace, namespace(&session.0)),
        parent.deployment_domain.clone(),
    )
}

fn program_request_id(
    parent: &ApplicationIdentity,
    session: &ProgramSessionId,
    identity: PreparedProgramRevisionIdentity,
    equal_revision_current_identity: Option<PreparedProgramRevisionIdentity>,
) -> ProgramRequestId {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.embedded-program-request.v3\0");
    for part in [
        parent.package_id.as_bytes(),
        parent.state_namespace.as_bytes(),
        parent.deployment_domain.as_bytes(),
        session.0.as_bytes(),
    ] {
        hash_owner_part(&mut hasher, part);
    }
    hash_prepared_program_revision_identity(&mut hasher, identity);
    match equal_revision_current_identity {
        Some(current) => {
            hash_owner_part(&mut hasher, b"equal-revision-current");
            hash_prepared_program_revision_identity(&mut hasher, current);
        }
        None => hash_owner_part(&mut hasher, b"no-equal-revision-current"),
    }
    ProgramRequestId(format!("{:x}", hasher.finalize()))
}

fn hash_prepared_program_revision_identity(
    hasher: &mut Sha256,
    identity: PreparedProgramRevisionIdentity,
) {
    for part in [
        identity.role.as_str().as_bytes(),
        identity.capability_profile.name().as_bytes(),
        identity.artifact_retention.name().as_bytes(),
    ] {
        hash_owner_part(hasher, part);
    }
    hash_owner_part(hasher, &identity.revision.to_be_bytes());
    match identity.payload {
        PreparedProgramPayloadIdentity::SourceBundle(digest) => {
            hash_owner_part(hasher, b"source-bundle-v1");
            hash_owner_part(hasher, digest.as_bytes());
            hash_owner_part(hasher, COMPILER_ID.as_bytes());
            hash_owner_part(hasher, b"software_bounded");
        }
        PreparedProgramPayloadIdentity::ContentArtifact(artifact) => {
            hash_owner_part(hasher, b"content-artifact");
            hash_owner_part(hasher, artifact.as_bytes());
        }
    }
}

fn program_artifact_ownership(
    parent: &ApplicationIdentity,
    session: &ProgramSessionId,
    request: &ProgramRequestId,
    retention: ProgramArtifactRetention,
) -> Option<ProgramArtifactOwnership> {
    let (domain, retention, include_request) = match retention {
        ProgramArtifactRetention::Ephemeral => return None,
        ProgramArtifactRetention::Replaceable => (
            b"boon.program.slot.v1".as_slice(),
            ContentArtifactRetention::Replaceable,
            false,
        ),
        ProgramArtifactRetention::Archive => (
            b"boon.program.archive.v1".as_slice(),
            ContentArtifactRetention::Immutable,
            true,
        ),
    };
    let mut hasher = Sha256::new();
    hash_owner_part(&mut hasher, domain);
    for part in [
        parent.package_id.as_bytes(),
        parent.state_namespace.as_bytes(),
        parent.deployment_domain.as_bytes(),
        session.0.as_bytes(),
    ] {
        hash_owner_part(&mut hasher, part);
    }
    if include_request {
        hash_owner_part(&mut hasher, request.0.as_bytes());
    }
    Some(ProgramArtifactOwnership {
        owner: ContentArtifactOwnerId(hasher.finalize().into()),
        retention,
    })
}

fn hash_owner_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn program_session_id(
    host: &DocumentNodeId,
    descriptor: &EmbeddedProgramDescriptor,
) -> ProgramSessionId {
    let explicit = descriptor.session_key.trim();
    if explicit.is_empty() {
        ProgramSessionId(host.0.clone())
    } else {
        ProgramSessionId(explicit.to_owned())
    }
}

fn prepare_embedded_program(
    descriptor: &EmbeddedProgramDescriptor,
) -> Result<PreparedEmbeddedProgram, ProgramDiagnostic> {
    let current = prepare_program_revision(
        descriptor.revision,
        descriptor.role,
        descriptor.capability_profile,
        descriptor.artifact_retention,
        &descriptor.source,
        &descriptor.artifact_id,
        &descriptor.support_sources,
    )?;

    let has_source = !descriptor.bootstrap_source.is_empty();
    let has_artifact = !descriptor.bootstrap_artifact_id.trim().is_empty();
    if has_source && has_artifact {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "embedded program bootstrap cannot provide both source and artifact_id",
        ));
    }
    if !descriptor.bootstrap_support_sources.is_empty() && !has_source {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "embedded program bootstrap support sources require bootstrap_source",
        ));
    }
    let has_payload = has_source || has_artifact;
    if has_payload && descriptor.bootstrap_revision == 0 {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "embedded program bootstrap payload requires a positive bootstrap_revision",
        ));
    }
    if !has_payload && descriptor.bootstrap_revision > 0 {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "positive bootstrap_revision requires bootstrap source or artifact_id",
        ));
    }
    if has_payload && descriptor.bootstrap_revision > descriptor.revision {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "bootstrap_revision must not exceed the current program revision",
        ));
    }
    let bootstrap = has_payload
        .then(|| {
            prepare_program_revision(
                descriptor.bootstrap_revision,
                descriptor.role,
                descriptor.capability_profile,
                ProgramArtifactRetention::Ephemeral,
                &descriptor.bootstrap_source,
                &descriptor.bootstrap_artifact_id,
                &descriptor.bootstrap_support_sources,
            )
        })
        .transpose()?;
    if let Some(bootstrap) = bootstrap.as_ref()
        && bootstrap.identity.revision == current.identity.revision
        && matches!(
            bootstrap.identity.payload,
            PreparedProgramPayloadIdentity::SourceBundle(_)
        )
        && bootstrap.identity.payload != current.identity.payload
    {
        return Err(ProgramDiagnostic::new(
            descriptor.revision,
            ProgramDiagnosticPhase::Request,
            "equal-revision bootstrap source must match the exact current program identity",
        ));
    }
    let bootstrap = bootstrap.filter(|bootstrap| {
        bootstrap.identity.revision != current.identity.revision
            || bootstrap.identity.payload != current.identity.payload
    });
    let identity = PreparedEmbeddedProgramIdentity {
        current: current.identity,
        bootstrap: bootstrap.as_ref().map(|bootstrap| bootstrap.identity),
    };
    Ok(PreparedEmbeddedProgram {
        identity,
        current,
        bootstrap,
    })
}

#[allow(clippy::too_many_arguments)]
fn prepare_program_revision(
    revision: u64,
    role: ProgramRole,
    capability_profile: ProgramCapabilityProfile,
    artifact_retention: ProgramArtifactRetention,
    source: &str,
    artifact_id: &str,
    support_sources: &[boon_document_model::EmbeddedProgramSourceUnit],
) -> Result<PreparedProgramRevision, ProgramDiagnostic> {
    let artifact_id = artifact_id.trim();
    let has_source = !source.is_empty();
    let has_artifact = !artifact_id.is_empty();
    if has_source && has_artifact {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Request,
            "embedded program cannot provide both source and artifact_id",
        ));
    }
    if !has_source && !has_artifact {
        return Err(ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Request,
            "embedded program requires source or artifact_id",
        ));
    }
    if has_artifact {
        if !support_sources.is_empty() {
            return Err(ProgramDiagnostic::new(
                revision,
                ProgramDiagnosticPhase::Request,
                "artifact-backed embedded program cannot also provide support sources",
            ));
        }
        if artifact_retention != ProgramArtifactRetention::Ephemeral {
            return Err(ProgramDiagnostic::new(
                revision,
                ProgramDiagnosticPhase::Request,
                "artifact-backed embedded program cannot retain an already stored artifact",
            ));
        }
        let artifact = ContentArtifactId::from_hex(artifact_id).map_err(|error| {
            ProgramDiagnostic::new(revision, ProgramDiagnosticPhase::Request, error)
        })?;
        return Ok(PreparedProgramRevision {
            identity: PreparedProgramRevisionIdentity {
                revision,
                role,
                capability_profile,
                artifact_retention,
                payload: PreparedProgramPayloadIdentity::ContentArtifact(artifact),
            },
            artifact_id: Some(artifact),
            entry_path: EMBEDDED_PROGRAM_ENTRY_PATH.to_owned(),
            units: Vec::new(),
        });
    }

    let bundle = CanonicalSourceBundleV1::new(
        EMBEDDED_PROGRAM_ENTRY_PATH,
        std::iter::once(SourceBundleUnit::new(EMBEDDED_PROGRAM_ENTRY_PATH, source)).chain(
            support_sources
                .iter()
                .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
        ),
    )
    .map_err(|error| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Request,
            format!("invalid embedded source bundle: {error}"),
        )
    })?;
    let identity = PreparedProgramRevisionIdentity {
        revision,
        role,
        capability_profile,
        artifact_retention,
        payload: PreparedProgramPayloadIdentity::SourceBundle(bundle.digest()),
    };
    let entry_path = bundle.entrypoint().to_owned();
    let units = bundle
        .units()
        .iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path().to_owned(),
            source: unit.source().to_owned(),
        })
        .collect();
    Ok(PreparedProgramRevision {
        identity,
        artifact_id: None,
        entry_path,
        units,
    })
}

fn prepared_revision_matches_artifact(
    identity: PreparedProgramRevisionIdentity,
    expected_application: &ApplicationIdentity,
    artifact: &ProgramArtifact,
) -> bool {
    identity.revision == artifact.revision()
        && identity.role == artifact.role()
        && identity.capability_profile == artifact.capability_profile()
        && expected_application == artifact.application()
        && match identity.payload {
            PreparedProgramPayloadIdentity::SourceBundle(digest) => {
                digest == artifact.source_bundle_digest_v1()
            }
            PreparedProgramPayloadIdentity::ContentArtifact(id) => id == artifact.id(),
        }
}

fn validate_prepared_artifact(
    identity: PreparedProgramRevisionIdentity,
    expected_application: &ApplicationIdentity,
    artifact: ProgramArtifact,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    validate_prepared_artifact_ref(identity, expected_application, &artifact)?;
    Ok(artifact)
}

fn validate_prepared_artifact_ref(
    identity: PreparedProgramRevisionIdentity,
    expected_application: &ApplicationIdentity,
    artifact: &ProgramArtifact,
) -> Result<(), ProgramDiagnostic> {
    prepared_revision_matches_artifact(identity, expected_application, artifact)
        .then_some(())
        .ok_or_else(|| {
            ProgramDiagnostic::new(
                identity.revision,
                ProgramDiagnosticPhase::Artifact,
                "completed artifact does not match the exact prepared revision identity",
            )
        })
}

fn namespaced_node(host: &DocumentNodeId, child: &DocumentNodeId) -> DocumentNodeId {
    DocumentNodeId(format!("embedded/{}/{}", namespace(&host.0), child.0))
}

fn source_route_key(host: &DocumentNodeId, source_path: &str) -> String {
    format!(
        "embedded-source/{}/{}",
        namespace(&host.0),
        boon_runtime::sha256_bytes(source_path.as_bytes())
    )
}

fn namespaced_materialization(
    host: &DocumentNodeId,
    materialization: u64,
    used: &mut BTreeSet<u64>,
) -> u64 {
    let digest = boon_runtime::sha256_bytes(format!("{}:{materialization}", host.0).as_bytes());
    let mut candidate = u64::from_str_radix(&digest[..16], 16).unwrap_or(materialization);
    while !used.insert(candidate) {
        candidate = candidate.wrapping_add(1);
    }
    candidate
}

fn namespace(value: &str) -> String {
    boon_runtime::sha256_bytes(value.as_bytes())
}
