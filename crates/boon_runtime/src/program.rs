use crate::{
    ApplicationIdentity, DocumentFrame, DocumentPatch, LiveRuntime, ProgramCapabilityProfile,
    RowId, RuntimeSourceUnit, SessionOptions, SourcePayload, source_units_hash,
};
use boon_compiler::{
    CompileProfile, CompilerSourceUnit, compile_runtime_source_units_to_machine_plan_with_identity,
};
use boon_document_model::{
    DocumentNodeId, DocumentNodeKind, EmbeddedProgramDescriptor, ScrollRootId, SourceBindingId,
};
use boon_plan::{DocumentConstructor, MachinePlan, OutputContractKind, TargetProfile};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

const MAX_DIAGNOSTIC_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLimits {
    pub max_source_units: usize,
    pub max_source_bytes: usize,
    pub max_operations: usize,
    pub max_scalar_slots: usize,
    pub max_list_slots: usize,
    pub max_source_routes: usize,
    pub max_document_expressions: usize,
    pub max_document_templates: usize,
    pub max_document_materializations: usize,
    pub max_declared_list_capacity: usize,
}

fn program_limits(profile: ProgramCapabilityProfile) -> ProgramLimits {
    match profile {
        ProgramCapabilityProfile::PublicDocument => ProgramLimits {
            max_source_units: 8,
            max_source_bytes: 64 * 1024,
            max_operations: 10_000,
            max_scalar_slots: 512,
            max_list_slots: 64,
            max_source_routes: 128,
            max_document_expressions: 10_000,
            max_document_templates: 2_000,
            max_document_materializations: 128,
            max_declared_list_capacity: 4_096,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramCompileRequest {
    pub revision: u64,
    pub source_label: String,
    pub units: Vec<RuntimeSourceUnit>,
    pub application: ApplicationIdentity,
    pub capability_profile: ProgramCapabilityProfile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramDiagnosticPhase {
    Request,
    Compile,
    Capability,
    Start,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramDiagnostic {
    pub revision: u64,
    pub phase: ProgramDiagnosticPhase,
    pub message: String,
}

impl ProgramDiagnostic {
    fn new(revision: u64, phase: ProgramDiagnosticPhase, message: impl Into<String>) -> Self {
        Self {
            revision,
            phase,
            message: bounded_diagnostic(message.into()),
        }
    }
}

impl fmt::Display for ProgramDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "program revision {} {:?} failed: {}",
            self.revision, self.phase, self.message
        )
    }
}

impl std::error::Error for ProgramDiagnostic {}

#[derive(Clone, Debug)]
pub struct ProgramArtifact {
    revision: u64,
    source_digest: String,
    capability_profile: ProgramCapabilityProfile,
    compile_profile: CompileProfile,
    plan: Arc<MachinePlan>,
}

impl ProgramArtifact {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn source_digest(&self) -> &str {
        &self.source_digest
    }

    pub fn capability_profile(&self) -> ProgramCapabilityProfile {
        self.capability_profile
    }

    pub fn compile_profile(&self) -> CompileProfile {
        self.compile_profile
    }

    pub fn plan(&self) -> &Arc<MachinePlan> {
        &self.plan
    }
}

pub fn compile_program_artifact(
    request: &ProgramCompileRequest,
) -> Result<ProgramArtifact, ProgramDiagnostic> {
    validate_request(request)?;
    let source_digest = source_units_hash(&request.units);
    let units = request
        .units
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path.clone(),
            source: unit.source.clone(),
        })
        .collect::<Vec<_>>();
    let compiled = compile_runtime_source_units_to_machine_plan_with_identity(
        &request.source_label,
        &units,
        TargetProfile::SoftwareBounded,
        request.application.clone(),
    )
    .map_err(|error| {
        ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Compile,
            error.to_string(),
        )
    })?;
    validate_plan(request.revision, request.capability_profile, &compiled.plan)?;
    Ok(ProgramArtifact {
        revision: request.revision,
        source_digest,
        capability_profile: request.capability_profile,
        compile_profile: compiled.profile,
        plan: Arc::new(compiled.plan),
    })
}

pub struct ProgramSession {
    artifact: ProgramArtifact,
    runtime: LiveRuntime,
}

impl ProgramSession {
    fn start(artifact: ProgramArtifact) -> Result<Self, ProgramDiagnostic> {
        let runtime = LiveRuntime::from_shared_machine_plan(
            Arc::clone(artifact.plan()),
            SessionOptions::default(),
        )
        .map_err(|error| {
            ProgramDiagnostic::new(
                artifact.revision(),
                ProgramDiagnosticPhase::Start,
                error.to_string(),
            )
        })?;
        Ok(Self { artifact, runtime })
    }

    pub fn artifact(&self) -> &ProgramArtifact {
        &self.artifact
    }

    pub fn runtime(&self) -> &LiveRuntime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut LiveRuntime {
        &mut self.runtime
    }

    pub fn frame(&self) -> &DocumentFrame {
        self.runtime
            .primary_retained_output_frame()
            .expect("validated program artifact keeps one retained visual output")
    }
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
    if request.revision == 0 {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            "revision zero is reserved for an uninitialized program",
        ));
    }
    if request.units.is_empty() || request.units.len() > limits.max_source_units {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "source unit count {} is outside 1..={}",
                request.units.len(),
                limits.max_source_units
            ),
        ));
    }
    let source_bytes = request
        .units
        .iter()
        .map(|unit| unit.path.len().saturating_add(unit.source.len()))
        .sum::<usize>();
    if source_bytes > limits.max_source_bytes {
        return Err(ProgramDiagnostic::new(
            request.revision,
            ProgramDiagnosticPhase::Request,
            format!(
                "source bundle uses {source_bytes} bytes, limit is {}",
                limits.max_source_bytes
            ),
        ));
    }
    let mut paths = BTreeSet::new();
    for unit in &request.units {
        if unit.path.trim().is_empty()
            || unit.path.trim() != unit.path
            || unit.path.starts_with('/')
            || unit.path.split('/').any(|part| part == "..")
        {
            return Err(ProgramDiagnostic::new(
                request.revision,
                ProgramDiagnosticPhase::Request,
                format!(
                    "source unit path `{}` is not a relative canonical path",
                    unit.path
                ),
            ));
        }
        if !paths.insert(unit.path.as_str()) {
            return Err(ProgramDiagnostic::new(
                request.revision,
                ProgramDiagnosticPhase::Request,
                format!("source unit path `{}` is duplicated", unit.path),
            ));
        }
    }
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
    let document = plan.document_plan().ok_or_else(|| {
        ProgramDiagnostic::new(
            revision,
            ProgramDiagnosticPhase::Capability,
            "program has no retained document or scene output",
        )
    })?;
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
    if !capabilities.executable
        || !capabilities.typed_lowering_executable
        || !capabilities.cpu_plan_executor_complete
    {
        failures.push("plan is not fully executable by the typed CPU executor".to_owned());
    }
    if retained_outputs != 1 {
        failures.push(format!(
            "program must expose exactly one retained visual output, found {retained_outputs}"
        ));
    }
    if !plan.effects.is_empty() {
        failures.push(format!(
            "profile `{}` forbids {} host effect contract(s)",
            profile.name(),
            plan.effects.len()
        ));
    }
    if document.templates.iter().any(|template| {
        matches!(
            template.constructor,
            DocumentConstructor::ElementProgram | DocumentConstructor::SceneElementProgram
        )
    }) {
        failures.push(format!(
            "profile `{}` forbids nested program hosts",
            profile.name()
        ));
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
    check_limit(
        &mut failures,
        "document expressions",
        document.expressions.len(),
        limits.max_document_expressions,
    );
    check_limit(
        &mut failures,
        "document templates",
        document.templates.len(),
        limits.max_document_templates,
    );
    check_limit(
        &mut failures,
        "document materializations",
        document.materializations.len(),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramHostRequest {
    pub request_id: ProgramRequestId,
    pub host: DocumentNodeId,
    pub compile: ProgramCompileRequest,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramHostUpdate {
    pub patches: Vec<DocumentPatch>,
    pub requests: Vec<ProgramHostRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramHostDiagnostic {
    pub host: DocumentNodeId,
    pub diagnostic: ProgramDiagnostic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramHostCompletion {
    Program(ProgramCompletion),
    Superseded {
        host: DocumentNodeId,
        request_id: ProgramRequestId,
    },
    Removed {
        host: DocumentNodeId,
    },
}

#[derive(Clone)]
struct ProgramSourceRoute {
    host: DocumentNodeId,
    source_path: String,
}

#[derive(Clone)]
struct ProgramMaterializationRoute {
    host: DocumentNodeId,
    materialization: u64,
}

struct HostedProgram {
    descriptor: EmbeddedProgramDescriptor,
    controller: ProgramController,
    request_diagnostic: Option<ProgramDiagnostic>,
    latest_request_id: Option<ProgramRequestId>,
}

/// Owns restricted child Sessions and projects them into one retained document.
/// Compilation is deliberately caller-scheduled so no compiler work can block an
/// input or rendering transaction.
pub struct ProgramDocumentHost {
    parent_application: ApplicationIdentity,
    programs: BTreeMap<DocumentNodeId, HostedProgram>,
    frame: DocumentFrame,
    source_routes: BTreeMap<String, ProgramSourceRoute>,
    materialization_routes: BTreeMap<u64, ProgramMaterializationRoute>,
}

impl ProgramDocumentHost {
    pub fn mount(
        parent_application: ApplicationIdentity,
        parent: &DocumentFrame,
    ) -> (Self, Vec<ProgramHostRequest>) {
        let mut host = Self {
            parent_application,
            programs: BTreeMap::new(),
            frame: parent.clone(),
            source_routes: BTreeMap::new(),
            materialization_routes: BTreeMap::new(),
        };
        let update = host.reconcile(parent);
        (host, update.requests)
    }

    pub fn frame(&self) -> &DocumentFrame {
        &self.frame
    }

    pub fn reconcile(&mut self, parent: &DocumentFrame) -> ProgramHostUpdate {
        let descriptors = parent
            .nodes
            .values()
            .filter_map(|node| {
                (node.kind == DocumentNodeKind::EmbeddedProgram)
                    .then(|| {
                        node.embedded_program
                            .clone()
                            .map(|program| (node.id.clone(), program))
                    })
                    .flatten()
            })
            .collect::<BTreeMap<_, _>>();
        self.programs
            .retain(|host, _| descriptors.contains_key(host));

        let mut requests = Vec::new();
        for (host, descriptor) in descriptors {
            let program = self
                .programs
                .entry(host.clone())
                .or_insert_with(|| HostedProgram {
                    controller: ProgramController::new(descriptor.capability_profile),
                    descriptor: descriptor.clone(),
                    request_diagnostic: None,
                    latest_request_id: None,
                });
            if program.descriptor == descriptor
                && program.controller.latest_requested_revision() >= descriptor.revision
            {
                continue;
            }
            if program.controller.capability_profile != descriptor.capability_profile {
                program.controller = ProgramController::new(descriptor.capability_profile);
            }
            program.descriptor = descriptor.clone();
            match program.controller.request(descriptor.revision) {
                Ok(()) => {
                    program.request_diagnostic = None;
                    let request_id =
                        program_request_id(&self.parent_application, &host, &descriptor);
                    program.latest_request_id = Some(request_id.clone());
                    requests.push(ProgramHostRequest {
                        request_id,
                        host: host.clone(),
                        compile: ProgramCompileRequest {
                            revision: descriptor.revision,
                            source_label: format!("embedded-program/{}.bn", namespace(&host.0)),
                            units: vec![RuntimeSourceUnit {
                                path: "RUN.bn".to_owned(),
                                source: descriptor.source,
                            }],
                            application: child_application(&self.parent_application, &host),
                            capability_profile: descriptor.capability_profile,
                        },
                    });
                }
                Err(diagnostic) => program.request_diagnostic = Some(diagnostic),
            }
        }
        let patches = self.refresh(parent);
        ProgramHostUpdate { patches, requests }
    }

    pub fn complete(
        &mut self,
        host: &DocumentNodeId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
        parent: &DocumentFrame,
    ) -> (ProgramHostCompletion, ProgramHostUpdate) {
        let Some(program) = self.programs.get_mut(host) else {
            return (
                ProgramHostCompletion::Removed { host: host.clone() },
                ProgramHostUpdate::default(),
            );
        };
        if program.latest_request_id.as_ref() != Some(request_id) {
            return (
                ProgramHostCompletion::Superseded {
                    host: host.clone(),
                    request_id: request_id.clone(),
                },
                ProgramHostUpdate::default(),
            );
        }
        let completion = ProgramHostCompletion::Program(program.controller.complete(result));
        let patches = self.refresh(parent);
        (
            completion,
            ProgramHostUpdate {
                patches,
                requests: Vec::new(),
            },
        )
    }

    pub fn diagnostics(&self) -> Vec<ProgramHostDiagnostic> {
        self.programs
            .iter()
            .filter_map(|(host, program)| {
                program
                    .request_diagnostic
                    .as_ref()
                    .or_else(|| program.controller.diagnostic())
                    .cloned()
                    .map(|diagnostic| ProgramHostDiagnostic {
                        host: host.clone(),
                        diagnostic,
                    })
            })
            .collect()
    }

    pub fn source_is_row_scoped(&self, route: &str) -> Option<bool> {
        let route = self.source_routes.get(route)?;
        self.programs
            .get(&route.host)?
            .controller
            .active()?
            .runtime()
            .source_is_row_scoped(&route.source_path)
    }

    pub fn row_target_for_source_text(
        &self,
        route: &str,
        text: &str,
        occurrence: usize,
    ) -> crate::RuntimeResult<Option<RowId>> {
        let route = self
            .source_routes
            .get(route)
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        let program = self
            .programs
            .get(&route.host)
            .and_then(|program| program.controller.active())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.host.0))?;
        program
            .runtime()
            .row_target_for_source_text(&route.source_path, text, occurrence)
    }

    pub fn row_target_for_source_path(
        &self,
        route: &str,
        key: u64,
        generation: u64,
    ) -> crate::RuntimeResult<RowId> {
        let route = self
            .source_routes
            .get(route)
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        let program = self
            .programs
            .get(&route.host)
            .and_then(|program| program.controller.active())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.host.0))?;
        program
            .runtime()
            .row_target_for_source_path(&route.source_path, key, generation)
    }

    pub fn dispatch(
        &mut self,
        sequence: u64,
        route: &str,
        target: Option<RowId>,
        payload: SourcePayload,
        parent: &DocumentFrame,
    ) -> crate::RuntimeResult<(crate::RuntimeTurn, Vec<DocumentPatch>)> {
        let route = self
            .source_routes
            .get(route)
            .cloned()
            .ok_or_else(|| format!("embedded program has no source route `{route}`"))?;
        let program = self
            .programs
            .get_mut(&route.host)
            .and_then(|program| program.controller.active_mut())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.host.0))?;
        let event =
            program
                .runtime()
                .source_event(sequence, &route.source_path, target, payload)?;
        let turn = program.runtime_mut().dispatch(event)?;
        let patches = self.refresh(parent);
        Ok((turn, patches))
    }

    pub fn demand_document_window(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
        parent: &DocumentFrame,
    ) -> crate::RuntimeResult<Vec<DocumentPatch>> {
        let route = self
            .materialization_routes
            .get(&materialization)
            .cloned()
            .ok_or_else(|| {
                format!("embedded program has no materialization `{materialization}`")
            })?;
        let program = self
            .programs
            .get_mut(&route.host)
            .and_then(|program| program.controller.active_mut())
            .ok_or_else(|| format!("embedded program `{}` is not active", route.host.0))?;
        program.runtime_mut().demand_document_window_by_id(
            route.materialization,
            visible,
            overscan,
        )?;
        Ok(self.refresh(parent))
    }

    pub fn owns_source_route(&self, route: &str) -> bool {
        self.source_routes.contains_key(route)
    }

    pub fn owns_materialization(&self, materialization: u64) -> bool {
        self.materialization_routes.contains_key(&materialization)
    }

    fn refresh(&mut self, parent: &DocumentFrame) -> Vec<DocumentPatch> {
        let composed = compose_frame(parent, &self.programs);
        let patches = crate::document::diff_frames(&self.frame, &composed.frame);
        self.frame = composed.frame;
        self.source_routes = composed.source_routes;
        self.materialization_routes = composed.materialization_routes;
        patches
    }
}

struct ComposedFrame {
    frame: DocumentFrame,
    source_routes: BTreeMap<String, ProgramSourceRoute>,
    materialization_routes: BTreeMap<u64, ProgramMaterializationRoute>,
}

fn compose_frame(
    parent: &DocumentFrame,
    programs: &BTreeMap<DocumentNodeId, HostedProgram>,
) -> ComposedFrame {
    let mut frame = parent.clone();
    let mut source_routes = BTreeMap::new();
    let mut materialization_routes = BTreeMap::new();
    let mut used_materializations = frame
        .nodes
        .values()
        .flat_map(|node| node.materialized.iter())
        .filter_map(|range| range.materialization)
        .collect::<BTreeSet<_>>();

    for (host, program) in programs {
        let Some(session) = program.controller.active() else {
            continue;
        };
        let child = session.frame();
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
                source_routes.insert(
                    route_key.clone(),
                    ProgramSourceRoute {
                        host: host.clone(),
                        source_path: original_path,
                    },
                );
                binding.id =
                    SourceBindingId(format!("embedded/{}/{}", namespace(&host.0), binding.id.0));
                binding.source_path = route_key;
            }
            for range in &mut node.materialized {
                let Some(original) = range.materialization else {
                    continue;
                };
                let mapped = namespaced_materialization(host, original, &mut used_materializations);
                materialization_routes.insert(
                    mapped,
                    ProgramMaterializationRoute {
                        host: host.clone(),
                        materialization: original,
                    },
                );
                range.materialization = Some(mapped);
            }
            frame.nodes.insert(node.id.clone(), node);
        }

        if let Some(host_node) = frame.nodes.get_mut(host) {
            host_node.children.extend(
                root_children
                    .iter()
                    .map(|child_id| namespaced_node(host, child_id)),
            );
        }
        if let Some(focus) = child.focus.as_ref() {
            frame.focus = Some(namespaced_node(host, focus));
        }
        for (scroll_root, state) in &child.scroll_roots {
            frame.scroll_roots.insert(
                ScrollRootId(format!("embedded/{}/{}", namespace(&host.0), scroll_root.0)),
                *state,
            );
        }
    }

    ComposedFrame {
        frame,
        source_routes,
        materialization_routes,
    }
}

fn child_application(parent: &ApplicationIdentity, host: &DocumentNodeId) -> ApplicationIdentity {
    ApplicationIdentity::new(
        format!("{}.embedded", parent.package_id),
        format!("{}.{}", parent.state_namespace, namespace(&host.0)),
        parent.deployment_domain.clone(),
    )
}

fn program_request_id(
    parent: &ApplicationIdentity,
    host: &DocumentNodeId,
    descriptor: &EmbeddedProgramDescriptor,
) -> ProgramRequestId {
    let revision = descriptor.revision.to_string();
    ProgramRequestId(crate::source_unit_parts_hash(&[
        ("parent.package_id", parent.package_id.as_str()),
        ("parent.state_namespace", parent.state_namespace.as_str()),
        (
            "parent.deployment_domain",
            parent.deployment_domain.as_str(),
        ),
        ("host", host.0.as_str()),
        ("revision", revision.as_str()),
        ("source", descriptor.source_digest.as_str()),
    ]))
}

fn namespaced_node(host: &DocumentNodeId, child: &DocumentNodeId) -> DocumentNodeId {
    DocumentNodeId(format!("embedded/{}/{}", namespace(&host.0), child.0))
}

fn source_route_key(host: &DocumentNodeId, source_path: &str) -> String {
    format!(
        "embedded-source/{}/{}",
        namespace(&host.0),
        crate::sha256_bytes(source_path.as_bytes())
    )
}

fn namespaced_materialization(
    host: &DocumentNodeId,
    materialization: u64,
    used: &mut BTreeSet<u64>,
) -> u64 {
    let digest = crate::sha256_bytes(format!("{}:{materialization}", host.0).as_bytes());
    let mut candidate = u64::from_str_radix(&digest[..16], 16).unwrap_or(materialization);
    while !used.insert(candidate) {
        candidate = candidate.wrapping_add(1);
    }
    candidate
}

fn namespace(value: &str) -> String {
    crate::sha256_bytes(value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_document_model::{DocumentNode, DocumentNodeKind, TextValue};

    fn request(revision: u64, source: &str) -> ProgramCompileRequest {
        ProgramCompileRequest {
            revision,
            source_label: "Child.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "Child.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.test-child",
                "test-child",
                "runtime-test",
            ),
            capability_profile: ProgramCapabilityProfile::PublicDocument,
        }
    }

    fn parent_frame(revision: u64, source: &str) -> DocumentFrame {
        let mut frame = DocumentFrame::empty("parent");
        let mut program = DocumentNode::new("program", DocumentNodeKind::EmbeddedProgram);
        program.parent = Some(frame.root.clone());
        program.embedded_program = Some(EmbeddedProgramDescriptor {
            source: source.to_owned(),
            source_digest: crate::sha256_bytes(source.as_bytes()),
            revision,
            capability_profile: ProgramCapabilityProfile::PublicDocument,
        });
        frame
            .nodes
            .get_mut(&frame.root)
            .unwrap()
            .children
            .push(program.id.clone());
        frame.nodes.insert(program.id.clone(), program);
        frame
    }

    fn child_source(label: &str) -> String {
        format!(
            "scene: Scene/Element/text(element: [], style: [width: Fill], text: TEXT {{ {label} }})\n"
        )
    }

    fn interactive_child_source() -> &'static str {
        r#"
store: [
    button: [events: [press: SOURCE]]
    label:
        TEXT { Before } |> HOLD label {
            store.button.events.press |> THEN { TEXT { After } }
        }
]

document: Document/new(
    root: Element/button(
        element: [events: store.button.events]
        style: [width: 120, height: 40]
        label: store.label
    )
)
"#
    }

    #[test]
    fn bounded_program_artifact_starts_one_retained_session() {
        let artifact = compile_program_artifact(&request(1, &child_source("First"))).unwrap();
        assert_eq!(artifact.revision(), 1);
        assert_eq!(
            artifact.plan().target_profile,
            TargetProfile::SoftwareBounded
        );
        let mut controller = ProgramController::new(ProgramCapabilityProfile::PublicDocument);
        controller.request(1).unwrap();
        assert_eq!(
            controller.complete(Ok(artifact)),
            ProgramCompletion::Activated { revision: 1 }
        );
        let frame = controller.active().unwrap().frame();
        assert!(
            frame
                .nodes
                .values()
                .any(|node| { node.text.as_ref().is_some_and(|text| text.text == "First") })
        );
    }

    #[test]
    fn rejected_revision_preserves_the_last_valid_session() {
        let mut controller = ProgramController::new(ProgramCapabilityProfile::PublicDocument);
        controller.request(1).unwrap();
        controller.complete(compile_program_artifact(&request(
            1,
            &child_source("Valid"),
        )));
        let valid_digest = controller
            .active()
            .unwrap()
            .artifact()
            .source_digest()
            .to_owned();

        controller.request(2).unwrap();
        let completion = controller.complete(compile_program_artifact(&request(
            2,
            "scene: Scene/Element/text(",
        )));
        assert!(matches!(completion, ProgramCompletion::Rejected { .. }));
        assert_eq!(
            controller.active().unwrap().artifact().source_digest(),
            valid_digest
        );
        assert_eq!(controller.diagnostic().unwrap().revision, 2);
    }

    #[test]
    fn stale_completion_cannot_replace_a_newer_requested_revision() {
        let older = compile_program_artifact(&request(1, &child_source("Older"))).unwrap();
        let newer = compile_program_artifact(&request(2, &child_source("Newer"))).unwrap();
        let mut controller = ProgramController::new(ProgramCapabilityProfile::PublicDocument);
        controller.request(1).unwrap();
        controller.request(2).unwrap();
        assert_eq!(
            controller.complete(Ok(older)),
            ProgramCompletion::Stale {
                revision: 1,
                latest_requested_revision: 2,
            }
        );
        assert!(controller.active().is_none());
        assert_eq!(
            controller.complete(Ok(newer)),
            ProgramCompletion::Activated { revision: 2 }
        );
        assert_eq!(controller.active().unwrap().artifact().revision(), 2);
    }

    #[test]
    fn public_document_profile_rejects_host_effects_and_oversized_source() {
        let effectful = compile_program_artifact(&request(
            1,
            r#"
path: TEXT { profile.txt }
contents: File/read_text(path: path)
scene: Scene/Element/text(element: [], style: [], text: contents)
"#,
        ))
        .unwrap_err();
        assert_eq!(effectful.phase, ProgramDiagnosticPhase::Capability);
        assert!(effectful.message.contains("forbids"));

        let oversized = "x"
            .repeat(program_limits(ProgramCapabilityProfile::PublicDocument).max_source_bytes + 1);
        let diagnostic = compile_program_artifact(&request(2, &oversized)).unwrap_err();
        assert_eq!(diagnostic.phase, ProgramDiagnosticPhase::Request);
        assert!(diagnostic.message.contains("source bundle uses"));
    }

    #[test]
    fn embedded_program_constructor_keeps_typed_private_descriptor() {
        let source = r#"
scene: Scene/Element/program(
    element: []
    style: [width: Fill, height: Fill]
    source: TEXT { child source }
    revision: 7
    capability_profile: PublicDocument
)
"#;
        let runtime = LiveRuntime::from_source("embedded-program.bn", source).unwrap();
        let frame = runtime.primary_retained_output_frame().unwrap();
        let node = frame
            .nodes
            .values()
            .find(|node| node.kind == DocumentNodeKind::EmbeddedProgram)
            .expect("embedded program node");
        let descriptor = node.embedded_program.as_ref().expect("program descriptor");

        assert_eq!(descriptor.source, "child source");
        assert_eq!(descriptor.revision, 7);
        assert_eq!(
            descriptor.capability_profile,
            ProgramCapabilityProfile::PublicDocument
        );
        assert_eq!(
            descriptor.source_digest,
            crate::sha256_bytes(b"child source")
        );
        assert!(!format!("{node:?}").contains("child source"));
    }

    #[test]
    fn document_host_composes_child_and_preserves_it_across_invalid_source() {
        let first_parent = parent_frame(1, &child_source("first"));
        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "test", "local"),
            &first_parent,
        );
        assert_eq!(requests.len(), 1);
        let first = compile_program_artifact(&requests[0].compile);
        let (completion, update) = host.complete(
            &requests[0].host,
            &requests[0].request_id,
            first,
            &first_parent,
        );
        assert_eq!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 1 })
        );
        assert!(!update.patches.is_empty());
        assert!(host.frame().nodes.values().any(|node| {
            node.text.as_ref()
                == Some(&TextValue {
                    text: "first".to_owned(),
                })
        }));

        let invalid_parent = parent_frame(2, "scene: Missing/constructor(");
        let invalid = host.reconcile(&invalid_parent);
        assert_eq!(invalid.requests.len(), 1);
        let failed = compile_program_artifact(&invalid.requests[0].compile);
        let (completion, _) = host.complete(
            &invalid.requests[0].host,
            &invalid.requests[0].request_id,
            failed,
            &invalid_parent,
        );
        assert!(matches!(
            completion,
            ProgramHostCompletion::Program(ProgramCompletion::Rejected { .. })
        ));
        assert_eq!(host.diagnostics().len(), 1);
        assert!(host.frame().nodes.values().any(|node| {
            node.text.as_ref()
                == Some(&TextValue {
                    text: "first".to_owned(),
                })
        }));
    }

    #[test]
    fn document_host_routes_namespaced_input_into_the_child_session() {
        let parent = parent_frame(1, interactive_child_source());
        let (mut host, requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "input", "local"),
            &parent,
        );
        host.complete(
            &requests[0].host,
            &requests[0].request_id,
            compile_program_artifact(&requests[0].compile),
            &parent,
        );
        let route = host
            .frame()
            .nodes
            .values()
            .find(|node| node.text.as_ref().is_some_and(|text| text.text == "Before"))
            .and_then(|node| node.primary_source_binding())
            .map(|binding| binding.source_path.clone())
            .expect("composed child button source route");
        assert!(host.owns_source_route(&route));

        let (_, patches) = host
            .dispatch(1, &route, None, SourcePayload::default(), &parent)
            .unwrap();
        assert!(!patches.is_empty());
        assert!(
            host.frame()
                .nodes
                .values()
                .any(|node| { node.text.as_ref().is_some_and(|text| text.text == "After") })
        );
    }

    #[test]
    fn document_host_rejects_stale_completion_then_activates_latest() {
        let first_parent = parent_frame(1, &child_source("first"));
        let (mut host, first_requests) = ProgramDocumentHost::mount(
            ApplicationIdentity::new("dev.boon.parent", "stale", "local"),
            &first_parent,
        );
        let first = compile_program_artifact(&first_requests[0].compile);
        host.complete(
            &first_requests[0].host,
            &first_requests[0].request_id,
            first,
            &first_parent,
        );

        let second_parent = parent_frame(2, &child_source("second"));
        let second = host.reconcile(&second_parent).requests.remove(0);
        let second_artifact = compile_program_artifact(&second.compile);
        let third_parent = parent_frame(3, &child_source("third"));
        let third = host.reconcile(&third_parent).requests.remove(0);
        let third_artifact = compile_program_artifact(&third.compile);

        let (stale, stale_update) = host.complete(
            &second.host,
            &second.request_id,
            second_artifact,
            &third_parent,
        );
        assert_eq!(
            stale,
            ProgramHostCompletion::Superseded {
                host: second.host.clone(),
                request_id: second.request_id.clone(),
            }
        );
        assert!(stale_update.patches.is_empty());
        let (latest, _) = host.complete(
            &third.host,
            &third.request_id,
            third_artifact,
            &third_parent,
        );
        assert_eq!(
            latest,
            ProgramHostCompletion::Program(ProgramCompletion::Activated { revision: 3 })
        );
        assert!(host.frame().nodes.values().any(|node| {
            node.text.as_ref()
                == Some(&TextValue {
                    text: "third".to_owned(),
                })
        }));
    }
}
