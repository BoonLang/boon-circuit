use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, VecDeque};
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixListener;
#[cfg(target_os = "linux")]
use std::process::{Command, ExitStatus, Stdio};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Arc, mpsc};
#[cfg(target_os = "linux")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::budget::{BudgetContract, BudgetUnit};
use crate::observer::AsyncLaneKind;

#[cfg(target_os = "linux")]
use crate::observer::{
    AsyncLaneOutcome, FrameEvidenceKey, FramePresented, InputAccepted, InputKind,
    MIGRATION_EVIDENCE_ENV, NATIVE_SESSION_ID_ENV, NATIVE_WORKFLOW_PROOF_STEPS_ENV,
    NATIVE_WORKFLOW_STEPS_ENV, OBSERVER_SOCKET_ENV, ObserverEvent, ObserverRole,
    PERSISTENCE_EVIDENCE_ENV, PRODUCT_PROOF_AFTER_TEST_ENV, PROFILE_BENCHMARK_ENV,
    PROFILE_BENCHMARK_STEPS_ENV, PROOF_ARTIFACT_DIR_ENV, PROOF_MODE_ENV, PROOF_SAMPLE_ORDINAL_ENV,
    PersistenceEvidenceKind, ProofArtifact, RESPONSIVE_EVIDENCE_WIDTH_ENV, RoleMetadata,
    SCROLL_PROOF_ORDINAL_ENV, STALE_PROGRAM_EVIDENCE_ENV, STATE_EVIDENCE_STEPS_ENV,
    STATE_MOUNT_EVIDENCE_ENV, StartupDisposition, TestPointerPhase, read_event,
};
#[cfg(target_os = "linux")]
use crate::proof::frame_capture_token_digest;
#[cfg(target_os = "linux")]
use crate::{
    native_input::NativeInput, ui::DEV_EDITOR_INPUT_TARGET, workspace_control::WorkspaceGuard,
};

const FORMAT_VERSION: u16 = 2;
const PROTOCOL: &str = "boon-gate-evidence-v2";
const MAX_DETAIL_BYTES: usize = 1_000;
#[cfg(target_os = "linux")]
const ROLE_READY_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(target_os = "linux")]
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "linux")]
const INPUT_CALIBRATION_QUIET: Duration = Duration::from_millis(20);
#[cfg(target_os = "linux")]
const DRAG_GRAB_SETTLE: Duration = Duration::from_millis(32);
#[cfg(target_os = "linux")]
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(target_os = "linux")]
const MAX_OBSERVER_EVENTS: usize = 8_192;
#[cfg(target_os = "linux")]
const OBSERVER_QUEUE_DEPTH: usize = 8_192;
#[cfg(target_os = "linux")]
const NATIVE_WORKSPACE: &str = "boon-circuit";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HarnessKind {
    Timed,
    Negative,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisibleSampleMode {
    Click,
    Hover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AlternateTarget {
    None,
    Any,
    SameSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VerifierProfile {
    gate: String,
    id: String,
    digest: String,
    harness: HarnessKind,
    example: Option<String>,
    visible_mode: VisibleSampleMode,
    visible_samples: usize,
    alternate_target: AlternateTarget,
    selection_samples: usize,
    scroll_samples: usize,
    switch_samples: usize,
    scenario_proof: Option<PathBuf>,
    require_semantic_scenario: bool,
    budget_proof: Option<PathBuf>,
    loaded_budget: Option<LoadedBudgetContract>,
    required_budget_metrics: Vec<String>,
    required_async_lanes: Vec<AsyncLaneKind>,
    profile_benchmark_steps: Vec<String>,
    state_root_policy: Option<String>,
    restart_required: bool,
    required_checkpoints: Vec<VerifierCheckpointRequirement>,
    required_native_workflow_steps: Vec<String>,
    required_native_workflow_proof_steps: BTreeSet<String>,
    native_workflow_delivery: Option<String>,
    native_workflow_scenario_boundary: Option<String>,
    native_workflow_capture_method: Option<String>,
    native_workflow_durability: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadedBudgetContract {
    declared_path: PathBuf,
    source: String,
    contract: BudgetContract,
}

impl LoadedBudgetContract {
    fn load(declared_path: &Path) -> Result<Self, String> {
        let filesystem_path = resolve_profile_input(declared_path);
        let source = fs::read_to_string(&filesystem_path)
            .map_err(|error| format!("read budget {}: {error}", filesystem_path.display()))?;
        let contract = BudgetContract::parse(&source)
            .map_err(|error| format!("parse budget {}: {error}", filesystem_path.display()))?;
        Ok(Self {
            declared_path: declared_path.to_path_buf(),
            source,
            contract,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct VerifierCheckpointRequirement {
    id: String,
    #[serde(flatten)]
    evidence: VerifierCheckpointRequirementKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum VerifierCheckpointRequirementKind {
    ScenarioStep {
        scenario_step: String,
    },
    RestartRestore {
        baseline_checkpoint: String,
    },
    ResponsiveLayout {
        baseline_checkpoint: String,
        logical_width: u32,
    },
    StaleCompileRejection,
    PersistenceOperation {
        operation: VerifierPersistenceOperation,
    },
    NativeWorkflowStep {
        scenario_step: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum VerifierPersistenceOperation {
    Exported,
    CorruptionRejected,
    ClearedAndStartedOver,
    ImportPreviewed,
    ImportActivated,
    MigrationActivated,
}

impl VerifierProfile {
    fn parse(args: &[String]) -> Result<Self, String> {
        let harness = match required(args, "--harness")? {
            "timed" => HarnessKind::Timed,
            "negative" => HarnessKind::Negative,
            value => return Err(format!("unsupported verifier harness `{value}`")),
        };
        let visible_mode = match optional(args, "--visible-mode").unwrap_or("hover") {
            "click" => VisibleSampleMode::Click,
            "hover" => VisibleSampleMode::Hover,
            value => return Err(format!("unsupported visible sample mode `{value}`")),
        };
        let alternate_target = match optional(args, "--alternate-target").unwrap_or("none") {
            "none" => AlternateTarget::None,
            "any" => AlternateTarget::Any,
            "same-source" => AlternateTarget::SameSource,
            value => return Err(format!("unsupported alternate-target policy `{value}`")),
        };
        let budget_proof = optional(args, "--budget-proof").map(PathBuf::from);
        let loaded_budget = budget_proof
            .as_deref()
            .map(LoadedBudgetContract::load)
            .transpose()?;
        let profile = Self {
            gate: required(args, "--gate")?.to_owned(),
            id: required(args, "--profile")?.to_owned(),
            digest: required(args, "--profile-digest")?.to_owned(),
            harness,
            example: optional(args, "--example").map(str::to_owned),
            visible_mode,
            visible_samples: parse_usize(args, "--visible-samples", 0)?,
            alternate_target,
            selection_samples: parse_usize(args, "--selection-samples", 0)?,
            scroll_samples: parse_usize(args, "--scroll-samples", 0)?,
            switch_samples: parse_usize(args, "--switch-samples", 0)?,
            scenario_proof: optional(args, "--scenario-proof").map(PathBuf::from),
            require_semantic_scenario: parse_bool(args, "--require-semantic-scenario", false)?,
            budget_proof,
            loaded_budget,
            required_budget_metrics: parse_csv(args, "--required-budget-metrics")?,
            required_async_lanes: parse_async_lanes(args)?,
            profile_benchmark_steps: parse_csv(args, "--profile-benchmark-steps")?,
            state_root_policy: optional(args, "--state-root-policy").map(str::to_owned),
            restart_required: parse_bool(args, "--restart-required", false)?,
            required_checkpoints: repeated(args, "--required-checkpoint")
                .into_iter()
                .map(|value| {
                    serde_json::from_str(value)
                        .map_err(|error| format!("invalid --required-checkpoint JSON: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            required_native_workflow_steps: parse_csv(args, "--required-native-workflow-steps")?,
            required_native_workflow_proof_steps: parse_csv(
                args,
                "--required-native-workflow-proof-steps",
            )?
            .into_iter()
            .collect(),
            native_workflow_delivery: optional(args, "--native-workflow-delivery")
                .map(str::to_owned),
            native_workflow_scenario_boundary: optional(
                args,
                "--native-workflow-scenario-boundary",
            )
            .map(str::to_owned),
            native_workflow_capture_method: optional(args, "--native-workflow-capture-method")
                .map(str::to_owned),
            native_workflow_durability: optional(args, "--native-workflow-durability")
                .map(str::to_owned),
        };
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), String> {
        if self.gate.is_empty() || self.id.is_empty() {
            return Err("verifier gate and profile identities must not be empty".to_owned());
        }
        if self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("verifier profile digest must be lowercase SHA-256".to_owned());
        }
        match self.harness {
            HarnessKind::Negative => {
                if self.example.is_some()
                    || self.visible_samples != 0
                    || self.selection_samples != 0
                    || self.scroll_samples != 0
                    || self.switch_samples != 0
                {
                    return Err("negative harness cannot carry native product sampling".to_owned());
                }
            }
            HarnessKind::Timed => {
                if self.example.as_deref().is_none_or(str::is_empty) {
                    return Err("timed harness requires a non-empty --example".to_owned());
                }
                if !(70..=256).contains(&self.visible_samples) {
                    return Err("timed harness visible samples must be within 70..=256".to_owned());
                }
                if self.visible_mode == VisibleSampleMode::Click
                    && self.alternate_target != AlternateTarget::None
                {
                    return Err("click sampling cannot use an alternate hover target".to_owned());
                }
                if self.visible_mode == VisibleSampleMode::Hover
                    && self.alternate_target == AlternateTarget::None
                {
                    return Err("hover sampling requires an alternate target".to_owned());
                }
                validate_optional_samples("selection", self.selection_samples, 24, 128)?;
                validate_optional_samples("scroll", self.scroll_samples, 140, 256)?;
                validate_optional_samples("switch", self.switch_samples, 23, 64)?;
                if self.selection_samples > 0 && self.alternate_target == AlternateTarget::None {
                    return Err("selection sampling requires an alternate target".to_owned());
                }
            }
        }
        if self.require_semantic_scenario && self.scenario_proof.is_none() {
            return Err("semantic scenario proof requires --scenario-proof".to_owned());
        }
        if !self.required_budget_metrics.is_empty() && self.budget_proof.is_none() {
            return Err("required budget metrics require --budget-proof".to_owned());
        }
        if self.required_async_lanes.len() > 16 {
            return Err("required async lane count exceeds 16".to_owned());
        }
        if self.budget_proof.is_some() != self.loaded_budget.is_some() {
            return Err("budget proof did not load its typed contract".to_owned());
        }
        if self.required_budget_metrics.is_empty() {
            if !self.profile_benchmark_steps.is_empty() {
                return Err("profile benchmark steps require budget metrics".to_owned());
            }
        } else if self.profile_benchmark_steps.len() != 2
            || self
                .profile_benchmark_steps
                .iter()
                .any(|step| !safe_evidence_id(step))
        {
            return Err(
                "budgeted profile requires exactly two bounded --profile-benchmark-steps values"
                    .to_owned(),
            );
        }
        if let Some(loaded) = &self.loaded_budget {
            for metric in &self.required_budget_metrics {
                let limit = loaded.contract.limit(metric)?;
                if let Some(expected) = observed_budget_unit(metric)
                    && limit.unit != expected
                {
                    return Err(format!(
                        "budget metric `{metric}` has unit {}, expected {}",
                        budget_unit_name(limit.unit),
                        budget_unit_name(expected),
                    ));
                }
            }
        }
        if self.restart_required && self.state_root_policy.is_none() {
            return Err("restart proof requires --state-root-policy".to_owned());
        }
        if self
            .state_root_policy
            .as_deref()
            .is_some_and(|policy| policy != "launch-scoped-clean")
        {
            return Err("unsupported state-root proof policy".to_owned());
        }
        if self.required_checkpoints.len() > 32 {
            return Err("required checkpoint count exceeds 32".to_owned());
        }
        if self.required_native_workflow_steps.len() > 32
            || self
                .required_native_workflow_steps
                .iter()
                .any(|step| !safe_evidence_id(step))
            || self
                .required_native_workflow_steps
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.required_native_workflow_steps.len()
            || self
                .required_native_workflow_proof_steps
                .iter()
                .any(|step| {
                    !self
                        .required_native_workflow_steps
                        .iter()
                        .any(|required| required == step)
                })
            || (self.required_native_workflow_steps.is_empty()
                != self.required_native_workflow_proof_steps.is_empty())
            || (!self.required_native_workflow_steps.is_empty() && self.scenario_proof.is_none())
            || (!self.required_native_workflow_steps.is_empty()
                && (self.native_workflow_delivery.as_deref()
                    != Some("kernel-uinput-isolated-seat")
                    || self.native_workflow_scenario_boundary.as_deref()
                        != Some("kernel-uinput-and-semantic-assertions")
                    || self.native_workflow_capture_method.as_deref()
                        != Some("app-owned-render-target-readback")
                    || self.native_workflow_durability.as_deref()
                        != Some("state-changing-steps-acked")))
            || (self.required_native_workflow_steps.is_empty()
                && (self.native_workflow_delivery.is_some()
                    || self.native_workflow_scenario_boundary.is_some()
                    || self.native_workflow_capture_method.is_some()
                    || self.native_workflow_durability.is_some()))
        {
            return Err(
                "native workflow steps must be unique, scenario-backed, and carry a non-empty proof subset"
                    .to_owned(),
            );
        }
        let mut checkpoint_ids = BTreeSet::new();
        for checkpoint in &self.required_checkpoints {
            if !safe_evidence_id(&checkpoint.id) || !checkpoint_ids.insert(checkpoint.id.as_str()) {
                return Err(format!(
                    "invalid or duplicate required checkpoint `{}`",
                    checkpoint.id
                ));
            }
            match &checkpoint.evidence {
                VerifierCheckpointRequirementKind::ScenarioStep { scenario_step }
                    if !safe_evidence_id(scenario_step) =>
                {
                    return Err(format!(
                        "checkpoint {} has an invalid scenario step",
                        checkpoint.id
                    ));
                }
                VerifierCheckpointRequirementKind::ResponsiveLayout { logical_width, .. }
                    if !(240..=1_920).contains(logical_width) =>
                {
                    return Err(format!(
                        "checkpoint {} has an unsupported responsive width",
                        checkpoint.id
                    ));
                }
                VerifierCheckpointRequirementKind::NativeWorkflowStep { scenario_step }
                    if !self
                        .required_native_workflow_proof_steps
                        .contains(scenario_step) =>
                {
                    return Err(format!(
                        "checkpoint {} references an undeclared native workflow proof step",
                        checkpoint.id
                    ));
                }
                _ => {}
            }
        }
        for checkpoint in &self.required_checkpoints {
            if let VerifierCheckpointRequirementKind::RestartRestore {
                baseline_checkpoint,
            } = &checkpoint.evidence
            {
                let valid_baseline = self.required_checkpoints.iter().any(|candidate| {
                    candidate.id == *baseline_checkpoint
                        && matches!(
                            &candidate.evidence,
                            VerifierCheckpointRequirementKind::ScenarioStep { .. }
                                | VerifierCheckpointRequirementKind::NativeWorkflowStep { .. }
                        )
                });
                if !valid_baseline {
                    return Err(format!(
                        "restart checkpoint {} must reference a declared scenario checkpoint",
                        checkpoint.id
                    ));
                }
            }
        }
        Ok(())
    }

    fn is_timed(&self) -> bool {
        self.harness == HarnessKind::Timed
    }

    fn example(&self) -> &str {
        self.example
            .as_deref()
            .expect("validated timed profile has an example")
    }

    fn scenario_checkpoint_steps(&self) -> Vec<&str> {
        self.required_checkpoints
            .iter()
            .filter_map(|checkpoint| match &checkpoint.evidence {
                VerifierCheckpointRequirementKind::ScenarioStep { scenario_step } => {
                    Some(scenario_step.as_str())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn requires_persistence_exercise(&self) -> bool {
        self.required_checkpoints.iter().any(|checkpoint| {
            matches!(
                checkpoint.evidence,
                VerifierCheckpointRequirementKind::PersistenceOperation { .. }
            )
        })
    }

    fn requires_migration_exercise(&self) -> bool {
        self.required_checkpoints.iter().any(|checkpoint| {
            matches!(
                checkpoint.evidence,
                VerifierCheckpointRequirementKind::PersistenceOperation {
                    operation: VerifierPersistenceOperation::MigrationActivated
                }
            )
        })
    }
}

fn safe_evidence_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_optional_samples(
    label: &str,
    value: usize,
    minimum: usize,
    maximum: usize,
) -> Result<(), String> {
    if value != 0 && !(minimum..=maximum).contains(&value) {
        return Err(format!(
            "{label} samples must be zero or within {minimum}..={maximum}"
        ));
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    let profile = VerifierProfile::parse(args)?;
    let output = PathBuf::from(required(args, "--evidence-output")?);
    let artifact_dir = PathBuf::from(required(args, "--artifact-dir")?);
    let run_id = required(args, "--run-id")?.to_owned();
    let source_digest = required(args, "--source-digest")?.to_owned();
    fs::create_dir_all(&artifact_dir).map_err(|error| {
        format!(
            "create verifier artifact directory {}: {error}",
            artifact_dir.display()
        )
    })?;

    let evidence = if profile.is_timed() {
        run_native_harness(&profile, &run_id, &artifact_dir)
    } else {
        negative_evidence(&profile)
    };
    let envelope = ProducerEnvelope {
        format: FORMAT_VERSION,
        protocol: PROTOCOL,
        gate: profile.gate.clone(),
        run_id,
        source_digest,
        evidence,
    };
    write_envelope(&output, &envelope)
}

fn negative_evidence(profile: &VerifierProfile) -> GateEvidence {
    let mut invalid = vec![0_u8; 4];
    invalid.copy_from_slice(&(u32::MAX).to_le_bytes());
    #[cfg(target_os = "linux")]
    let rejected = read_event(&mut invalid.as_slice()).is_err();
    #[cfg(not(target_os = "linux"))]
    let rejected = true;
    GateEvidence {
        checks: vec![if rejected {
            Check::pass(
                "negative-bounded-observer-frame",
                "the verifier-only binary observer rejects an oversized frame before allocation",
            )
        } else {
            Check::fail(
                "negative-bounded-observer-frame",
                "the verifier-only binary observer accepted an oversized frame",
            )
        }],
        producer: None,
        profile: Some(profile_evidence(profile, None)),
        native: None,
        product_ux_timings: Vec::new(),
        async_proof_timing: None,
        async_lanes: Vec::new(),
        artifacts: Vec::new(),
    }
}

fn run_native_harness(
    profile: &VerifierProfile,
    run_id: &str,
    artifact_dir: &Path,
) -> GateEvidence {
    #[cfg(target_os = "linux")]
    {
        run_linux_harness(profile, run_id, artifact_dir)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (run_id, artifact_dir);
        GateEvidence::failed(
            profile,
            Check::fail(
                "native-os-input-harness",
                "the kernel virtual-input harness is available only on Linux",
            ),
        )
    }
}

#[cfg(target_os = "linux")]
fn run_linux_harness(profile: &VerifierProfile, run_id: &str, artifact_dir: &Path) -> GateEvidence {
    let mut capture = Capture::default();
    capture.checks.push(product_scheduler_check());
    let workspace = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            capture.checks.push(Check::fail(
                "workspace-discovery",
                format!("cannot read verifier working directory: {error}"),
            ));
            return capture.into_evidence(profile);
        }
    };
    let scratch = match ScratchDir::create(run_id, &profile.gate) {
        Ok(scratch) => scratch,
        Err(error) => {
            capture
                .checks
                .push(Check::fail("native-os-input-scratch", error));
            return capture.into_evidence(profile);
        }
    };
    let state_root = scratch.path.join("state");
    let state_root_clean_at_start = !state_root.exists();

    let observer_path = scratch.path.join("observer.sock");
    let mut observer = match ObserverServer::bind(&observer_path) {
        Ok(server) => server,
        Err(error) => {
            capture
                .checks
                .push(Check::fail("verifier-observer-bind", error));
            return capture.into_evidence(profile);
        }
    };
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            capture.checks.push(Check::fail(
                "native-producer-executable",
                format!("cannot resolve native producer executable: {error}"),
            ));
            return capture.into_evidence(profile);
        }
    };
    let mut session = match NativeSession::start(
        &workspace,
        &scratch.path,
        &executable,
        profile.example(),
        &observer_path,
        &artifact_dir.join("primary"),
        profile
            .state_root_policy
            .as_ref()
            .map(|_| state_root.as_path()),
        profile,
        NativeSessionPhase::Primary,
    ) {
        Ok(session) => session,
        Err(error) => {
            capture
                .checks
                .push(Check::fail("native-os-input-session", error));
            return capture.into_evidence(profile);
        }
    };
    capture.checks.push(Check::pass(
        "kernel-virtual-input",
        format!(
            "uinput pointer and keyboard are owned by launch-scoped seat {}",
            session.isolated_seat_name
        ),
    ));
    capture.state_root = profile
        .state_root_policy
        .as_ref()
        .map(|_| CapturedStateRoot {
            path: state_root.clone(),
            clean_at_start: state_root_clean_at_start,
            restart_count: 0,
            restored_after_restart: false,
        });
    capture.checks.push(Check::pass(
        "regular-cosmic-wayland",
        "preview and dev use ordinary Wayland/app_window callbacks on a launch-scoped COSMIC seat",
    ));

    let mut roles = match session.wait_for_roles(ROLE_READY_TIMEOUT) {
        Ok(roles) => {
            capture.checks.push(Check::pass(
                "native-role-processes",
                format!(
                    "desktop pid {}, preview pid {}, dev pid {} are distinct live processes",
                    session.desktop_id(),
                    roles.preview,
                    roles.dev
                ),
            ));
            roles
        }
        Err(error) => {
            capture
                .checks
                .push(Check::fail("native-role-processes", error));
            capture.checks.push(cleanup_check(session.shutdown()));
            return capture.into_evidence(profile);
        }
    };

    if let Err(error) = session.prepare_background_workspace(&executable) {
        capture
            .checks
            .push(Check::fail("isolated-cosmic-workspace", error));
        capture.checks.push(cleanup_check(session.shutdown()));
        return capture.into_evidence(profile);
    }
    capture.checks.push(Check::pass(
        "isolated-cosmic-workspace",
        "the bounded test workspace remained inactive while launch-scoped input targeted it",
    ));
    match session.launch_isolation_evidence() {
        Ok(evidence) => capture.launch_isolation.push(evidence),
        Err(error) => capture
            .checks
            .push(Check::fail("structured-launch-isolation", error)),
    }

    let exercise = exercise_native_roles(
        profile,
        &mut session,
        &mut observer,
        &mut capture.events,
        &mut capture.samples,
    );
    let exercise_succeeded = match exercise {
        Ok(()) => {
            capture.checks.push(Check::pass(
                "real-native-scenario",
                "kernel virtual devices clicked dev TEST, exercised preview input, and completed bounded samples through COSMIC",
            ));
            true
        }
        Err(error) => {
            capture
                .checks
                .push(Check::fail("real-native-scenario", error));
            false
        }
    };

    drain_events(
        &mut observer,
        &mut capture.events,
        Duration::from_millis(300),
    );
    if process_exists(session.desktop_id())
        && process_exists(roles.preview)
        && process_exists(roles.dev)
    {
        capture.checks.push(Check::pass(
            "native-role-liveness-after-input",
            "desktop, preview, and dev remained live after the real Wayland input sequence",
        ));
    } else {
        capture.checks.push(Check::fail(
            "native-role-liveness-after-input",
            format!(
                "a native role exited after input; desktop={}, preview={}, dev={}",
                process_exists(session.desktop_id()),
                process_exists(roles.preview),
                process_exists(roles.dev)
            ),
        ));
    }
    if exercise_succeeded && profile.restart_required {
        let baseline = restart_baseline(profile, &capture.events);
        let primary_shutdown = session.shutdown();
        match (baseline, primary_shutdown) {
            (Ok(baseline), Ok(())) => match run_restart_phase(
                &workspace,
                &scratch.path,
                &executable,
                profile,
                &observer_path,
                &artifact_dir.join("restart"),
                &state_root,
                &mut observer,
                &mut capture.events,
                &baseline,
            ) {
                Ok((restart_session, restart_roles)) => {
                    session = restart_session;
                    roles = restart_roles;
                    match session.launch_isolation_evidence() {
                        Ok(evidence) => capture.launch_isolation.push(evidence),
                        Err(error) => capture
                            .checks
                            .push(Check::fail("structured-restart-isolation", error)),
                    }
                    if let Some(state_root) = capture.state_root.as_mut() {
                        state_root.restart_count = state_root.restart_count.saturating_add(1);
                        state_root.restored_after_restart = true;
                    }
                    capture.checks.push(Check::pass(
                        "native-restart-restore",
                        "a second native process restored the exact acknowledged authority before its first product frame",
                    ));
                }
                Err(error) => capture
                    .checks
                    .push(Check::fail("native-restart-restore", error)),
            },
            (Err(error), _) => capture
                .checks
                .push(Check::fail("native-restart-restore", error)),
            (_, Err(error)) => capture.checks.push(Check::fail(
                "native-restart-restore",
                format!("primary native process did not shut down cleanly: {error}"),
            )),
        }
    }
    capture.checks.push(cleanup_check(session.shutdown()));
    drain_events(
        &mut observer,
        &mut capture.events,
        Duration::from_millis(100),
    );
    capture.finalize_checks(profile, roles);
    capture.into_evidence(profile)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
struct RestartBaseline {
    checkpoint_id: String,
    source_revision: u64,
    runtime_sequence: u64,
    durable_epoch: u64,
    durable_turn_sequence: u64,
    state_digest: String,
    frame: FrameEvidenceKey,
}

#[cfg(target_os = "linux")]
fn restart_baseline(
    profile: &VerifierProfile,
    events: &[ObserverEvent],
) -> Result<RestartBaseline, String> {
    let baseline_checkpoint = profile
        .required_checkpoints
        .iter()
        .find_map(|checkpoint| match &checkpoint.evidence {
            VerifierCheckpointRequirementKind::RestartRestore {
                baseline_checkpoint,
            } => Some(baseline_checkpoint.as_str()),
            _ => None,
        })
        .ok_or("restart profile has no restart-restore checkpoint")?;
    let baseline = profile
        .required_checkpoints
        .iter()
        .find(|checkpoint| checkpoint.id == baseline_checkpoint)
        .ok_or_else(|| {
            format!("restart baseline checkpoint `{baseline_checkpoint}` is undeclared")
        })?;
    let baseline = events
        .iter()
        .rev()
        .find_map(|event| match (&baseline.evidence, event) {
            (
                VerifierCheckpointRequirementKind::ScenarioStep { scenario_step },
                ObserverEvent::ScenarioCheckpoint {
                    step_id,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    state_digest,
                    key,
                    ..
                },
            ) if step_id == scenario_step && *durable_turn_sequence > 0 => Some(RestartBaseline {
                checkpoint_id: baseline_checkpoint.to_owned(),
                source_revision: *source_revision,
                runtime_sequence: *runtime_sequence,
                durable_epoch: *durable_epoch,
                durable_turn_sequence: *durable_turn_sequence,
                state_digest: state_digest.clone(),
                frame: key.clone(),
            }),
            (
                VerifierCheckpointRequirementKind::NativeWorkflowStep { scenario_step },
                ObserverEvent::NativeWorkflowStep {
                    step_id,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    durable_acked: true,
                    state_digest,
                    key,
                    ..
                },
            ) if step_id == scenario_step && *durable_turn_sequence > 0 => Some(RestartBaseline {
                checkpoint_id: baseline_checkpoint.to_owned(),
                source_revision: *source_revision,
                runtime_sequence: *runtime_sequence,
                durable_epoch: *durable_epoch,
                durable_turn_sequence: *durable_turn_sequence,
                state_digest: state_digest.clone(),
                frame: key.clone(),
            }),
            _ => None,
        })
        .ok_or_else(|| {
            format!("restart baseline `{baseline_checkpoint}` has no acknowledged state checkpoint")
        })?;
    if baseline.source_revision == 0
        || baseline.runtime_sequence == 0
        || baseline.durable_epoch == 0
        || !baseline.frame.is_complete()
    {
        return Err(format!(
            "restart baseline `{baseline_checkpoint}` has incomplete revision, durability, or frame identity"
        ));
    }
    Ok(baseline)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn run_restart_phase(
    workspace: &Path,
    runtime_dir: &Path,
    executable: &Path,
    profile: &VerifierProfile,
    observer_path: &Path,
    artifact_dir: &Path,
    state_root: &Path,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    baseline: &RestartBaseline,
) -> Result<(NativeSession, RolePids), String> {
    let start = events.len();
    let mut session = NativeSession::start(
        workspace,
        runtime_dir,
        executable,
        profile.example(),
        observer_path,
        artifact_dir,
        Some(state_root),
        profile,
        NativeSessionPhase::Restart,
    )?;
    let result = (|| {
        let roles = session.wait_for_roles(ROLE_READY_TIMEOUT)?;
        if roles.preview == baseline.frame.process_id
            || session.session_id == baseline.frame.session_id
        {
            return Err(
                "restart reused the primary preview process or native session identity".to_owned(),
            );
        }
        session.prepare_background_workspace(executable)?;
        let mounted = wait_for_value(
            observer,
            events,
            EVENT_TIMEOUT,
            start,
            |event| match event {
                ObserverEvent::StateMounted {
                    disposition,
                    schema_version,
                    schema_hash,
                    migration,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    state_digest,
                    key,
                } => Some(Ok((
                    *disposition,
                    *schema_version,
                    schema_hash.clone(),
                    migration.is_some(),
                    *source_revision,
                    *runtime_sequence,
                    *durable_epoch,
                    *durable_turn_sequence,
                    state_digest.clone(),
                    key.clone(),
                ))),
                ObserverEvent::SourceFailed {
                    revision,
                    stage,
                    message,
                } => Some(Err(format!(
                    "source revision {revision} failed during {stage}: {message}"
                ))),
                _ => None,
            },
        )??;
        if mounted.0 != StartupDisposition::Restored
            || mounted.1 == 0
            || mounted.2.len() != 64
            || mounted.3
            || mounted.4 != baseline.source_revision
            || mounted.5 == 0
            || mounted.6 < baseline.durable_epoch
            || mounted.7 < baseline.durable_turn_sequence
            || mounted.8 != baseline.state_digest
            || !mounted.9.is_complete()
            || !frame_key_matches_session(events, &mounted.9, &session, ObserverRole::Preview)
        {
            return Err(format!(
                "restart mount did not restore the exact durable baseline `{}`: disposition={:?}, schema_version={}, schema_hash={}, migration={}, source_revision={}, runtime_sequence={}, durable_epoch={} (expected >= {}), durable_turn={} (expected >= {}), digest={}, expected={}",
                baseline.checkpoint_id,
                mounted.0,
                mounted.1,
                mounted.2,
                mounted.3,
                mounted.4,
                mounted.5,
                mounted.6,
                baseline.durable_epoch,
                mounted.7,
                baseline.durable_turn_sequence,
                mounted.8,
                baseline.state_digest,
            ));
        }
        let first_preview_frame = events[start..].iter().find_map(|event| match event {
            ObserverEvent::FramePresented(frame) if frame.role == ObserverRole::Preview => {
                Some(frame.key.clone())
            }
            _ => None,
        });
        let source_switch_frame = events[start..].iter().find_map(|event| match event {
            ObserverEvent::SourceSwitchFinal { key, .. }
                if key.process_id == mounted.9.process_id =>
            {
                Some(key.clone())
            }
            _ => None,
        });
        if first_preview_frame.as_ref() != Some(&mounted.9)
            || source_switch_frame.as_ref() != Some(&mounted.9)
        {
            return Err(
                "restart authority was not linked to the first presented preview frame and source-switch commit"
                    .to_owned(),
            );
        }
        if profile.requires_migration_exercise() {
            let migration =
                wait_for_value(
                    observer,
                    events,
                    EVENT_TIMEOUT,
                    start,
                    |event| match event {
                        ObserverEvent::PersistenceEvidence {
                            kind: PersistenceEvidenceKind::MigrationActivated,
                            durable_epoch,
                            durable_turn_sequence,
                            before_state_digest,
                            after_state_digest,
                            key,
                            ..
                        } => Some((
                            *durable_epoch,
                            *durable_turn_sequence,
                            before_state_digest.clone(),
                            after_state_digest.clone(),
                            key.clone(),
                        )),
                        _ => None,
                    },
                )?;
            if migration.0 == 0
                || migration.1 == 0
                || migration.2 != baseline.state_digest
                || migration.3 == migration.2
                || !migration.4.is_complete()
                || !frame_key_matches_session(events, &migration.4, &session, ObserverRole::Preview)
            {
                return Err(
                    "restart process did not activate a durable, frame-linked schema migration"
                        .to_owned(),
                );
            }
        }
        wait_for_evidence_proofs(observer, events)
            .map_err(|error| format!("restart proof lane did not drain: {error}"))?;
        if exact_proof_for_key(events, &mounted.9).is_none() {
            return Err("restart mount has no exact app-owned WGPU proof".to_owned());
        }
        if !process_exists(session.desktop_id())
            || !process_exists(roles.preview)
            || !process_exists(roles.dev)
        {
            return Err("a restarted native role exited before evidence completed".to_owned());
        }
        Ok(roles)
    })();
    match result {
        Ok(roles) => Ok((session, roles)),
        Err(error) => {
            let cleanup = session.shutdown();
            Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; restart cleanup failed: {cleanup}"),
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn product_scheduler_check() -> Check {
    let policy = unsafe { libc::sched_getscheduler(0) };
    let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    check_result(
        "normal-product-scheduler",
        policy == libc::SCHED_OTHER && nice == 0,
        "native evidence producer and product children use SCHED_OTHER at nice 0",
        format!("native evidence producer scheduler policy={policy}, nice={nice}"),
    )
}

#[cfg(target_os = "linux")]
fn exercise_native_roles(
    profile: &VerifierProfile,
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    samples: &mut ProductSamples,
) -> Result<(), String> {
    wait_for_metadata(observer, events)
        .map_err(|error| format!("preview/dev metadata did not become ready: {error}"))?;
    wait_for_value(observer, events, EVENT_TIMEOUT, 0, |event| match event {
        ObserverEvent::SourceSwitchFinal { .. } => Some(Ok(())),
        ObserverEvent::SourceFailed {
            revision,
            stage,
            message,
        } => Some(Err(format!(
            "preview source revision {revision} failed during {stage}: {message}"
        ))),
        _ => None,
    })
    .map_err(|error| format!("initial preview source did not become ready: {error}"))??;
    let mut placements = discover_window_placements(session, observer, events)?;
    let mut dev_placement = activate_window(
        session,
        observer,
        events,
        &mut placements,
        ObserverRole::Dev,
    )?;
    drain_events(observer, events, INPUT_CALIBRATION_QUIET);
    let dev_test_center = observed_role_target(events, ObserverRole::Dev, "dev.test")
        .ok_or("dev TEST target was not published")?;
    let dev_editor_center =
        observed_role_target(events, ObserverRole::Dev, DEV_EDITOR_INPUT_TARGET)
            .ok_or("dev editor target was not published")?;

    let editor_point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Dev,
        DEV_EDITOR_INPUT_TARGET,
        dev_editor_center,
        translated_target_candidates(dev_placement, dev_editor_center.0, dev_editor_center.1),
    )?;
    let dev_input_start = events.len();
    session.run_driver(&[
        "move",
        &editor_point.0.to_string(),
        &editor_point.1.to_string(),
    ])?;
    session.run_driver(&["click", "left"])?;
    session.run_driver(&["key", "down", "left"])?;
    session.run_driver(&["key", "up", "left"])?;
    session.run_driver(&["axis", "vertical", "4"])?;
    session.run_driver(&["axis", "vertical", "-4"])?;
    wait_for_event(observer, events, EVENT_TIMEOUT, dev_input_start, |event| {
        matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Keyboard)
    })
    .map_err(|error| format!("dev editor did not accept real keyboard input: {error}"))?;
    wait_for_event(observer, events, EVENT_TIMEOUT, dev_input_start, |event| {
        matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Wheel)
    })
    .map_err(|error| format!("dev editor did not accept real wheel input: {error}"))?;

    let test_point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Dev,
        "dev.test",
        dev_test_center,
        translated_target_candidates(dev_placement, dev_test_center.0, dev_test_center.1),
    )
    .map_err(|error| format!("{error}; calibrated_dev={dev_placement:?}"))?;
    let before_test = events.len();
    session.run_driver(&["move", &test_point.0.to_string(), &test_point.1.to_string()])?;
    session.run_driver(&["click", "left"])?;
    let test_action_visible = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        before_test,
        |event| match event {
            ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.target.as_deref() == Some("dev.test")
                    && input.kind == InputKind::PointerButton
                    && input.pointer_button_pressed == Some(false) =>
            {
                Some(input.visible_change)
            }
            _ => None,
        },
    )
    .map_err(|error| {
        format!(
            "dev TEST click was not accepted: {error}; observed={}",
            input_event_trace(events, before_test, 8)
        )
    })?;
    if !test_action_visible {
        return Err(format!(
            "dev TEST release reached the button but did not activate it; observed={}",
            input_event_trace(events, before_test, 8)
        ));
    }
    let test_target = wait_for_value(observer, events, EVENT_TIMEOUT, before_test, |event| {
        match event {
            ObserverEvent::TestTarget {
                request_id,
                node,
                source_path,
                x,
                y,
            } => Some(Ok((*request_id, node.clone(), source_path.clone(), *x, *y))),
            ObserverEvent::TestCompleted {
                request_id,
                passed: false,
                completed_steps,
                message,
                ..
            } => Some(Err(format!(
                "preview TEST request {request_id} failed after {completed_steps} steps: {message}"
            ))),
            _ => None,
        }
    })
    .map_err(|error| format!("preview did not publish a TEST result: {error}"))??;
    let preview_placement = activate_window(
        session,
        observer,
        events,
        &mut placements,
        ObserverRole::Preview,
    )?;
    if profile.required_native_workflow_steps.is_empty() {
        require_test_completion(observer, events, before_test, test_target.0, None)?;
        drain_events(observer, events, Duration::from_millis(250));
        wait_for_evidence_proofs(observer, events)
            .map_err(|error| format!("checkpoint proof lane did not drain: {error}"))?;
    }

    if !profile.required_budget_metrics.is_empty() {
        drive_product_profile_benchmark(
            profile,
            session,
            observer,
            events,
            preview_placement,
            before_test,
        )?;
        wait_for_evidence_proofs(observer, events)
            .map_err(|error| format!("profile proof lane did not drain: {error}"))?;
    }
    if !profile.required_native_workflow_steps.is_empty() {
        drive_native_workflow(
            profile,
            session,
            observer,
            events,
            preview_placement,
            before_test,
            test_target.0,
        )?;
        require_test_completion(
            observer,
            events,
            before_test,
            test_target.0,
            Some(profile.required_native_workflow_steps.len()),
        )?;
        drain_events(observer, events, Duration::from_millis(250));
        wait_for_evidence_proofs(observer, events)
            .map_err(|error| format!("native workflow proof lane did not drain: {error}"))?;
    }

    let preview_candidates =
        translated_target_candidates(preview_placement, test_target.3, test_target.4);
    let preview_point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Preview,
        &test_target.1,
        (test_target.3, test_target.4),
        preview_candidates,
    )?;
    let off_target = if profile.visible_mode == VisibleSampleMode::Click {
        None
    } else {
        Some(locate_different_preview_target(
            session,
            observer,
            events,
            &test_target.1,
            (profile.alternate_target == AlternateTarget::SameSource)
                .then_some(test_target.2.as_str()),
            preview_point,
            preview_placement.origin,
        )?)
    };
    let mut proof_prelude = VecDeque::with_capacity(11);
    let mut driven_ordinal = 0usize;
    loop {
        if driven_ordinal >= 160 {
            return Err(
                "product proof was not requested within 160 bounded interactions".to_owned(),
            );
        }
        let sequence = drive_profile_visible_sample(
            profile,
            session,
            observer,
            events,
            preview_point,
            &test_target.1,
            off_target.as_ref(),
            driven_ordinal,
        )
        .map_err(|error| format!("preview proof prelude {driven_ordinal} failed: {error}"))?;
        driven_ordinal += 1;
        if proof_prelude.len() == 11 {
            proof_prelude.pop_front();
        }
        proof_prelude.push_back(sequence);
        drain_events(observer, events, Duration::from_millis(1));
        let Some(key) = presented_key_for_sequence(events, sequence) else {
            return Err(format!(
                "preview proof prelude sequence {sequence} has no presented frame"
            ));
        };
        if events.iter().any(
            |event| matches!(event, ObserverEvent::ProofRequested { key: requested, .. } if requested == &key),
        ) {
            if proof_prelude.len() < 11 {
                return Err(
                    "product proof was consumed before eleven warm interactions were available"
                        .to_owned(),
                );
            }
            wait_for_exact_proof(observer, events, EVENT_TIMEOUT, &key)
                .map_err(|error| format!("product-frame proof did not complete: {error}"))?;
            break;
        }
    }
    samples.visible.extend(proof_prelude);
    for sample_ordinal in 11..profile.visible_samples {
        let sequence = drive_profile_visible_sample(
            profile,
            session,
            observer,
            events,
            preview_point,
            &test_target.1,
            off_target.as_ref(),
            driven_ordinal,
        )
        .map_err(|error| format!("preview sample {sample_ordinal} failed: {error}"))?;
        driven_ordinal += 1;
        samples.visible.insert(sequence);
    }
    if samples.visible.len() < profile.visible_samples {
        return Err("serialized preview interaction samples were not retained".to_owned());
    }

    if profile.selection_samples > 0 {
        let alternate = off_target.as_ref().expect("selection alternate target");
        for ordinal in 0..profile.selection_samples {
            let (point, node) = if ordinal % 2 == 0 {
                (preview_point, test_target.1.as_str())
            } else {
                (
                    alternate.0,
                    alternate
                        .1
                        .as_deref()
                        .ok_or("selection alternate has no observed target")?,
                )
            };
            let sequence = drive_click_sample(session, observer, events, point, node)
                .map_err(|error| format!("selection sample {ordinal} failed: {error}"))?;
            samples.visible.insert(sequence);
            samples.clicks.insert(sequence);
        }
    }

    if profile.scroll_samples > 0 {
        let discovery_start = events.len();
        let mut candidates = Vec::new();
        if let Some(alternate) = off_target.as_ref() {
            candidates.push(alternate.0);
        }
        candidates.extend([
            preview_point,
            (
                preview_placement.origin.0 + 24,
                preview_placement.origin.1 + 120,
            ),
            (
                preview_placement.origin.0 + 760,
                preview_placement.origin.1 + 120,
            ),
        ]);
        candidates.sort_unstable();
        candidates.dedup();
        let mut selected = None;
        for point in candidates {
            let move_start = events.len();
            session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
            if wait_for_event(
                observer,
                events,
                Duration::from_secs(2),
                move_start,
                |event| {
                    matches!(event, ObserverEvent::InputAccepted(input)
                        if input.role == ObserverRole::Preview
                            && input.real_os
                            && input.kind == InputKind::PointerMove)
                },
            )
            .is_err()
            {
                continue;
            }
            for amount in [4_i32, -4_i32] {
                if let Some(sequence) = drive_wheel_attempt(session, observer, events, amount)? {
                    selected = Some((point, -amount, sequence));
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let (_scroll_point, mut amount, first_sequence) = selected.ok_or_else(|| {
            format!(
                "no real preview point exposed visible vertical scrolling; observed={}",
                input_event_trace(events, discovery_start, 16)
            )
        })?;
        samples.scroll.insert(first_sequence);
        for ordinal in 1..profile.scroll_samples {
            let sample_start = events.len();
            let mut visible_sequence = None;
            for _ in 0..2 {
                if let Some(sequence) = drive_wheel_attempt(session, observer, events, amount)? {
                    visible_sequence = Some(sequence);
                    break;
                }
                amount = -amount;
            }
            let sequence = visible_sequence.ok_or_else(|| {
                format!(
                    "scroll sample {ordinal} changed no visible scroll state in either direction; observed={}",
                    input_event_trace(events, sample_start, 8)
                )
            })?;
            samples.scroll.insert(sequence);
            amount = -amount;
        }
        if samples.scroll.len() < profile.scroll_samples {
            return Err("serialized preview scroll samples were not retained".to_owned());
        }
    }

    if profile.required_checkpoints.iter().any(|checkpoint| {
        matches!(
            checkpoint.evidence,
            VerifierCheckpointRequirementKind::ResponsiveLayout { .. }
        )
    }) {
        drive_responsive_resize(session, observer, events, &mut placements, before_test)?;
        wait_for_evidence_proofs(observer, events)
            .map_err(|error| format!("responsive proof lane did not drain: {error}"))?;
    }

    if profile.switch_samples > 0 {
        let mut revision = maximum_switch_revision(events);
        for ordinal in 0..profile.switch_samples {
            session.reconcile_background_layout()?;
            dev_placement = activate_window(
                session,
                observer,
                events,
                &mut placements,
                ObserverRole::Dev,
            )?;
            let target = if ordinal % 2 == 0 {
                "dev.next"
            } else {
                "dev.previous"
            };
            let center = observed_role_target(events, ObserverRole::Dev, target)
                .ok_or_else(|| format!("{target} retained hit center was not observed"))?;
            locate_target(
                session,
                observer,
                events,
                ObserverRole::Dev,
                target,
                center,
                translated_target_candidates(dev_placement, center.0, center.1),
            )?;
            let start = events.len();
            session.run_driver(&["click", "left"])?;
            revision = wait_for_value(
                observer,
                events,
                EVENT_TIMEOUT,
                start,
                |event| match event {
                    ObserverEvent::SourceSwitchFinal { revision: next, .. } if *next > revision => {
                        Some(*next)
                    }
                    _ => None,
                },
            )
            .map_err(|error| {
                format!(
                    "source switch {ordinal} did not finish: {error}; observed={}",
                    input_event_trace(events, start, 12)
                )
            })?;
        }
    }

    wait_for_event(observer, events, EVENT_TIMEOUT, 0, |event| {
        matches!(
            event,
            ObserverEvent::ProofCompleted {
                artifact: Some(_),
                error: None,
                ..
            }
        )
    })
    .map_err(|error| format!("final app-owned proof did not complete: {error}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn drive_product_profile_benchmark(
    profile: &VerifierProfile,
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    preview_placement: WindowPlacement,
    start: usize,
) -> Result<(), String> {
    let target = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::ProfileInputTarget {
                node,
                source_path,
                x,
                y,
                sample_count,
                key,
            } => Some((
                node.clone(),
                source_path.clone(),
                *x,
                *y,
                *sample_count,
                key.clone(),
            )),
            _ => None,
        },
    )
    .map_err(|error| format!("profile input target was not published: {error}"))?;
    if target.4 != 120
        || !frame_key_matches_session(events, &target.5, session, ObserverRole::Preview)
    {
        return Err(
            "profile target did not carry the declared count and exact primary preview identity"
                .to_owned(),
        );
    }
    let seed = profile_seed_text(profile, &target.1)?;
    let point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Preview,
        &target.0,
        (target.2, target.3),
        translated_target_candidates(preview_placement, target.2, target.3),
    )?;
    session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
    session.run_driver(&["click", "left"])?;
    session.run_driver(&["chord", "ctrl", "a"])?;

    let seed_start = events.len();
    session.run_driver(&["text", &seed])?;
    let seeded = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        seed_start,
        |event| match event {
            ObserverEvent::ProfileInputSeeded {
                input_sequence,
                callback_to_host_ns,
                compile_us,
                pending_child_artifacts,
                editor_key,
                key,
            } => Some((
                *input_sequence,
                *callback_to_host_ns,
                *compile_us,
                *pending_child_artifacts,
                editor_key.clone(),
                key.clone(),
            )),
            _ => None,
        },
    )
    .map_err(|error| format!("profile seed edit did not complete: {error}"))?;
    require_exact_profile_frame_chain(events, session, seeded.0, seeded.1, &seeded.4, &seeded.5)?;

    for expected_ordinal in 1..=120_u32 {
        let sample_start = events.len();
        session.run_driver(&["text", " "])?;
        let sample =
            wait_for_value(
                observer,
                events,
                EVENT_TIMEOUT,
                sample_start,
                |event| match event {
                    ObserverEvent::ProfileSample {
                        ordinal,
                        input_sequence,
                        callback_to_host_ns,
                        editor_key,
                        key,
                        ..
                    } if *ordinal == expected_ordinal => Some((
                        *input_sequence,
                        *callback_to_host_ns,
                        editor_key.clone(),
                        key.clone(),
                    )),
                    _ => None,
                },
            )
            .map_err(|error| {
                format!("profile text sample {expected_ordinal} did not complete: {error}")
            })?;
        require_exact_profile_frame_chain(
            events, session, sample.0, sample.1, &sample.2, &sample.3,
        )?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn profile_seed_text(profile: &VerifierProfile, source_path: &str) -> Result<String, String> {
    let scenario_path = profile
        .scenario_proof
        .as_deref()
        .ok_or("profile benchmark has no scenario proof")?;
    let scenario = boon_runtime::parse_scenario(&resolve_profile_input(scenario_path))
        .map_err(|error| format!("load profile benchmark scenario: {error}"))?;
    let steps = profile
        .profile_benchmark_steps
        .iter()
        .map(|id| {
            scenario
                .steps
                .iter()
                .find(|step| step.id == *id)
                .ok_or_else(|| format!("profile benchmark step `{id}` is absent"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if steps.len() != 2
        || steps.iter().any(|step| {
            step.user_action_kind.as_deref() != Some("type_text")
                || step
                    .source_event
                    .as_ref()
                    .is_none_or(|event| event.source != source_path)
        })
    {
        return Err(
            "profile benchmark steps do not identify two text edits for the published source"
                .to_owned(),
        );
    }
    let seed = steps[0]
        .user_action_text
        .clone()
        .ok_or("profile benchmark seed has no text")?;
    if seed.is_empty()
        || seed.len() > crate::native_input::ASCII_TEXT_BATCH_MAX_BYTES
        || !seed.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    {
        return Err("profile benchmark seed is not bounded printable ASCII".to_owned());
    }
    Ok(seed)
}

#[cfg(target_os = "linux")]
fn require_test_completion(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    start: usize,
    request_id: u64,
    expected_steps: Option<usize>,
) -> Result<(), String> {
    let completion = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::TestCompleted {
                request_id: observed_request,
                passed,
                semantic_assertions_proven,
                completed_steps,
                message,
            } if *observed_request == request_id => Some((
                *passed,
                *semantic_assertions_proven,
                *completed_steps,
                message.clone(),
            )),
            _ => None,
        },
    )
    .map_err(|error| format!("preview TEST did not complete: {error}"))?;
    if !completion.0 {
        return Err(format!(
            "preview TEST failed after {} steps: {}",
            completion.2, completion.3
        ));
    }
    if !completion.1 {
        return Err(format!(
            "preview TEST completed {} steps without proving semantic assertions: {}",
            completion.2, completion.3
        ));
    }
    if let Some(expected) = expected_steps
        && completion.2 as usize != expected
    {
        return Err(format!(
            "preview TEST completed {} steps, expected {expected}",
            completion.2
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct NativeWorkflowAction {
    id: String,
    source_path: String,
    action_kind: String,
    action_digest: String,
    text: Option<String>,
    key: Option<String>,
}

#[cfg(target_os = "linux")]
fn native_workflow_request_id(test_request_id: u64, ordinal: usize) -> u64 {
    test_request_id
        .saturating_mul(64)
        .saturating_add(ordinal.try_into().unwrap_or(u64::MAX))
        .max(1)
}

#[cfg(target_os = "linux")]
fn declared_native_workflow(
    profile: &VerifierProfile,
) -> Result<Vec<NativeWorkflowAction>, String> {
    let scenario_path = profile
        .scenario_proof
        .as_deref()
        .ok_or("native workflow has no scenario proof")?;
    let scenario_path = resolve_profile_input(scenario_path);
    let scenario = crate::catalog::ordinary_test_steps(&scenario_path.to_string_lossy())
        .map_err(|error| format!("load native workflow scenario: {error}"))?;
    profile
        .required_native_workflow_steps
        .iter()
        .map(|id| {
            let step = scenario
                .iter()
                .find(|step| step.id == *id)
                .ok_or_else(|| format!("native workflow step `{id}` is absent"))?;
            if step.expectations.is_empty() {
                return Err(format!(
                    "native workflow step `{id}` has no semantic assertions"
                ));
            }
            let action_kind = step
                .action_kind
                .clone()
                .unwrap_or_else(|| "assertion_only".to_owned());
            if !matches!(
                action_kind.as_str(),
                "assertion_only" | "click" | "type_text" | "double_click" | "key" | "blur"
            ) {
                return Err(format!(
                    "native workflow step `{id}` has unsupported action `{action_kind}`"
                ));
            }
            if action_kind == "assertion_only" && !step.source_path.is_empty() {
                return Err(format!(
                    "native workflow assertion-only step `{id}` unexpectedly has a source"
                ));
            }
            if action_kind == "type_text" {
                let text = step
                    .text
                    .as_deref()
                    .ok_or_else(|| format!("native workflow step `{id}` has no text"))?;
                if text.is_empty()
                    || text.len() > crate::native_input::ASCII_TEXT_BATCH_MAX_BYTES
                    || !text.bytes().all(|byte| (b' '..=b'~').contains(&byte))
                {
                    return Err(format!(
                        "native workflow step `{id}` text is not bounded printable ASCII"
                    ));
                }
            }
            Ok(NativeWorkflowAction {
                id: id.clone(),
                source_path: if action_kind == "assertion_only" {
                    "assertion-only".to_owned()
                } else {
                    step.source_path.clone()
                },
                action_kind,
                action_digest: crate::preview::native_workflow_action_digest(step),
                text: step.text.clone(),
                key: step.key.clone(),
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn next_native_workflow_event_cursor(
    events: &[ObserverEvent],
    start: usize,
    expected_request_id: u64,
    expected_ordinal: u32,
    expected_step_id: &str,
) -> Result<usize, String> {
    events
        .get(start..)
        .and_then(|events| {
            events.iter().position(|event| {
                matches!(
                    event,
                    ObserverEvent::NativeWorkflowStep {
                        request_id,
                        ordinal,
                        step_id,
                        ..
                    } if *request_id == expected_request_id
                        && *ordinal == expected_ordinal
                        && step_id == expected_step_id
                )
            })
        })
        .map(|offset| start.saturating_add(offset).saturating_add(1))
        .ok_or_else(|| {
            format!("native workflow step `{expected_step_id}` has no matching observer event")
        })
}

#[cfg(target_os = "linux")]
fn drive_native_workflow(
    profile: &VerifierProfile,
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    preview_placement: WindowPlacement,
    start: usize,
    test_request_id: u64,
) -> Result<(), String> {
    let actions = declared_native_workflow(profile)?;
    let ready = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::NativeWorkflowReady {
                test_request_id,
                step_count,
                source_revision,
                runtime_sequence,
                durable_epoch,
                state_digest,
                key,
            } => Some((
                *test_request_id,
                *step_count,
                *source_revision,
                *runtime_sequence,
                *durable_epoch,
                state_digest.clone(),
                key.clone(),
            )),
            _ => None,
        },
    )
    .map_err(|error| format!("native workflow did not become ready: {error}"))?;
    if ready.0 != test_request_id
        || ready.1 as usize != actions.len()
        || ready.2 == 0
        || ready.3 == 0
        || ready.4 == 0
        || ready.5.len() != 64
        || !frame_key_matches_session(events, &ready.6, session, ObserverRole::Preview)
    {
        return Err(
            "native workflow reset evidence is incomplete or belongs to another session".to_owned(),
        );
    }

    let mut previous_frame_id = ready.6.frame_id;
    let mut previous_state_digest = ready.5.clone();
    let mut workflow_event_start = start;
    for (index, action) in actions.iter().enumerate() {
        let ordinal = u32::try_from(index + 1).unwrap_or(u32::MAX);
        let expected_request_id = native_workflow_request_id(test_request_id, index + 1);
        let action_start = workflow_event_start;
        if action.action_kind != "assertion_only" {
            let target = wait_for_native_workflow_target(
                session,
                observer,
                events,
                ordinal,
                expected_request_id,
                action,
            )?;
            if !frame_key_matches_session(events, &target.3, session, ObserverRole::Preview) {
                return Err(format!(
                    "native workflow target `{}` belongs to another preview session",
                    action.id
                ));
            }
            let point = locate_target(
                session,
                observer,
                events,
                ObserverRole::Preview,
                &target.0,
                (target.1, target.2),
                translated_target_candidates(preview_placement, target.1, target.2),
            )?;
            let pointer_start = events.len();
            session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
            wait_for_native_workflow_pointer_phase(
                session,
                observer,
                events,
                pointer_start,
                test_request_id,
                index,
                TestPointerPhase::Move,
            )?;
            wait_for_native_workflow_pointer_phase(
                session,
                observer,
                events,
                pointer_start,
                test_request_id,
                index,
                TestPointerPhase::Hover,
            )?;
            let pointer_cycles = usize::from(action.action_kind == "double_click") + 1;
            for _ in 0..pointer_cycles {
                let down_start = events.len();
                session.run_driver(&["button", "down", "left"])?;
                wait_for_native_workflow_pointer_phase(
                    session,
                    observer,
                    events,
                    down_start,
                    test_request_id,
                    index,
                    TestPointerPhase::Down,
                )?;
                thread::sleep(crate::native_input::POINTER_CLICK_HOLD);
                let up_start = events.len();
                session.run_driver(&["button", "up", "left"])?;
                wait_for_native_workflow_pointer_phase(
                    session,
                    observer,
                    events,
                    up_start,
                    test_request_id,
                    index,
                    TestPointerPhase::Up,
                )?;
            }
            match action.action_kind.as_str() {
                "type_text" => {
                    session.run_driver(&["chord", "ctrl", "a"])?;
                    session.run_driver(&[
                        "text",
                        action
                            .text
                            .as_deref()
                            .expect("validated native workflow text"),
                    ])?;
                }
                "double_click" => {}
                "key" => {
                    let key = action.key.as_deref().ok_or_else(|| {
                        format!("native workflow key step `{}` has no key", action.id)
                    })?;
                    session.run_driver(&["key", "down", key])?;
                    session.run_driver(&["key", "up", key])?;
                }
                "blur" => {
                    session.run_driver(&["key", "down", "tab"])?;
                    session.run_driver(&["key", "up", "tab"])?;
                }
                "click" => {}
                other => return Err(format!("unsupported native workflow action `{other}`")),
            }
        }
        let completed =
            wait_for_value(
                observer,
                events,
                EVENT_TIMEOUT,
                action_start,
                |event| match event {
                    ObserverEvent::NativeWorkflowStep {
                        request_id,
                        ordinal: observed,
                        step_id,
                        source_path,
                        action_kind,
                        action_digest,
                        input_first_sequence,
                        input_last_sequence,
                        input_event_count,
                        input_event_digest,
                        assertion_count,
                        source_revision,
                        runtime_sequence,
                        durable_epoch,
                        durable_turn_sequence,
                        durable_acked,
                        before_state_digest,
                        state_digest,
                        key,
                    } if *observed == ordinal && step_id == &action.id => Some((
                        *request_id,
                        source_path.clone(),
                        action_kind.clone(),
                        action_digest.clone(),
                        *input_first_sequence,
                        *input_last_sequence,
                        *input_event_count,
                        input_event_digest.clone(),
                        *assertion_count,
                        *source_revision,
                        *runtime_sequence,
                        *durable_epoch,
                        *durable_turn_sequence,
                        *durable_acked,
                        before_state_digest.clone(),
                        state_digest.clone(),
                        key.clone(),
                    )),
                    _ => None,
                },
            )
            .map_err(|error| {
                format!(
                    "native workflow step `{}` did not complete: {error}",
                    action.id
                )
            })?;
        workflow_event_start = next_native_workflow_event_cursor(
            events,
            action_start,
            expected_request_id,
            ordinal,
            &action.id,
        )?;
        let mut input_events = events[action_start..]
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::InputAccepted(input)
                    if input.role == ObserverRole::Preview
                        && input.real_os
                        && input.event_sequence >= completed.4
                        && input.event_sequence <= completed.5 =>
                {
                    Some(input)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        input_events.sort_by_key(|input| input.event_sequence);
        let observed_input_digest = crate::preview::native_workflow_input_digest(
            &input_events
                .iter()
                .map(|input| input.event_digest.clone())
                .collect::<Vec<_>>(),
        );
        let assertion_only = action.action_kind == "assertion_only";
        let input_span_valid = if assertion_only {
            completed.4 == 0 && completed.5 == 0 && completed.6 == 0 && input_events.is_empty()
        } else {
            completed.4 > 0
                && completed.5 >= completed.4
                && completed.6 as usize == input_events.len()
                && input_events
                    .first()
                    .is_some_and(|input| input.event_sequence == completed.4)
                && input_events
                    .last()
                    .is_some_and(|input| input.event_sequence == completed.5)
        };
        if completed.0 != expected_request_id
            || completed.1 != action.source_path
            || completed.2 != action.action_kind
            || completed.3 != action.action_digest
            || !input_span_valid
            || completed.7 != observed_input_digest
            || completed.8 == 0
            || completed.9 == 0
            || completed.10 == 0
            || completed.11 == 0
            || completed.12 == 0
            || !completed.13
            || completed.14 != previous_state_digest
            || completed.15.len() != 64
            || completed.16.frame_id <= previous_frame_id
            || !frame_key_matches_session(events, &completed.16, session, ObserverRole::Preview)
        {
            return Err(format!(
                "native workflow step `{}` lacks exact action-span, durable, semantic, or frame evidence",
                action.id
            ));
        }
        previous_frame_id = completed.16.frame_id;
        previous_state_digest.clone_from(&completed.15);
        if profile
            .required_native_workflow_proof_steps
            .contains(&action.id)
        {
            wait_for_exact_proof(observer, events, EVENT_TIMEOUT, &completed.16).map_err(
                |error| {
                    format!(
                        "native workflow proof `{}` did not complete: {error}",
                        action.id
                    )
                },
            )?;
        }
    }
    let completed = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::NativeWorkflowCompleted {
                test_request_id,
                step_count,
                initial_state_digest,
                final_state_digest,
                key,
            } => Some((
                *test_request_id,
                *step_count,
                initial_state_digest.clone(),
                final_state_digest.clone(),
                key.clone(),
            )),
            _ => None,
        },
    )?;
    if completed.0 != test_request_id
        || completed.1 as usize != actions.len()
        || completed.2 != ready.5
        || completed.3 != previous_state_digest
        || completed.4.frame_id != previous_frame_id
        || !frame_key_matches_session(events, &completed.4, session, ObserverRole::Preview)
    {
        return Err("native workflow completion evidence is incomplete".to_owned());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for_native_workflow_pointer_phase(
    session: &NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    start: usize,
    request_id: u64,
    step_index: usize,
    phase: TestPointerPhase,
) -> Result<FrameEvidenceKey, String> {
    let key = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::TestPointerFrame {
                request_id: observed_request,
                step_index: observed_step,
                phase: observed_phase,
                key,
                ..
            } if *observed_request == request_id
                && *observed_step as usize == step_index
                && *observed_phase == phase =>
            {
                Some(key.clone())
            }
            _ => None,
        },
    )
    .map_err(|error| {
        format!(
            "native workflow step {step_index} did not present its {phase:?} cursor frame: {error}"
        )
    })?;
    if !frame_key_matches_session(events, &key, session, ObserverRole::Preview) {
        return Err(format!(
            "native workflow step {step_index} {phase:?} cursor frame belongs to another session"
        ));
    }
    Ok(key)
}

#[cfg(target_os = "linux")]
fn wait_for_native_workflow_target(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    ordinal: u32,
    expected_request_id: u64,
    action: &NativeWorkflowAction,
) -> Result<(String, f32, f32, FrameEvidenceKey), String> {
    for attempt in 0..24 {
        if let Ok(target) = wait_for_value(
            observer,
            events,
            Duration::from_millis(250),
            0,
            |event| match event {
                ObserverEvent::NativeWorkflowTarget {
                    request_id,
                    ordinal: observed,
                    step_id,
                    source_path,
                    action_kind,
                    action_digest,
                    node,
                    x,
                    y,
                    key,
                } if *observed == ordinal && step_id == &action.id => Some((
                    *request_id,
                    node.clone(),
                    *x,
                    *y,
                    source_path.clone(),
                    action_kind.clone(),
                    action_digest.clone(),
                    key.clone(),
                )),
                _ => None,
            },
        ) {
            if target.0 != expected_request_id
                || target.4 != action.source_path
                || target.5 != action.action_kind
                || target.6 != action.action_digest
            {
                return Err(format!(
                    "native workflow target `{}` does not match its scenario declaration",
                    action.id
                ));
            }
            return Ok((target.1, target.2, target.3, target.7));
        }
        let amount = if attempt < 12 { -4 } else { 4 };
        session.run_driver(&["axis", "vertical", &amount.to_string()])?;
    }
    Err(format!(
        "native workflow target `{}` did not become visible after bounded real scrolling",
        action.id
    ))
}

#[cfg(target_os = "linux")]
fn require_exact_profile_frame_chain(
    events: &[ObserverEvent],
    session: &NativeSession,
    input_sequence: u64,
    callback_to_host_ns: u64,
    editor_key: &FrameEvidenceKey,
    child_key: &FrameEvidenceKey,
) -> Result<(), String> {
    let exact = profile_frame_chain_is_exact(
        events,
        input_sequence,
        callback_to_host_ns,
        editor_key,
        child_key,
    ) && frame_key_matches_session(events, editor_key, session, ObserverRole::Preview)
        && frame_key_matches_session(events, child_key, session, ObserverRole::Preview);
    if exact {
        Ok(())
    } else {
        Err(
            "profile sample did not bind one real text callback to exact editor and child frames"
                .to_owned(),
        )
    }
}

#[cfg(target_os = "linux")]
fn profile_frame_chain_is_exact(
    events: &[ObserverEvent],
    input_sequence: u64,
    callback_to_host_ns: u64,
    editor_key: &FrameEvidenceKey,
    child_key: &FrameEvidenceKey,
) -> bool {
    let input = events.iter().find_map(|event| match event {
        ObserverEvent::InputAccepted(input)
            if input.role == ObserverRole::Preview
                && input.real_os
                && input.kind == InputKind::Text
                && input.event_sequence == input_sequence =>
        {
            Some(input)
        }
        _ => None,
    });
    let editor = events.iter().any(|event| {
        matches!(event, ObserverEvent::FramePresented(frame)
            if frame.role == ObserverRole::Preview
                && frame.key == *editor_key
                && frame.event_sequence == Some(input_sequence)
                && frame.input_kind == Some(InputKind::Text))
    });
    let child = events.iter().any(|event| {
        matches!(event, ObserverEvent::FramePresented(frame)
            if frame.role == ObserverRole::Preview && frame.key == *child_key)
    });
    input.is_some_and(|input| {
        input.callback_to_host_ns == callback_to_host_ns
            && input.surface_epoch == editor_key.surface_epoch
    }) && editor
        && child
        && editor_key.same_producer_surface(child_key)
        && child_key.frame_id > editor_key.frame_id
        && frame_key_matches_metadata(events, editor_key, ObserverRole::Preview)
        && frame_key_matches_metadata(events, child_key, ObserverRole::Preview)
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct RoleRectangle {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[cfg(target_os = "linux")]
fn drive_responsive_resize(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    placements: &mut BTreeMap<ObserverRole, WindowPlacement>,
    start: usize,
) -> Result<(), String> {
    let ready = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::ResponsiveResizeReady {
                desired_width,
                desired_height,
                current_width,
                current_height,
                key,
            } => Some((
                *desired_width,
                *desired_height,
                *current_width,
                *current_height,
                key.clone(),
            )),
            _ => None,
        },
    )
    .map_err(|error| format!("responsive resize target was not published: {error}"))?;
    if !frame_key_matches_session(events, &ready.4, session, ObserverRole::Preview) {
        return Err(
            "responsive resize target has the wrong preview process or session identity".to_owned(),
        );
    }
    if (ready.0, ready.1) == (ready.2, ready.3) {
        return Err("responsive checkpoint requires a real native size transition".to_owned());
    }

    let preview_placement =
        activate_window(session, observer, events, placements, ObserverRole::Preview)?;
    let dev_placement = activate_window(session, observer, events, placements, ObserverRole::Dev)?;
    let dev_metadata = events
        .iter()
        .rev()
        .find_map(|event| match event {
            ObserverEvent::RoleMetadata(metadata)
                if metadata.role == ObserverRole::Dev
                    && metadata.session_id == session.session_id =>
            {
                Some(metadata)
            }
            _ => None,
        })
        .ok_or("responsive divider proof has no exact dev role metadata")?;
    let preview = RoleRectangle {
        x: preview_placement.origin.0,
        y: preview_placement.origin.1,
        width: i32::try_from(ready.2).map_err(|_| "preview width is out of range")?,
        height: i32::try_from(ready.3).map_err(|_| "preview height is out of range")?,
    };
    let dev = RoleRectangle {
        x: dev_placement.origin.0,
        y: dev_placement.origin.1,
        width: dev_metadata.logical_width.round() as i32,
        height: dev_metadata.logical_height.round() as i32,
    };
    let (from, to) = divider_drag_points(preview, dev, ready.0, ready.1)?;
    session.run_driver(&["move", &from.0.to_string(), &from.1.to_string()])?;
    thread::sleep(DRAG_GRAB_SETTLE);
    session.run_driver(&["button", "down", "left"])?;
    thread::sleep(DRAG_GRAB_SETTLE);
    let drag = session.run_driver(&["move", &to.0.to_string(), &to.1.to_string()]);
    thread::sleep(DRAG_GRAB_SETTLE);
    let release = session.run_driver(&["button", "up", "left"]);
    drag?;
    release?;

    let observed = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        start,
        |event| match event {
            ObserverEvent::ResponsiveResizeObserved {
                event_sequence,
                logical_width,
                logical_height,
                previous_surface_epoch,
                key,
            } if (*logical_width, *logical_height) == (ready.0, ready.1) => {
                Some((*event_sequence, *previous_surface_epoch, key.clone()))
            }
            _ => None,
        },
    )
    .map_err(|error| {
        let observed_sizes = events[start..]
            .iter()
            .rev()
            .filter_map(|event| match event {
                ObserverEvent::ResponsiveResizeObserved {
                    logical_width,
                    logical_height,
                    previous_surface_epoch,
                    key,
                    ..
                } => Some(format!(
                    "{}x{} epoch {}->{}",
                    logical_width, logical_height, previous_surface_epoch, key.surface_epoch
                )),
                _ => None,
            })
            .take(8)
            .collect::<Vec<_>>();
        let latest_role_sizes = events[start..]
            .iter()
            .rev()
            .filter_map(|event| match event {
                ObserverEvent::RoleMetadata(metadata) => Some(format!(
                    "{:?} {:.0}x{:.0} epoch {}",
                    metadata.role,
                    metadata.logical_width,
                    metadata.logical_height,
                    metadata.surface_epoch
                )),
                _ => None,
            })
            .take(8)
            .collect::<Vec<_>>();
        format!(
            "native divider drag did not reach the declared size: {error}; target={}x{} current={}x{}; preview={preview:?}; dev={dev:?}; drag={from:?}->{to:?}; latest observed resize frames={observed_sizes:?}; latest role sizes={latest_role_sizes:?}",
            ready.0, ready.1, ready.2, ready.3
        )
    })?;
    let exact_resize = observed.2.surface_epoch > observed.1
        && frame_key_matches_session(events, &observed.2, session, ObserverRole::Preview)
        && events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Preview
                    && input.real_os
                    && input.kind == InputKind::Resize
                    && input.event_sequence == observed.0
                    && input.surface_epoch == observed.2.surface_epoch)
        })
        && events.iter().any(|event| {
            matches!(event, ObserverEvent::FramePresented(frame)
                if frame.role == ObserverRole::Preview
                    && frame.key == observed.2)
        })
        && events.iter().any(|event| {
            matches!(event, ObserverEvent::ResponsiveLayoutEvidence {
                resize_sequence,
                logical_width,
                logical_height,
                key,
                ..
            } if *resize_sequence == observed.0
                && (*logical_width, *logical_height) == (ready.0, ready.1)
                && key == &observed.2)
        });
    if !exact_resize {
        return Err(
            "responsive checkpoint is not bound to one exact native Resize frame".to_owned(),
        );
    }
    let status = query_isolation_status(&session.launch_id)?;
    status.require_safe(&session.isolated_seat_name)?;
    status.require_layout(session.observed_roles.len())?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn divider_drag_points(
    preview: RoleRectangle,
    dev: RoleRectangle,
    desired_width: u32,
    desired_height: u32,
) -> Result<((i32, i32), (i32, i32)), String> {
    let desired_width =
        i32::try_from(desired_width).map_err(|_| "desired width is out of range")?;
    let desired_height =
        i32::try_from(desired_height).map_err(|_| "desired height is out of range")?;
    let overlap_midpoint =
        |first_start: i32, first_len: i32, second_start: i32, second_len: i32| {
            let start = first_start.max(second_start);
            let end = first_start
                .saturating_add(first_len)
                .min(second_start.saturating_add(second_len));
            (end > start).then_some(start.saturating_add((end - start) / 2))
        };
    let gap_tolerance = 48;
    let preview_right = preview.x.saturating_add(preview.width);
    let dev_right = dev.x.saturating_add(dev.width);
    if desired_height == preview.height
        && (preview_right - dev.x).abs() <= gap_tolerance
        && let Some(y) = overlap_midpoint(preview.y, preview.height, dev.y, dev.height)
    {
        let from = ((preview_right + dev.x) / 2, y);
        return Ok((from, (from.0 + desired_width - preview.width, y)));
    }
    if desired_height == preview.height
        && (dev_right - preview.x).abs() <= gap_tolerance
        && let Some(y) = overlap_midpoint(preview.y, preview.height, dev.y, dev.height)
    {
        let from = ((dev_right + preview.x) / 2, y);
        return Ok((from, (from.0 + preview.width - desired_width, y)));
    }
    let preview_bottom = preview.y.saturating_add(preview.height);
    let dev_bottom = dev.y.saturating_add(dev.height);
    if desired_width == preview.width
        && (preview_bottom - dev.y).abs() <= gap_tolerance
        && let Some(x) = overlap_midpoint(preview.x, preview.width, dev.x, dev.width)
    {
        let from = (x, (preview_bottom + dev.y) / 2);
        return Ok((from, (x, from.1 + desired_height - preview.height)));
    }
    if desired_width == preview.width
        && (dev_bottom - preview.y).abs() <= gap_tolerance
        && let Some(x) = overlap_midpoint(preview.x, preview.width, dev.x, dev.width)
    {
        let from = (x, (dev_bottom + preview.y) / 2);
        return Ok((from, (x, from.1 + preview.height - desired_height)));
    }
    Err(format!(
        "declared responsive size {desired_width}x{desired_height} cannot be reached through the proven tiled divider: preview={preview:?}, dev={dev:?}"
    ))
}

#[cfg(target_os = "linux")]
fn input_event_trace(events: &[ObserverEvent], start: usize, limit: usize) -> String {
    let mut values = events
        .iter()
        .skip(start.min(events.len()))
        .filter_map(|event| match event {
            ObserverEvent::InputAccepted(input) => Some(format!(
                "{:?}:{:?}:{:?}@({:.1},{:.1}) target={:?} visible={}",
                input.role,
                input.kind,
                input.pointer_button_pressed,
                input.pointer_x.unwrap_or_default(),
                input.pointer_y.unwrap_or_default(),
                input.target,
                input.visible_change
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    if values.len() > limit {
        values.drain(..values.len() - limit);
    }
    format!("{values:?}")
}

#[cfg(target_os = "linux")]
fn drive_click_sample(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    target: &str,
) -> Result<u64, String> {
    drain_events(observer, events, INPUT_CALIBRATION_QUIET);
    let start = events.len();
    session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
    session.run_driver(&["click", "left"])?;
    wait_for_visible_present(observer, events, start, InputKind::PointerButton, |input| {
        input.target.as_deref() == Some(target) && input.pointer_button_pressed == Some(false)
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn drive_profile_visible_sample(
    profile: &VerifierProfile,
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    preview_point: (i32, i32),
    preview_target: &str,
    off_target: Option<&((i32, i32), Option<String>)>,
    ordinal: usize,
) -> Result<u64, String> {
    if profile.visible_mode == VisibleSampleMode::Click {
        return drive_click_sample(session, observer, events, preview_point, preview_target);
    }
    let alternate = off_target.ok_or("hover profile has no alternate target")?;
    let (point, expected) = if ordinal % 2 == 0 {
        (preview_point, Some(preview_target))
    } else {
        (alternate.0, alternate.1.as_deref())
    };
    drive_visible_sample(
        session,
        observer,
        events,
        point,
        InputKind::PointerMove,
        |input| input.target.as_deref() == expected,
    )
}

#[cfg(target_os = "linux")]
fn presented_key_for_sequence(events: &[ObserverEvent], sequence: u64) -> Option<FrameEvidenceKey> {
    events.iter().rev().find_map(|event| match event {
        ObserverEvent::FramePresented(frame)
            if frame.role == ObserverRole::Preview && frame.event_sequence == Some(sequence) =>
        {
            Some(frame.key.clone())
        }
        _ => None,
    })
}

#[cfg(target_os = "linux")]
fn wait_for_evidence_proofs(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
) -> Result<(), String> {
    let evidence_keys = events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::StateMounted { key, .. }
            | ObserverEvent::ScenarioCheckpoint { key, .. }
            | ObserverEvent::PersistenceEvidence { key, .. }
            | ObserverEvent::ResponsiveLayoutEvidence { key, .. }
            | ObserverEvent::StaleProgramRejected { key, .. }
            | ObserverEvent::NativeWorkflowStep { key, .. }
            | ObserverEvent::ScrollProofFrame { key, .. } => Some(key.clone()),
            ObserverEvent::ProfileSample {
                ordinal: 11, key, ..
            } => Some(key.clone()),
            _ => None,
        })
        .fold(Vec::<FrameEvidenceKey>::new(), |mut keys, key| {
            if !keys.contains(&key) {
                keys.push(key);
            }
            keys
        })
        .into_iter()
        .filter(|key| {
            events.iter().any(
                |event| matches!(event, ObserverEvent::ProofRequested { key: requested, .. } if requested == key),
            )
        })
        .collect::<Vec<_>>();
    if evidence_keys.is_empty() {
        return Ok(());
    }
    wait_for_count(observer, events, Duration::from_secs(60), |events| {
        evidence_keys
            .iter()
            .all(|key| exact_proof_for_key(events, key).is_some())
    })
}

#[cfg(target_os = "linux")]
fn wait_for_exact_proof(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    key: &FrameEvidenceKey,
) -> Result<(), String> {
    wait_for_count(observer, events, timeout, |events| {
        exact_proof_for_key(events, key).is_some()
    })
}

#[cfg(target_os = "linux")]
fn drive_visible_sample(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    kind: InputKind,
    matches_input: impl Fn(&InputAccepted) -> bool,
) -> Result<u64, String> {
    let start = events.len();
    session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
    wait_for_visible_present(observer, events, start, kind, matches_input)
}

#[cfg(target_os = "linux")]
fn wait_for_visible_present(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    start: usize,
    kind: InputKind,
    matches_input: impl Fn(&InputAccepted) -> bool,
) -> Result<u64, String> {
    let sequence =
        wait_for_value(
            observer,
            events,
            Duration::from_secs(2),
            start,
            |event| match event {
                ObserverEvent::InputAccepted(input)
                    if input.role == ObserverRole::Preview
                        && input.real_os
                        && input.kind == kind
                        && input.visible_change
                        && matches_input(input) =>
                {
                    Some(input.event_sequence)
                }
                _ => None,
            },
        )
        .map_err(|error| format!("visible input was not accepted: {error}"))?;
    wait_for_event(observer, events, Duration::from_secs(2), start, |event| {
        matches!(event, ObserverEvent::FramePresented(frame)
                if frame.role == ObserverRole::Preview
                    && frame.event_sequence == Some(sequence))
    })
    .map_err(|error| format!("accepted input {sequence} was not presented: {error}"))?;
    Ok(sequence)
}

#[cfg(target_os = "linux")]
fn drive_wheel_attempt(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    amount: i32,
) -> Result<Option<u64>, String> {
    let start = events.len();
    session.run_driver(&["axis", "vertical", &amount.to_string()])?;
    let accepted =
        wait_for_value(
            observer,
            events,
            Duration::from_secs(2),
            start,
            |event| match event {
                ObserverEvent::InputAccepted(input)
                    if input.role == ObserverRole::Preview
                        && input.real_os
                        && input.kind == InputKind::Wheel =>
                {
                    Some(input.clone())
                }
                _ => None,
            },
        )?;
    if !accepted.visible_change {
        return Ok(None);
    }
    wait_for_event(observer, events, Duration::from_secs(2), start, |event| {
        matches!(event, ObserverEvent::FramePresented(frame)
                if frame.role == ObserverRole::Preview
                    && frame.event_sequence == Some(accepted.event_sequence))
    })?;
    Ok(Some(accepted.event_sequence))
}

#[cfg(target_os = "linux")]
fn wait_for_metadata(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
) -> Result<(), String> {
    wait_for_count(observer, events, ROLE_READY_TIMEOUT, |events| {
        let preview = events.iter().any(|event| {
            matches!(event, ObserverEvent::RoleMetadata(metadata)
                if metadata.role == ObserverRole::Preview)
        });
        let dev = events.iter().any(|event| {
            matches!(event, ObserverEvent::RoleMetadata(metadata)
                if metadata.role == ObserverRole::Dev)
        });
        preview && dev
    })
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct WindowPlacement {
    origin: (i32, i32),
    visible_point: (i32, i32),
}

#[cfg(target_os = "linux")]
fn discover_window_placements(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
) -> Result<BTreeMap<ObserverRole, WindowPlacement>, String> {
    let mut placements = BTreeMap::new();
    let (first_role, _) = observe_window(session, observer, events, None)?;
    let first_placement = stable_window_placement(session, observer, events, first_role)?;
    placements.insert(first_role, first_placement);
    let second_role = other_role(first_role);
    let second_placement = stable_window_placement(session, observer, events, second_role)?;
    placements.insert(second_role, second_placement);
    Ok(placements)
}

#[cfg(target_os = "linux")]
fn stable_window_placement(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    expected: ObserverRole,
) -> Result<WindowPlacement, String> {
    let mut previous: Option<(i32, i32)> = None;
    for _ in 0..8 {
        let (_, placement) = observe_window(session, observer, events, Some(expected))?;
        if previous.is_some_and(|previous| {
            (previous.0 - placement.origin.0).abs() <= 1
                && (previous.1 - placement.origin.1).abs() <= 1
        }) {
            return Ok(placement);
        }
        previous = Some(placement.origin);
    }
    Err(format!(
        "{expected:?} native window placement did not stabilize; last origin={previous:?}"
    ))
}

#[cfg(target_os = "linux")]
fn activate_window(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    placements: &mut BTreeMap<ObserverRole, WindowPlacement>,
    expected: ObserverRole,
) -> Result<WindowPlacement, String> {
    let placement = match placements.get(&expected).copied() {
        Some(placement) => {
            refresh_window_placement(session, observer, events, placement.visible_point, expected)
                .or_else(|_| stable_window_placement(session, observer, events, expected))?
        }
        _ => stable_window_placement(session, observer, events, expected)?,
    };
    placements.insert(expected, placement);
    Ok(placement)
}

#[cfg(target_os = "linux")]
fn refresh_window_placement(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    expected: ObserverRole,
) -> Result<WindowPlacement, String> {
    let (actual, input) =
        move_with_marker(session, observer, events, point, Duration::from_millis(400)).or_else(
            |_| {
                move_with_marker(
                    session,
                    observer,
                    events,
                    (point.0.saturating_add(1), point.1),
                    Duration::from_millis(400),
                )
            },
        )?;
    if input.role != expected {
        return Err(format!(
            "expected {expected:?} at {point:?}, observed {:?}",
            input.role
        ));
    }
    let local_x = input.pointer_x.expect("filtered pointer x");
    let local_y = input.pointer_y.expect("filtered pointer y");
    Ok(WindowPlacement {
        origin: (
            actual.0 - local_x.round() as i32,
            actual.1 - local_y.round() as i32,
        ),
        visible_point: actual,
    })
}

#[cfg(target_os = "linux")]
fn move_with_marker(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    timeout: Duration,
) -> Result<((i32, i32), InputAccepted), String> {
    drain_events(observer, events, INPUT_CALIBRATION_QUIET);
    let start = events.len();
    let actual = session.move_pointer(point)?;
    let marker = wait_for_value(observer, events, timeout, start, |event| match event {
        ObserverEvent::InputAccepted(input)
            if input.real_os
                && input.kind == InputKind::PointerMove
                && input.pointer_x.is_some()
                && input.pointer_y.is_some() =>
        {
            Some(input.clone())
        }
        _ => None,
    })
    .map_err(|error| format!("pointer move marker was not accepted: {error}"))?;
    Ok((actual, marker))
}

#[cfg(target_os = "linux")]
fn observe_window(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    expected: Option<ObserverRole>,
) -> Result<(ObserverRole, WindowPlacement), String> {
    let mut last_role = None;
    let mut observations = VecDeque::with_capacity(12);
    for point in window_scan_candidates(session.pointer_space()?) {
        if let Ok((actual, input)) =
            move_with_marker(session, observer, events, point, Duration::from_millis(100))
        {
            last_role = Some(input.role);
            if observations.len() == 12 {
                observations.pop_front();
            }
            observations.push_back(format!(
                "requested={point:?} acknowledged={actual:?} role={:?} local=({:.1},{:.1})",
                input.role,
                input.pointer_x.unwrap_or_default(),
                input.pointer_y.unwrap_or_default()
            ));
            if expected.is_some() && expected != Some(input.role) {
                continue;
            }
            let local_x = input.pointer_x.expect("filtered pointer x");
            let local_y = input.pointer_y.expect("filtered pointer y");
            return Ok((
                input.role,
                WindowPlacement {
                    origin: (
                        actual.0 - local_x.round() as i32,
                        actual.1 - local_y.round() as i32,
                    ),
                    visible_point: actual,
                },
            ));
        }
    }
    match expected {
        Some(role) => Err(format!(
            "real pointer scan could not observe {role:?}; last visible role was {last_role:?}; observations={observations:?}"
        )),
        None => Err("real pointer input did not identify a native role".to_owned()),
    }
}

#[cfg(target_os = "linux")]
fn other_role(role: ObserverRole) -> ObserverRole {
    match role {
        ObserverRole::Preview => ObserverRole::Dev,
        ObserverRole::Dev => ObserverRole::Preview,
    }
}

#[cfg(target_os = "linux")]
fn window_scan_candidates((width, height): (i32, i32)) -> Vec<(i32, i32)> {
    let point = |x_num: i32, x_den: i32, y_num: i32, y_den: i32| {
        (
            (width.saturating_mul(x_num) / x_den).clamp(0, width.saturating_sub(1)),
            (height.saturating_mul(y_num) / y_den).clamp(0, height.saturating_sub(1)),
        )
    };
    let mut candidates = vec![
        point(1, 4, 1, 4),
        point(3, 4, 1, 4),
        point(1, 4, 3, 4),
        point(3, 4, 3, 4),
        point(1, 2, 1, 2),
        point(7, 8, 1, 2),
    ];
    for y_index in 1..10 {
        for x_index in 1..16 {
            let candidate = point(x_index, 16, y_index, 10);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

#[cfg(target_os = "linux")]
fn observed_role_target(
    events: &[ObserverEvent],
    role: ObserverRole,
    expected_node: &str,
) -> Option<(f32, f32)> {
    events.iter().rev().find_map(|event| match event {
        ObserverEvent::RoleTarget {
            role: observed,
            node,
            x,
            y,
        } if *observed == role && node == expected_node => Some((*x, *y)),
        _ => None,
    })
}

#[cfg(target_os = "linux")]
fn locate_target(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    role: ObserverRole,
    target: &str,
    target_center: (f32, f32),
    candidates: Vec<(i32, i32)>,
) -> Result<(i32, i32), String> {
    let seed = candidates.iter().copied().take(4).collect::<Vec<_>>();
    let mut candidates = VecDeque::from(candidates);
    let mut observations = VecDeque::with_capacity(4);
    for _ in 0..32 {
        let Some(point) = candidates.pop_front() else {
            break;
        };
        let Ok((actual, input)) =
            move_with_marker(session, observer, events, point, Duration::from_millis(400))
        else {
            continue;
        };
        if observations.len() == 4 {
            observations.pop_front();
        }
        observations.push_back(format!(
            "global={actual:?} local=({:.1},{:.1}) role={:?} target={:?}",
            input.pointer_x.unwrap_or_default(),
            input.pointer_y.unwrap_or_default(),
            input.role,
            input.target
        ));
        if input.role != role {
            continue;
        }
        if input.target.as_deref() == Some(target) {
            return Ok(actual);
        }
        let local_x = input.pointer_x.expect("filtered pointer x");
        let local_y = input.pointer_y.expect("filtered pointer y");
        let corrected = (
            actual.0 + (target_center.0 - local_x).round() as i32,
            actual.1 + (target_center.1 - local_y).round() as i32,
        );
        if corrected != actual {
            let quarter = (
                actual.0 + ((target_center.0 - local_x) * 0.25).round() as i32,
                actual.1 + ((target_center.1 - local_y) * 0.25).round() as i32,
            );
            if quarter != actual && quarter != corrected {
                candidates.push_front(quarter);
            }
            let halfway = (
                actual.0 + ((target_center.0 - local_x) * 0.5).round() as i32,
                actual.1 + ((target_center.1 - local_y) * 0.5).round() as i32,
            );
            if halfway != actual && halfway != corrected {
                candidates.push_front(halfway);
            }
            candidates.push_front(corrected);
        }
    }
    Err(format!(
        "real pointer scan could not resolve {role:?} target `{target}` at local center ({:.1},{:.1}); seed={seed:?}; observations={observations:?}",
        target_center.0, target_center.1
    ))
}

#[cfg(target_os = "linux")]
fn locate_different_preview_target(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    target: &str,
    required_source_path: Option<&str>,
    around: (i32, i32),
    origin: (i32, i32),
) -> Result<((i32, i32), Option<String>), String> {
    let candidates = [
        (around.0 + 100, around.1),
        (around.0 - 100, around.1),
        (around.0, around.1 + 80),
        (origin.0 + 20, origin.1 + 20),
        (origin.0 + 760, origin.1 + 20),
        (origin.0 + 20, origin.1 + 560),
    ];
    for point in candidates {
        if let Ok((actual, input)) =
            move_with_marker(session, observer, events, point, Duration::from_millis(180))
            && input.role == ObserverRole::Preview
            && input.target.as_deref() != Some(target)
            && required_source_path
                .is_none_or(|source_path| input.target_source_path.as_deref() == Some(source_path))
        {
            return Ok((actual, input.target));
        }
    }
    Err("could not find a second preview hover state for real interaction samples".to_owned())
}

#[cfg(target_os = "linux")]
fn translated_target_candidates(placement: WindowPlacement, x: f32, y: f32) -> Vec<(i32, i32)> {
    let base_x = placement.origin.0 + x.round() as i32;
    let base_y = placement.origin.1 + y.round() as i32;
    let visible = placement.visible_point;
    let mut candidates = vec![
        visible,
        (visible.0.saturating_add(1), visible.1),
        (visible.0.saturating_sub(1), visible.1),
        (visible.0, visible.1.saturating_add(1)),
        (visible.0, visible.1.saturating_sub(1)),
    ];
    for dy in [0, 24, 32, -8, 40, -16] {
        for dx in [0, -8, 8, -16, 16, -24, 24] {
            candidates.push((base_x + dx, base_y + dy));
        }
    }
    candidates
}

#[cfg(target_os = "linux")]
fn wait_for_event(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    start: usize,
    predicate: impl Fn(&ObserverEvent) -> bool,
) -> Result<(), String> {
    wait_for_value(observer, events, timeout, start, |event| {
        predicate(event).then_some(())
    })
}

#[cfg(target_os = "linux")]
fn wait_for_value<T>(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    start: usize,
    map: impl Fn(&ObserverEvent) -> Option<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + timeout;
    let mut scanned = start.min(events.len());
    loop {
        while scanned < events.len() {
            if let Some(value) = map(&events[scanned]) {
                return Ok(value);
            }
            scanned += 1;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "observer condition was not met within {}ms",
                timeout.as_millis()
            ));
        }
        match observer.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => push_event(events, event)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("verifier observer disconnected".to_owned());
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_for_count(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    predicate: impl Fn(&[ObserverEvent]) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if predicate(events) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "observer sample count was not met within {}ms",
                timeout.as_millis()
            ));
        }
        match observer.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => push_event(events, event)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("verifier observer disconnected".to_owned());
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn drain_events(observer: &mut ObserverServer, events: &mut Vec<ObserverEvent>, quiet: Duration) {
    while let Ok(event) = observer.recv_timeout(quiet) {
        if push_event(events, event).is_err() {
            return;
        }
    }
}

#[cfg(target_os = "linux")]
fn push_event(
    events: &mut Vec<ObserverEvent>,
    event: Result<ObserverEvent, String>,
) -> Result<(), String> {
    let event = event?;
    if events.len() >= MAX_OBSERVER_EVENTS {
        return Err(format!(
            "verifier observer exceeded its bounded {MAX_OBSERVER_EVENTS}-event capacity"
        ));
    }
    events.push(event);
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct ProductSamples {
    visible: BTreeSet<u64>,
    clicks: BTreeSet<u64>,
    scroll: BTreeSet<u64>,
}

#[cfg(target_os = "linux")]
impl ProductSamples {
    fn callback_sequences(&self) -> BTreeSet<u64> {
        self.visible.union(&self.scroll).copied().collect()
    }
}

#[cfg(target_os = "linux")]
fn test_pointer_playback_summary(events: &[ObserverEvent]) -> (bool, String) {
    let Some((request_id, completed_steps)) = events.iter().rev().find_map(|event| match event {
        ObserverEvent::TestCompleted {
            request_id,
            passed: true,
            semantic_assertions_proven: true,
            completed_steps,
            ..
        } => Some((*request_id, *completed_steps)),
        _ => None,
    }) else {
        return (false, "no passing TEST playback was observed".to_owned());
    };
    let frames = events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::TestPointerFrame {
                request_id: frame_request,
                step_index,
                phase,
                x,
                y,
                target,
                runtime_sequence,
                key,
            } if *frame_request == request_id => Some((
                *step_index,
                *phase,
                *x,
                *y,
                target.as_deref(),
                *runtime_sequence,
                key,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    if frames.is_empty() {
        return (
            false,
            format!("TEST #{request_id} completed without any presented pointer frames"),
        );
    }
    if frames
        .windows(2)
        .any(|pair| pair[0].6.frame_id >= pair[1].6.frame_id)
        || frames.iter().any(|frame| !frame.6.is_complete())
    {
        return (
            false,
            format!("TEST #{request_id} pointer frames lack strict presented-frame identity"),
        );
    }
    let unique_positions = frames
        .iter()
        .filter(|frame| frame.1 == TestPointerPhase::Move)
        .map(|frame| (frame.2.round() as i32, frame.3.round() as i32))
        .collect::<BTreeSet<_>>();
    if unique_positions.len() < 2 {
        return (
            false,
            format!("TEST #{request_id} cursor never visibly moved between distinct positions"),
        );
    }
    for step_index in 0..completed_steps {
        let step_frames = frames
            .iter()
            .filter(|frame| frame.0 == step_index)
            .collect::<Vec<_>>();
        let interactive = step_frames
            .iter()
            .any(|frame| frame.1 != TestPointerPhase::State);
        let required_phases = if interactive {
            &[
                TestPointerPhase::Move,
                TestPointerPhase::Hover,
                TestPointerPhase::Down,
                TestPointerPhase::Up,
                TestPointerPhase::State,
            ][..]
        } else {
            &[TestPointerPhase::State][..]
        };
        for required in required_phases {
            if !step_frames.iter().any(|frame| frame.1 == *required) {
                return (
                    false,
                    format!(
                        "TEST #{request_id} step {step_index} has no presented {required:?} frame"
                    ),
                );
            }
        }
        let interactive_targets = step_frames
            .iter()
            .filter(|frame| {
                matches!(
                    frame.1,
                    TestPointerPhase::Hover | TestPointerPhase::Down | TestPointerPhase::Up
                )
            })
            .filter_map(|frame| frame.4)
            .collect::<BTreeSet<_>>();
        if interactive && interactive_targets.len() != 1 {
            return (
                false,
                format!(
                    "TEST #{request_id} step {step_index} hover/down/up did not retain one hit target: {interactive_targets:?}"
                ),
            );
        }
        let first_sequence = step_frames.first().map_or(0, |frame| frame.5);
        let final_sequence = step_frames.last().map_or(0, |frame| frame.5);
        if interactive && final_sequence <= first_sequence {
            return (
                false,
                format!(
                    "TEST #{request_id} step {step_index} did not change runtime sequence after pointer playback"
                ),
            );
        }
    }
    (
        true,
        format!(
            "TEST #{request_id} presented {} cursor frames across {completed_steps} steps with move, hover, down, up, and resulting state",
            frames.len()
        ),
    )
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct Capture {
    checks: Vec<Check>,
    events: Vec<ObserverEvent>,
    samples: ProductSamples,
    state_root: Option<CapturedStateRoot>,
    launch_isolation: Vec<LaunchIsolationEvidence>,
}

#[cfg(target_os = "linux")]
struct CapturedStateRoot {
    path: PathBuf,
    clean_at_start: bool,
    restart_count: u32,
    restored_after_restart: bool,
}

#[cfg(target_os = "linux")]
impl Capture {
    fn finalize_checks(&mut self, profile: &VerifierProfile, roles: RolePids) {
        let metadata = role_metadata(&self.events);
        let metadata_ok = metadata.contains_key(&ObserverRole::Preview)
            && metadata.contains_key(&ObserverRole::Dev)
            && metadata
                .get(&ObserverRole::Preview)
                .is_some_and(|value| value.pid == roles.preview)
            && metadata
                .get(&ObserverRole::Dev)
                .is_some_and(|value| value.pid == roles.dev);
        self.checks.push(check_result(
            "role-owned-native-metadata",
            metadata_ok,
            "preview and dev exported PID, adapter, surface, epoch, format, and present metadata",
            "preview/dev role metadata was missing or did not match the live role PIDs",
        ));

        let real_test = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.target.as_deref() == Some("dev.test")
                    && input.kind == InputKind::PointerButton)
        });
        let test_passed = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::TestCompleted {
                passed: true,
                semantic_assertions_proven: true,
                completed_steps,
                ..
            }
                if *completed_steps > 0)
        });
        self.checks.push(check_result(
            "real-dev-test-click",
            real_test && test_passed,
            "real Wayland pointer input clicked dev TEST and the preview scenario completed",
            "dev TEST was not both clicked through real app_window input and completed in preview",
        ));
        let (playback_valid, playback_detail) = test_pointer_playback_summary(&self.events);
        self.checks.push(check_result(
            "visible-test-pointer-playback",
            playback_valid,
            playback_detail.clone(),
            playback_detail,
        ));

        let real_dev_keyboard = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Keyboard)
        });
        let real_dev_wheel = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Wheel)
        });
        self.checks.push(check_result(
            "real-dev-keyboard-and-wheel",
            real_dev_keyboard && real_dev_wheel,
            "kernel keyboard and wheel events reached the focused dev editor through app_window",
            "dev editor did not receive both real keyboard and wheel callbacks",
        ));

        let real_preview = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Preview
                    && input.real_os
                    && input.visible_change)
        });
        self.checks.push(check_result(
            "real-preview-interaction",
            real_preview,
            "preview visible state changed from a real app_window callback",
            "no real preview HostEvent produced a visible frame",
        ));

        let observer_drops = self
            .events
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::FramePresented(frame) => Some(frame.observer_drop_count),
                ObserverEvent::ProofCompleted {
                    result_drop_count, ..
                } => Some(*result_drop_count),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let proof_replaced = self
            .events
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::ProofCompleted { replaced_count, .. } => Some(*replaced_count),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.checks.push(check_result(
            "bounded-observer-and-proof-backpressure",
            observer_drops == 0 && proof_replaced == 0,
            "bounded observer and depth-one proof lanes completed without dropped or replaced evidence",
            format!(
                "observer drops={observer_drops}, proof replacements={proof_replaced}"
            ),
        ));

        let callback_sequences = self.samples.callback_sequences();
        let callback = callback_samples(&self.events, &callback_sequences);
        let visible = preview_visible_frames(&self.events, &self.samples.visible);
        self.checks.push(check_result(
            "minimum-product-samples",
            callback.len() >= 70 && visible.len() >= 70,
            format!(
                "collected {} callback and {} preview visible samples including warmup",
                callback.len(),
                visible.len()
            ),
            format!(
                "insufficient bounded samples: callbacks={}, preview-visible={}",
                callback.len(),
                visible.len()
            ),
        ));
        if profile.selection_samples > 0 {
            let clicks = preview_visible_frames(&self.events, &self.samples.clicks);
            self.checks.push(check_result(
                "minimum-selection-samples",
                clicks.len() >= profile.selection_samples,
                format!("collected {} real selection samples", clicks.len()),
                format!("collected only {} real selection samples", clicks.len()),
            ));
        }
        if profile.scroll_samples > 0 {
            let scroll = preview_scroll_frames(&self.events, &self.samples.scroll);
            self.checks.push(check_result(
                "minimum-scroll-samples",
                scroll.len() >= profile.scroll_samples,
                format!("collected {} real preview scroll samples", scroll.len()),
                format!(
                    "collected only {} real preview scroll samples",
                    scroll.len()
                ),
            ));
            let representative = scroll.get(20).map(|frame| &frame.key);
            let declared = scroll_proof_key(&self.events, 21);
            self.checks.push(check_result(
                "warm-scroll-exact-proof",
                representative.is_some_and(|key| declared == Some(key)),
                "scroll ordinal 21 has an exact app-owned proof for its reported frame identity",
                "scroll ordinal 21 is missing an exact same-surface, process, and session proof",
            ));
        }
        if profile.switch_samples > 0 {
            let ack = switch_ack_samples(&self.events);
            let final_samples = switch_final_samples(&self.events);
            self.checks.push(check_result(
                "minimum-example-switch-samples",
                ack.len() >= profile.switch_samples
                    && final_samples.len() >= profile.switch_samples,
                format!(
                    "collected {} source acknowledgements and {} final preview switches",
                    ack.len(),
                    final_samples.len()
                ),
                format!(
                    "insufficient switch samples: acknowledgements={}, final={}",
                    ack.len(),
                    final_samples.len()
                ),
            ));
        }

        let exact_proof = exact_proof(&self.events).is_some();
        self.checks.push(check_result(
            "exact-frame-app-owned-proof",
            exact_proof,
            "post-present proof completed for the same FrameEvidenceKey through app-owned WGPU readback",
            "no app-owned proof matched a previously presented product frame identity",
        ));

        let expected_isolation_records = usize::from(profile.restart_required) + 1;
        let structured_isolation = self.launch_isolation.len() == expected_isolation_records
            && self
                .launch_isolation
                .iter()
                .all(LaunchIsolationEvidence::is_fail_closed);
        self.checks.push(check_result(
            "structured-launch-isolation",
            structured_isolation,
            "launch isolation, tiled layout, two-device ownership, and input ordering are recorded as bounded structured values",
            format!(
                "structured launch isolation is incomplete: observed={}, expected={expected_isolation_records}",
                self.launch_isolation.len()
            ),
        ));

        if let Some(preview) = metadata.get(&ObserverRole::Preview) {
            self.checks.push(check_result(
                "hardware-adapter",
                !preview.software_adapter && preview.adapter_device_type != "cpu",
                format!(
                    "preview used hardware adapter {} ({})",
                    preview.adapter_name, preview.adapter_device_type
                ),
                format!(
                    "the real COSMIC session exposed software adapter {} ({}); correctness evidence remains valid but the product gate cannot pass",
                    preview.adapter_name, preview.adapter_device_type
                ),
            ));
        }
        self.add_budget_checks(profile);
        self.add_profile_checks(profile);
    }

    fn add_budget_checks(&mut self, profile: &VerifierProfile) {
        let callback_sequences = self.samples.callback_sequences();
        add_budget_check(
            &mut self.checks,
            "callback-to-host-budget",
            &callback_samples(&self.events, &callback_sequences),
            10,
            60,
            Some(1_000),
            Some(1_000),
            2_000,
        );
        add_frame_budget_check(
            &mut self.checks,
            "warm-visible-budget",
            &preview_visible_frames(&self.events, &self.samples.visible),
            10,
            60,
            16_700,
            33_400,
        );
        if profile.selection_samples > 0 {
            add_frame_budget_check(
                &mut self.checks,
                "repeated-selection-budget",
                &preview_visible_frames(&self.events, &self.samples.clicks),
                4,
                20,
                16_700,
                33_400,
            );
        }
        if profile.scroll_samples > 0 {
            add_frame_budget_check(
                &mut self.checks,
                "warm-scroll-budget",
                &preview_scroll_frames(&self.events, &self.samples.scroll),
                20,
                120,
                16_700,
                33_400,
            );
        }
        if profile.switch_samples > 0 {
            add_switch_budget_check(
                &mut self.checks,
                "switch-ack-budget",
                &switch_ack_samples(&self.events),
                3,
                20,
                16_700,
                33_400,
            );
            add_switch_final_budget_check(
                &mut self.checks,
                "switch-final-budget",
                &switch_final_samples(&self.events),
                &self.events,
                3,
                20,
                250_000,
                500_000,
            );
        }
    }

    fn add_profile_checks(&mut self, profile: &VerifierProfile) {
        let evidence = observed_profile_evidence(
            profile,
            scenario_completion(&self.events),
            &self.events,
            &self.samples,
            self.state_root.as_ref(),
        );
        if profile.scenario_proof.is_some() {
            let scenario = evidence.scenario.as_ref();
            let complete = scenario.is_some_and(|proof| {
                proof.passed
                    && proof.executable_steps > 0
                    && proof.completed_steps == proof.executable_steps
                    && (!profile.require_semantic_scenario || proof.semantic_assertions_proven)
            });
            self.checks.push(check_result(
                "profile-scenario-proof",
                complete,
                "the declared scenario completed with the required semantic assertions",
                match scenario {
                    Some(proof) if proof.passed && !proof.semantic_assertions_proven => {
                        "native TEST playback completed, but observer evidence does not prove the scenario's semantic assertions".to_owned()
                    }
                    Some(proof) => format!(
                        "scenario proof is incomplete: passed={}, completed={}/{}",
                        proof.passed, proof.completed_steps, proof.executable_steps
                    ),
                    None => "the declared scenario could not be read and identified".to_owned(),
                },
            ));
        }
        if !profile.required_budget_metrics.is_empty() {
            let direct_samples = self
                .events
                .iter()
                .filter(|event| match event {
                    ObserverEvent::ProfileSample {
                        input_sequence,
                        callback_to_host_ns,
                        editor_key,
                        key,
                        ..
                    } => profile_frame_chain_is_exact(
                        &self.events,
                        *input_sequence,
                        *callback_to_host_ns,
                        editor_key,
                        key,
                    ),
                    _ => false,
                })
                .count();
            self.checks.push(check_result(
                "profile-benchmark-host-boundary",
                direct_samples == 120,
                "launch-scoped uinput produced 120 real text callbacks with exact editor-present and child-program-present frame chains",
                format!(
                    "profile benchmark emitted {direct_samples}/120 exact kernel-uinput text frame chains"
                ),
            ));
            self.checks.push(match profile_stage_summary(&self.events) {
                Some(detail) => Check::pass("profile-stage-breakdown", detail),
                None => Check::fail(
                    "profile-stage-breakdown",
                    "profile stage timings were incomplete or did not cover 120 ordered samples",
                ),
            });
            let observed = evidence
                .budget
                .as_ref()
                .map_or(&[][..], |proof| proof.observations.as_slice());
            let missing = profile
                .required_budget_metrics
                .iter()
                .filter(|metric| {
                    !observed.iter().any(|observation| {
                        observation.metric == metric.as_str()
                            && observation.observed <= observation.limit
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            self.checks.push(check_result(
                "profile-budget-proof",
                missing.is_empty(),
                "all manifest-required budget observations were measured within their limits",
                format!(
                    "budget declaration was identified, but measured observations are missing or failing: {}",
                    missing.join(", ")
                ),
            ));
        }
        if !profile.required_async_lanes.is_empty() {
            let missing = profile
                .required_async_lanes
                .iter()
                .copied()
                .filter(|required| {
                    !self.events.iter().any(|event| {
                        matches!(
                            event,
                            ObserverEvent::AsyncLaneCompleted {
                                lane,
                                outcome: AsyncLaneOutcome::Applied,
                                ..
                            } if lane == required && async_lane_event_is_valid(event, &self.events)
                        )
                    })
                })
                .map(async_lane_name)
                .collect::<Vec<_>>();
            self.checks.push(check_result(
                "profile-async-lane-proof",
                missing.is_empty(),
                "every manifest-required async lane completed off the product frame and applied to an exact presented frame",
                format!("missing valid applied async lane evidence: {}", missing.join(", ")),
            ));
        }
        if profile.state_root_policy.is_some() {
            let state = evidence.state_root.as_ref();
            let complete = state.is_some_and(|proof| {
                proof.clean_at_start
                    && proof.durable_file_count > 0
                    && (!profile.restart_required
                        || (proof.restart_count > 0 && proof.restored_after_restart))
            });
            self.checks.push(check_result(
                "profile-state-root-proof",
                complete,
                "launch-scoped state began clean, persisted data, and restored after restart",
                "no app-owned launch-scoped state-root and restart evidence was emitted",
            ));
        }
        if !profile.required_native_workflow_steps.is_empty() {
            let workflow = evidence.native_workflow.as_ref();
            let complete = workflow.is_some_and(|proof| {
                proof.input_delivery == "native-os-app-window-callback"
                    && proof.steps.len() == profile.required_native_workflow_steps.len()
                    && proof
                        .steps
                        .iter()
                        .zip(&profile.required_native_workflow_steps)
                        .all(|(observed, required)| &observed.scenario_step == required)
            });
            self.checks.push(check_result(
                "profile-native-workflow-proof",
                complete,
                "every declared product workflow action entered through launch-scoped real OS input and reached its semantic state",
                "the manifest-declared native workflow is incomplete or not bound to real OS input",
            ));
        }
        if !profile.required_checkpoints.is_empty() {
            let missing = profile
                .required_checkpoints
                .iter()
                .filter(|required| {
                    !evidence
                        .checkpoints
                        .iter()
                        .any(|checkpoint| checkpoint.id == required.id)
                })
                .map(|required| required.id.clone())
                .collect::<Vec<_>>();
            self.checks.push(check_result(
                "profile-checkpoint-proof",
                missing.is_empty(),
                "all manifest-required authoritative state checkpoints were observed",
                format!(
                    "missing authoritative state checkpoints: {}",
                    missing.join(", ")
                ),
            ));
        }
    }

    fn into_evidence(self, profile: &VerifierProfile) -> GateEvidence {
        let profile_evidence = observed_profile_evidence(
            profile,
            scenario_completion(&self.events),
            &self.events,
            &self.samples,
            self.state_root.as_ref(),
        );
        build_gate_evidence(
            profile,
            self.checks,
            &self.events,
            &self.samples,
            profile_evidence,
            self.launch_isolation,
        )
    }
}

#[derive(Clone, Copy)]
struct ScenarioCompletion {
    request_id: u64,
    passed: bool,
    semantic_assertions_proven: bool,
    completed_steps: u32,
}

#[cfg(target_os = "linux")]
fn scenario_completion(events: &[ObserverEvent]) -> Option<ScenarioCompletion> {
    events.iter().rev().find_map(|event| match event {
        ObserverEvent::TestCompleted {
            request_id,
            passed,
            semantic_assertions_proven,
            completed_steps,
            ..
        } => Some(ScenarioCompletion {
            request_id: *request_id,
            passed: *passed,
            semantic_assertions_proven: *semantic_assertions_proven,
            completed_steps: *completed_steps,
        }),
        _ => None,
    })
}

fn profile_evidence(
    profile: &VerifierProfile,
    completion: Option<ScenarioCompletion>,
) -> VerificationProfileEvidence {
    profile_evidence_with_observations(profile, completion, Vec::new(), None, None, Vec::new())
}

fn profile_evidence_with_observations(
    profile: &VerifierProfile,
    completion: Option<ScenarioCompletion>,
    budget_observations: Vec<BudgetObservation>,
    state_root: Option<StateRootProof>,
    native_workflow: Option<NativeWorkflowProof>,
    checkpoints: Vec<StateCheckpointProof>,
) -> VerificationProfileEvidence {
    VerificationProfileEvidence {
        profile_id: profile.id.clone(),
        profile_digest: profile.digest.clone(),
        scenario: profile.scenario_proof.as_deref().and_then(|path| {
            scenario_proof(
                path,
                completion,
                !profile.required_native_workflow_steps.is_empty(),
            )
            .ok()
        }),
        budget: profile
            .loaded_budget
            .as_ref()
            .map(|budget| budget_proof(budget, budget_observations)),
        state_root,
        native_workflow,
        checkpoints,
    }
}

#[cfg(target_os = "linux")]
fn observed_profile_evidence(
    profile: &VerifierProfile,
    completion: Option<ScenarioCompletion>,
    events: &[ObserverEvent],
    samples: &ProductSamples,
    state_root: Option<&CapturedStateRoot>,
) -> VerificationProfileEvidence {
    profile_evidence_with_observations(
        profile,
        completion,
        budget_observations(profile, events, samples).unwrap_or_default(),
        state_root.map(|state| StateRootProof {
            root: state.path.to_string_lossy().into_owned(),
            policy: profile
                .state_root_policy
                .clone()
                .unwrap_or_else(|| "undeclared".to_owned()),
            clean_at_start: state.clean_at_start,
            durable_file_count: count_regular_files(&state.path),
            restart_count: state.restart_count,
            restored_after_restart: state.restored_after_restart,
        }),
        native_workflow_proof(profile, events),
        checkpoint_proofs(profile, completion, events),
    )
}

#[cfg(target_os = "linux")]
fn native_workflow_proof(
    profile: &VerifierProfile,
    events: &[ObserverEvent],
) -> Option<NativeWorkflowProof> {
    if profile.required_native_workflow_steps.is_empty() {
        return None;
    }
    let (test_request_id, initial_state_digest, ready_key) =
        events.iter().find_map(|event| match event {
            ObserverEvent::NativeWorkflowReady {
                test_request_id,
                state_digest,
                key,
                ..
            } => Some((*test_request_id, state_digest.clone(), key.clone())),
            _ => None,
        })?;
    let (completed_request_id, step_count, completed_initial, final_state_digest, final_key) =
        events.iter().rev().find_map(|event| match event {
            ObserverEvent::NativeWorkflowCompleted {
                test_request_id,
                step_count,
                initial_state_digest,
                final_state_digest,
                key,
            } => Some((
                *test_request_id,
                *step_count,
                initial_state_digest.clone(),
                final_state_digest.clone(),
                key.clone(),
            )),
            _ => None,
        })?;
    if completed_request_id != test_request_id
        || completed_initial != initial_state_digest
        || step_count as usize != profile.required_native_workflow_steps.len()
    {
        return None;
    }
    let mut steps = Vec::with_capacity(profile.required_native_workflow_steps.len());
    for (index, required) in profile.required_native_workflow_steps.iter().enumerate() {
        let ordinal = u32::try_from(index + 1).ok()?;
        let step = events.iter().find_map(|event| match event {
            ObserverEvent::NativeWorkflowStep {
                request_id,
                ordinal: observed,
                step_id,
                source_path,
                action_kind,
                action_digest,
                input_first_sequence,
                input_last_sequence,
                input_event_count,
                input_event_digest,
                assertion_count,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                durable_acked,
                before_state_digest,
                state_digest,
                key,
                ..
            } if *observed == ordinal && step_id == required => Some(NativeWorkflowStepProof {
                request_id: *request_id,
                ordinal,
                scenario_step: step_id.clone(),
                source_path: source_path.clone(),
                action_kind: action_kind.clone(),
                action_digest: action_digest.clone(),
                input_first_sequence: *input_first_sequence,
                input_last_sequence: *input_last_sequence,
                input_event_count: *input_event_count,
                input_event_digest: input_event_digest.clone(),
                assertion_count: *assertion_count,
                source_revision: *source_revision,
                runtime_sequence: *runtime_sequence,
                durable_epoch: *durable_epoch,
                durable_turn_sequence: *durable_turn_sequence,
                durable_acked: *durable_acked,
                before_state_digest: before_state_digest.clone(),
                state_digest: state_digest.clone(),
                frame: key.clone().into(),
            }),
            _ => None,
        })?;
        steps.push(step);
    }
    Some(NativeWorkflowProof {
        input_delivery: "native-os-app-window-callback",
        scenario_boundary: "kernel-uinput-and-semantic-assertions",
        test_request_id,
        initial_state_digest,
        final_state_digest,
        ready_frame: ready_key.into(),
        final_frame: final_key.into(),
        steps,
    })
}

fn scenario_proof(
    path: &Path,
    completion: Option<ScenarioCompletion>,
    kernel_uinput_workflow: bool,
) -> Result<ScenarioProof, String> {
    let filesystem_path = resolve_profile_input(path);
    let source = fs::read_to_string(&filesystem_path)
        .map_err(|error| format!("read scenario {}: {error}", filesystem_path.display()))?;
    let value = toml::from_str::<toml::Value>(&source)
        .map_err(|error| format!("parse scenario {}: {error}", filesystem_path.display()))?;
    let steps = value
        .get("step")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("scenario {} has no steps", path.display()))?;
    let executable_steps = boon_runtime::parse_scenario(&filesystem_path)
        .map_err(|error| format!("load scenario semantics {}: {error}", path.display()))?
        .steps
        .into_iter()
        .filter(|step| step.source_event.is_some() || !step.expectations.is_empty())
        .count();
    Ok(ScenarioProof {
        path: path.to_string_lossy().into_owned(),
        sha256: sha256(source.as_bytes()),
        boundary: if kernel_uinput_workflow
            && completion.is_some_and(|value| value.semantic_assertions_proven)
        {
            "kernel-uinput-workflow-and-semantic-assertions"
        } else if completion.is_some_and(|value| value.semantic_assertions_proven) {
            "native-test-playback-and-semantic-assertions"
        } else {
            "native-test-playback"
        },
        request_id: completion.map(|value| value.request_id),
        declared_steps: steps.len().try_into().unwrap_or(u32::MAX),
        executable_steps: executable_steps.try_into().unwrap_or(u32::MAX),
        completed_steps: completion.map_or(0, |value| value.completed_steps),
        passed: completion.is_some_and(|value| value.passed),
        semantic_assertions_proven: completion
            .is_some_and(|value| value.semantic_assertions_proven),
    })
}

fn budget_proof(
    budget: &LoadedBudgetContract,
    observations: Vec<BudgetObservation>,
) -> BudgetProof {
    BudgetProof {
        path: budget.declared_path.to_string_lossy().into_owned(),
        sha256: sha256(budget.source.as_bytes()),
        observations,
    }
}

#[cfg(target_os = "linux")]
fn count_regular_files(root: &Path) -> u32 {
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0_u32;
    while let Some(path) = pending.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() {
                count = count.saturating_add(1);
            } else if kind.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    count
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct ProfileStageSample {
    ordinal: u32,
    total_us: u64,
    parent_dispatch_us: u64,
    parent_executor_us: u64,
    parent_runtime_document_us: u64,
    parent_persistence_us: u64,
    compile_us: u64,
    completion_us: u64,
    completion_executor_us: u64,
    completion_runtime_document_us: u64,
    completion_persistence_us: u64,
    document_us: u64,
    interaction_us: u64,
    demand_us: u64,
    present_us: u64,
    patch_count: u32,
    full_lowered: bool,
}

#[cfg(target_os = "linux")]
fn profile_stage_summary(events: &[ObserverEvent]) -> Option<String> {
    let mut samples = events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::ProfileSample {
                ordinal,
                input_sequence,
                callback_to_host_ns,
                preview_visible_us,
                compile_us,
                parent_dispatch_us,
                parent_executor_us,
                parent_runtime_document_us,
                parent_persistence_us,
                completion_us,
                completion_executor_us,
                completion_runtime_document_us,
                completion_persistence_us,
                document_us,
                interaction_us,
                demand_us,
                present_us,
                patch_count,
                full_lowered,
                editor_key,
                key,
                ..
            } if profile_frame_chain_is_exact(
                events,
                *input_sequence,
                *callback_to_host_ns,
                editor_key,
                key,
            ) =>
            {
                Some(ProfileStageSample {
                    ordinal: *ordinal,
                    total_us: *preview_visible_us,
                    parent_dispatch_us: *parent_dispatch_us,
                    parent_executor_us: *parent_executor_us,
                    parent_runtime_document_us: *parent_runtime_document_us,
                    parent_persistence_us: *parent_persistence_us,
                    compile_us: *compile_us,
                    completion_us: *completion_us,
                    completion_executor_us: *completion_executor_us,
                    completion_runtime_document_us: *completion_runtime_document_us,
                    completion_persistence_us: *completion_persistence_us,
                    document_us: *document_us,
                    interaction_us: *interaction_us,
                    demand_us: *demand_us,
                    present_us: *present_us,
                    patch_count: *patch_count,
                    full_lowered: *full_lowered,
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    samples.sort_unstable_by_key(|sample| sample.ordinal);
    samples.dedup_by_key(|sample| sample.ordinal);
    if samples.len() != 120
        || samples
            .iter()
            .enumerate()
            .any(|(index, sample)| sample.ordinal as usize != index + 1)
    {
        return None;
    }
    let measured = &samples[10..];
    let summary = |field: fn(&ProfileStageSample) -> u64| {
        let mut values = measured.iter().map(field).collect::<Vec<_>>();
        values.sort_unstable();
        (
            nearest_rank(&values, 95),
            values.last().copied().unwrap_or(0),
        )
    };
    let total = summary(|sample| sample.total_us);
    let parent = summary(|sample| sample.parent_dispatch_us);
    let parent_executor = summary(|sample| sample.parent_executor_us);
    let parent_runtime_document = summary(|sample| sample.parent_runtime_document_us);
    let parent_persistence = summary(|sample| sample.parent_persistence_us);
    let compile = summary(|sample| sample.compile_us);
    let completion = summary(|sample| sample.completion_us);
    let completion_executor = summary(|sample| sample.completion_executor_us);
    let completion_runtime_document = summary(|sample| sample.completion_runtime_document_us);
    let completion_persistence = summary(|sample| sample.completion_persistence_us);
    let document = summary(|sample| sample.document_us);
    let interaction = summary(|sample| sample.interaction_us);
    let demand = summary(|sample| sample.demand_us);
    let present = summary(|sample| sample.present_us);
    let patches = summary(|sample| u64::from(sample.patch_count));
    let full_lowers = measured.iter().filter(|sample| sample.full_lowered).count();
    Some(format!(
        "110 measured samples after 10 warmup; p95/max us: total={}/{}, parent_dispatch={}/{}, parent_executor={}/{}, parent_runtime_document={}/{}, parent_persistence={}/{}, compile={}/{}, child_completion={}/{}, completion_executor={}/{}, completion_runtime_document={}/{}, completion_persistence={}/{}, retained_document={}/{}, interaction={}/{}, demands={}/{}, present={}/{}; patches p95/max={}/{}; full_lowers={full_lowers}",
        total.0,
        total.1,
        parent.0,
        parent.1,
        parent_executor.0,
        parent_executor.1,
        parent_runtime_document.0,
        parent_runtime_document.1,
        parent_persistence.0,
        parent_persistence.1,
        compile.0,
        compile.1,
        completion.0,
        completion.1,
        completion_executor.0,
        completion_executor.1,
        completion_runtime_document.0,
        completion_runtime_document.1,
        completion_persistence.0,
        completion_persistence.1,
        document.0,
        document.1,
        interaction.0,
        interaction.1,
        demand.0,
        demand.1,
        present.0,
        present.1,
        patches.0,
        patches.1,
    ))
}

#[cfg(target_os = "linux")]
fn budget_observations(
    profile: &VerifierProfile,
    events: &[ObserverEvent],
    samples: &ProductSamples,
) -> Result<Vec<BudgetObservation>, String> {
    const PROFILE_SAMPLES: usize = 120;
    let Some(budget) = profile.loaded_budget.as_ref() else {
        return Ok(Vec::new());
    };
    let mut observations = Vec::new();
    let scroll = preview_scroll_frames(events, &samples.scroll);
    if scroll.len() >= 140 {
        let mut values = scroll
            .iter()
            .skip(20)
            .map(|sample| sample.input_to_present_us)
            .collect::<Vec<_>>();
        values.sort_unstable();
        observations.push(budget_observation(
            budget,
            "passive-preview-scroll-p95",
            BudgetUnit::Microseconds,
            nearest_rank(&values, 95),
        )?);
    }

    let mut profile_samples = events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::ProfileSample {
                ordinal,
                input_sequence,
                callback_to_host_ns,
                editor_visible_us,
                preview_visible_us,
                compile_us,
                interaction_frame_block_us,
                pending_child_artifacts,
                pending_program_artifact_stores,
                pending_program_artifact_loads,
                pending_persistence_artifact_stores,
                pending_persistence_artifact_loads,
                pending_durable_turns,
                trusted_parent_rebuilds,
                editor_key,
                key,
                ..
            } if profile_frame_chain_is_exact(
                events,
                *input_sequence,
                *callback_to_host_ns,
                editor_key,
                key,
            ) =>
            {
                Some((
                    *ordinal,
                    [
                        *editor_visible_us,
                        *preview_visible_us,
                        *compile_us,
                        *interaction_frame_block_us,
                        u64::from(*pending_child_artifacts),
                        u64::from(*pending_program_artifact_stores),
                        u64::from(*pending_program_artifact_loads),
                        u64::from(*pending_persistence_artifact_stores),
                        u64::from(*pending_persistence_artifact_loads),
                        u64::from(*pending_durable_turns),
                        u64::from(*trusted_parent_rebuilds),
                    ],
                ))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    profile_samples.sort_unstable_by_key(|sample| sample.0);
    profile_samples.dedup_by_key(|sample| sample.0);
    let complete_ordinals = profile_samples.len() == PROFILE_SAMPLES
        && profile_samples
            .iter()
            .enumerate()
            .all(|(index, sample)| sample.0 as usize == index + 1);
    if !complete_ordinals {
        return Ok(observations);
    }
    let Some(representative_key) = events.iter().find_map(|event| match event {
        ObserverEvent::ProfileSample {
            ordinal: 11, key, ..
        } => Some(key),
        _ => None,
    }) else {
        return Ok(observations);
    };
    if exact_proof_for_key(events, representative_key).is_none() {
        return Ok(observations);
    }
    let measured = &profile_samples[10..];
    let sorted = |index: usize| {
        let mut values = measured
            .iter()
            .map(|sample| sample.1[index])
            .collect::<Vec<_>>();
        values.sort_unstable();
        values
    };
    let editor = sorted(0);
    let preview = sorted(1);
    let compile = sorted(2);
    observations.extend([
        budget_observation(
            budget,
            "keystroke-to-editor-visible-p95",
            BudgetUnit::Microseconds,
            nearest_rank(&editor, 95),
        )?,
        budget_observation(
            budget,
            "valid-edit-to-preview-visible-p95",
            BudgetUnit::Microseconds,
            nearest_rank(&preview, 95),
        )?,
        budget_observation(
            budget,
            "valid-edit-to-preview-visible-p99",
            BudgetUnit::Microseconds,
            nearest_rank(&preview, 99),
        )?,
        budget_observation(
            budget,
            "bounded-starter-source-compile-p95",
            BudgetUnit::Microseconds,
            nearest_rank(&compile, 95),
        )?,
        budget_observation(
            budget,
            "bounded-starter-source-compile-max",
            BudgetUnit::Microseconds,
            compile.last().copied().unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "interaction-frame-block-max",
            BudgetUnit::Microseconds,
            measured
                .iter()
                .map(|sample| sample.1[3])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "pending-child-artifact-max",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[4])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "pending-program-artifact-store-max",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[5])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "pending-program-artifact-load-max",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[6])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "pending-persistence-artifact-store-max",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[7])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "pending-persistence-artifact-load-max",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[8])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "pending-durable-turn-max",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[9])
                .max()
                .unwrap_or(u64::MAX),
        )?,
        budget_observation(
            budget,
            "proof-replacement-max",
            BudgetUnit::Count,
            events
                .iter()
                .filter_map(|event| match event {
                    ObserverEvent::ProofCompleted { replaced_count, .. } => Some(*replaced_count),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
        )?,
        budget_observation(
            budget,
            "proof-result-drop-max",
            BudgetUnit::Count,
            events
                .iter()
                .filter_map(|event| match event {
                    ObserverEvent::ProofCompleted {
                        result_drop_count, ..
                    } => Some(*result_drop_count),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
        )?,
        budget_observation(
            budget,
            "trusted-parent-rebuilds-per-edit",
            BudgetUnit::Count,
            measured
                .iter()
                .map(|sample| sample.1[10])
                .max()
                .unwrap_or(u64::MAX),
        )?,
    ]);
    Ok(observations)
}

fn budget_observation(
    budget: &LoadedBudgetContract,
    metric: &str,
    expected_unit: BudgetUnit,
    observed: u64,
) -> Result<BudgetObservation, String> {
    let limit = budget.contract.limit(metric)?;
    if limit.unit != expected_unit {
        return Err(format!(
            "budget metric `{metric}` has unit {}, expected {}",
            budget_unit_name(limit.unit),
            budget_unit_name(expected_unit),
        ));
    }
    Ok(BudgetObservation {
        metric: metric.to_owned(),
        unit: budget_unit_name(limit.unit),
        comparison: "at-most",
        observed,
        limit: limit.at_most,
    })
}

fn budget_unit_name(unit: BudgetUnit) -> &'static str {
    match unit {
        BudgetUnit::Microseconds => "microseconds",
        BudgetUnit::Bytes => "bytes",
        BudgetUnit::Count => "count",
    }
}

fn observed_budget_unit(metric: &str) -> Option<BudgetUnit> {
    match metric {
        "keystroke-to-editor-visible-p95"
        | "valid-edit-to-preview-visible-p95"
        | "valid-edit-to-preview-visible-p99"
        | "bounded-starter-source-compile-p95"
        | "bounded-starter-source-compile-max"
        | "passive-preview-scroll-p95"
        | "interaction-frame-block-max" => Some(BudgetUnit::Microseconds),
        "pending-child-artifact-max"
        | "pending-program-artifact-store-max"
        | "pending-program-artifact-load-max"
        | "pending-persistence-artifact-store-max"
        | "pending-persistence-artifact-load-max"
        | "pending-durable-turn-max"
        | "proof-replacement-max"
        | "proof-result-drop-max"
        | "trusted-parent-rebuilds-per-edit" => Some(BudgetUnit::Count),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn checkpoint_proofs(
    profile: &VerifierProfile,
    completion: Option<ScenarioCompletion>,
    events: &[ObserverEvent],
) -> Vec<StateCheckpointProof> {
    profile
        .required_checkpoints
        .iter()
        .filter_map(|required| {
            checkpoint_proof(profile, required, completion, events).filter(|checkpoint| {
                exact_proof_for_key(events, &checkpoint.frame.clone().into()).is_some()
            })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn checkpoint_proof(
    profile: &VerifierProfile,
    required: &VerifierCheckpointRequirement,
    completion: Option<ScenarioCompletion>,
    events: &[ObserverEvent],
) -> Option<StateCheckpointProof> {
    match &required.evidence {
        VerifierCheckpointRequirementKind::ScenarioStep { scenario_step } => {
            events.iter().rev().find_map(|event| match event {
                ObserverEvent::ScenarioCheckpoint {
                    request_id,
                    step_id,
                    assertion_count,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    state_digest,
                    key,
                } if step_id == scenario_step
                    && completion
                        .is_some_and(|completion| completion.request_id == *request_id)
                    && *assertion_count > 0
                    && *durable_turn_sequence > 0 =>
                {
                    Some(StateCheckpointProof {
                        id: required.id.clone(),
                        source_revision: *source_revision,
                        runtime_sequence: *runtime_sequence,
                        durable_epoch: *durable_epoch,
                        durable_turn_sequence: *durable_turn_sequence,
                        state_digest: state_digest.clone(),
                        frame: key.clone().into(),
                        evidence: StateCheckpointEvidence::ScenarioSemanticFrame {
                            scenario_step: step_id.clone(),
                            assertion_count: *assertion_count,
                        },
                    })
                }
                _ => None,
            })
        }
        VerifierCheckpointRequirementKind::ResponsiveLayout {
            baseline_checkpoint,
            logical_width,
        } => events.iter().rev().find_map(|event| match event {
            ObserverEvent::ResponsiveLayoutEvidence {
                logical_width: observed_width,
                logical_height: observed_height,
                action_count,
                action_digest,
                state_digest,
                source_revision,
                runtime_sequence,
                durable_epoch,
                durable_turn_sequence,
                key,
                ..
            } if observed_width == logical_width && *action_count > 0 => {
                Some(StateCheckpointProof {
                    id: required.id.clone(),
                    source_revision: *source_revision,
                    runtime_sequence: *runtime_sequence,
                    durable_epoch: *durable_epoch,
                    durable_turn_sequence: *durable_turn_sequence,
                    state_digest: state_digest.clone(),
                    frame: key.clone().into(),
                    evidence: StateCheckpointEvidence::ResponsiveLayout {
                        baseline_checkpoint: baseline_checkpoint.clone(),
                        logical_width: *observed_width,
                        logical_height: *observed_height,
                        action_count: *action_count,
                        action_digest: action_digest.clone(),
                    },
                })
            }
            _ => None,
        }),
        VerifierCheckpointRequirementKind::StaleCompileRejection => {
            events.iter().rev().find_map(|event| match event {
                ObserverEvent::StaleProgramRejected {
                    session,
                    stale_revision,
                    latest_revision,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    state_digest,
                    key,
                } if stale_revision < latest_revision && *durable_turn_sequence > 0 => {
                    Some(StateCheckpointProof {
                        id: required.id.clone(),
                        source_revision: *source_revision,
                        runtime_sequence: *runtime_sequence,
                        durable_epoch: *durable_epoch,
                        durable_turn_sequence: *durable_turn_sequence,
                        state_digest: state_digest.clone(),
                        frame: key.clone().into(),
                        evidence: StateCheckpointEvidence::StaleCompileRejection {
                            session: session.clone(),
                            stale_revision: *stale_revision,
                            latest_revision: *latest_revision,
                        },
                    })
                }
                _ => None,
            })
        }
        VerifierCheckpointRequirementKind::PersistenceOperation { operation } => {
            let expected = persistence_event_kind(*operation);
            events.iter().rev().find_map(|event| match event {
                ObserverEvent::PersistenceEvidence {
                    kind,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    before_state_digest,
                    after_state_digest,
                    key,
                } if *kind == expected && *durable_turn_sequence > 0 => {
                    Some(StateCheckpointProof {
                        id: required.id.clone(),
                        source_revision: *source_revision,
                        runtime_sequence: *runtime_sequence,
                        durable_epoch: *durable_epoch,
                        durable_turn_sequence: *durable_turn_sequence,
                        state_digest: after_state_digest.clone(),
                        frame: key.clone().into(),
                        evidence: StateCheckpointEvidence::PersistenceOperation {
                            operation: *operation,
                            before_state_digest: before_state_digest.clone(),
                        },
                    })
                }
                _ => None,
            })
        }
        VerifierCheckpointRequirementKind::RestartRestore {
            baseline_checkpoint,
        } => {
            let baseline_requirement = profile.required_checkpoints.iter().find(|checkpoint| {
                checkpoint.id == *baseline_checkpoint
                    && matches!(
                        &checkpoint.evidence,
                        VerifierCheckpointRequirementKind::ScenarioStep { .. }
                            | VerifierCheckpointRequirementKind::NativeWorkflowStep { .. }
                    )
            })?;
            let baseline = checkpoint_proof(profile, baseline_requirement, completion, events)?;
            events.iter().rev().find_map(|event| match event {
                ObserverEvent::StateMounted {
                    disposition: StartupDisposition::Restored,
                    migration: None,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    state_digest,
                    key,
                    ..
                } if *durable_turn_sequence > 0 && state_digest == &baseline.state_digest => {
                    let first_observable_frame = events.iter().find_map(|event| match event {
                        ObserverEvent::FramePresented(frame)
                            if frame.role == ObserverRole::Preview
                                && frame.key.process_id == key.process_id =>
                        {
                            Some(frame.key.clone())
                        }
                        _ => None,
                    });
                    Some(StateCheckpointProof {
                        id: required.id.clone(),
                        source_revision: *source_revision,
                        runtime_sequence: *runtime_sequence,
                        durable_epoch: *durable_epoch,
                        durable_turn_sequence: *durable_turn_sequence,
                        state_digest: state_digest.clone(),
                        frame: key.clone().into(),
                        evidence: StateCheckpointEvidence::RestartRestore {
                            baseline_checkpoint: baseline_checkpoint.clone(),
                            before_restart_digest: baseline.state_digest.clone(),
                            baseline_durable_epoch: baseline.durable_epoch,
                            baseline_durable_turn_sequence: baseline.durable_turn_sequence,
                            baseline_frame: baseline.frame.clone(),
                            process_replaced: baseline.frame.process_id != key.process_id,
                            session_replaced: baseline.frame.session_id != key.session_id,
                            first_observable_frame: first_observable_frame.as_ref() == Some(key),
                            startup_restored: true,
                        },
                    })
                }
                _ => None,
            })
        }
        VerifierCheckpointRequirementKind::NativeWorkflowStep { scenario_step } => {
            events.iter().rev().find_map(|event| match event {
                ObserverEvent::NativeWorkflowStep {
                    request_id,
                    step_id,
                    action_kind,
                    action_digest,
                    input_first_sequence,
                    input_last_sequence,
                    input_event_count,
                    input_event_digest,
                    assertion_count,
                    source_revision,
                    runtime_sequence,
                    durable_epoch,
                    durable_turn_sequence,
                    durable_acked,
                    state_digest,
                    key,
                    ..
                } if step_id == scenario_step && *request_id > 0 && *assertion_count > 0 => {
                    Some(StateCheckpointProof {
                        id: required.id.clone(),
                        source_revision: *source_revision,
                        runtime_sequence: *runtime_sequence,
                        durable_epoch: *durable_epoch,
                        durable_turn_sequence: *durable_turn_sequence,
                        state_digest: state_digest.clone(),
                        frame: key.clone().into(),
                        evidence: StateCheckpointEvidence::NativeWorkflowFrame {
                            scenario_step: step_id.clone(),
                            action_kind: action_kind.clone(),
                            request_id: *request_id,
                            action_digest: action_digest.clone(),
                            input_first_sequence: *input_first_sequence,
                            input_last_sequence: *input_last_sequence,
                            input_event_count: *input_event_count,
                            input_event_digest: input_event_digest.clone(),
                            durable_acked: *durable_acked,
                            assertion_count: *assertion_count,
                        },
                    })
                }
                _ => None,
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn persistence_event_kind(operation: VerifierPersistenceOperation) -> PersistenceEvidenceKind {
    match operation {
        VerifierPersistenceOperation::Exported => PersistenceEvidenceKind::Exported,
        VerifierPersistenceOperation::CorruptionRejected => {
            PersistenceEvidenceKind::CorruptionRejected
        }
        VerifierPersistenceOperation::ClearedAndStartedOver => {
            PersistenceEvidenceKind::ClearedAndStartedOver
        }
        VerifierPersistenceOperation::ImportPreviewed => PersistenceEvidenceKind::ImportPreviewed,
        VerifierPersistenceOperation::ImportActivated => PersistenceEvidenceKind::ImportActivated,
        VerifierPersistenceOperation::MigrationActivated => {
            PersistenceEvidenceKind::MigrationActivated
        }
    }
}

fn resolve_profile_input(path: &Path) -> PathBuf {
    if path.is_absolute() || path.is_file() {
        return path.to_path_buf();
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("native playground lives at crates/boon_native_playground")
        .join(path)
}

fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(target_os = "linux")]
fn build_gate_evidence(
    profile: &VerifierProfile,
    checks: Vec<Check>,
    events: &[ObserverEvent],
    samples: &ProductSamples,
    profile_evidence: VerificationProfileEvidence,
    launch_isolation: Vec<LaunchIsolationEvidence>,
) -> GateEvidence {
    let metadata = role_metadata(events);
    let proofs = exact_proofs(events);
    let native = metadata
        .get(&ObserverRole::Preview)
        .zip(metadata.get(&ObserverRole::Dev))
        .and_then(|(preview, dev)| {
            native_evidence(
                preview,
                dev.pid,
                launch_isolation,
                proofs.first()?.artifact.capture_method.clone(),
            )
        });
    let mut product_ux_timings = Vec::new();

    let callback_sequences = samples.callback_sequences();
    let callbacks = callback_samples(events, &callback_sequences);
    let visible = preview_visible_frames(events, &samples.visible);
    let proof = proofs
        .iter()
        .find(|proof| visible.iter().any(|frame| frame.key == proof.key))
        .cloned();
    let proof_frame = proof.as_ref().map(|proof| &proof.key);
    if callbacks.len() >= 70
        && let Some(representative) = representative_callback(&callbacks, &visible, proof_frame)
    {
        let values = callbacks
            .iter()
            .skip(10)
            .map(|sample| sample.input.callback_to_host_ns / 1_000)
            .collect::<Vec<_>>();
        product_ux_timings.push(ProductTimingEvidence {
            metric: "callback-to-host-event",
            representative_frame: representative.frame.key.clone().into(),
            representative_sample_ordinal: representative.ordinal,
            summary: TimingSummary::from_values(&values, 1_000),
        });
    }
    if visible.len() >= 70 {
        let representative_index = proof_frame
            .and_then(|key| visible.iter().position(|frame| &frame.key == key))
            .filter(|index| *index >= 10)
            .unwrap_or(10);
        let values = visible
            .iter()
            .skip(10)
            .map(|sample| sample.input_to_present_us)
            .collect::<Vec<_>>();
        product_ux_timings.push(ProductTimingEvidence {
            metric: "warm-visible-interaction",
            representative_frame: visible[representative_index].key.clone().into(),
            representative_sample_ordinal: (representative_index + 1)
                .try_into()
                .unwrap_or(u32::MAX),
            summary: TimingSummary::from_values(&values, 16_700),
        });
    }
    if profile.selection_samples > 0 {
        let clicks = preview_visible_frames(events, &samples.clicks);
        if clicks.len() >= 24 {
            let values = clicks
                .iter()
                .skip(4)
                .map(|sample| sample.input_to_present_us)
                .collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "repeated-selection",
                representative_frame: clicks[4].key.clone().into(),
                representative_sample_ordinal: 5,
                summary: TimingSummary::from_values(&values, 16_700),
            });
        }
    }
    if profile.scroll_samples > 0 {
        let scroll = preview_scroll_frames(events, &samples.scroll);
        if scroll.len() >= 140 {
            let representative = &scroll[20];
            if scroll_proof_key(events, 21) == Some(&representative.key) {
                let values = scroll
                    .iter()
                    .skip(20)
                    .map(|sample| sample.input_to_present_us)
                    .collect::<Vec<_>>();
                product_ux_timings.push(ProductTimingEvidence {
                    metric: "warm-scroll",
                    representative_frame: representative.key.clone().into(),
                    representative_sample_ordinal: 21,
                    summary: TimingSummary::from_values(&values, 16_700),
                });
            }
        }
    }
    if profile.switch_samples > 0 {
        let acknowledgements = switch_ack_samples(events);
        let final_samples = switch_final_samples(events);
        if acknowledgements.len() >= 23 && final_samples.len() >= 23 {
            let ack_values = acknowledgements.iter().skip(3).copied().collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "example-switch-acknowledgement",
                representative_frame: final_samples[3].key.clone().into(),
                representative_sample_ordinal: 4,
                summary: TimingSummary::from_values(&ack_values, 16_700),
            });
            let final_values = final_samples
                .iter()
                .skip(3)
                .map(|sample| sample.elapsed_us)
                .collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "example-switch-final-preview",
                representative_frame: final_samples[3].key.clone().into(),
                representative_sample_ordinal: 4,
                summary: TimingSummary::from_values(&final_values, 250_000),
            });
        }
    }

    let artifacts = proofs
        .iter()
        .enumerate()
        .map(|(index, proof)| ArtifactMetadata {
            artifact_id: format!("proof-{index}-frame-{}", proof.key.frame_id),
            kind: "wgpu-png-readback",
            path: proof.artifact.path.clone(),
            sha256: proof.artifact.sha256.clone(),
            byte_len: proof.artifact.byte_len,
            capture_method: proof.artifact.capture_method.clone(),
            capture_token_digest: proof.artifact.capture_token_digest.clone(),
            nonblank_samples: proof.artifact.nonblank_samples,
            unique_rgba_values: proof.artifact.unique_rgba_values,
            frame: proof.key.clone().into(),
        })
        .collect::<Vec<_>>();
    let async_proof_timing = proof.map(|proof| {
        let artifact_id = artifacts
            .iter()
            .find(|artifact| artifact.frame == proof.key.clone().into())
            .map(|artifact| artifact.artifact_id.clone())
            .unwrap_or_else(|| format!("missing-proof-frame-{}", proof.key.frame_id));
        let completed_after = proof.completed_after_key.clone();
        let lag = completed_after.frame_id.saturating_sub(proof.key.frame_id);
        AsyncProofTimingEvidence {
            linked_product_metric: "warm-visible-interaction",
            captured_frame: proof.key.clone().into(),
            completed_after_frame: completed_after.into(),
            proof_lag_frames: lag.try_into().unwrap_or(u32::MAX),
            artifact_id: artifact_id.clone(),
            snapshot_prepare_us: proof.snapshot_prepare_us,
            queue_wait_us: proof.queue_wait_us,
            worker_us: proof.worker_us,
            apply_us: proof.apply_us,
            summary: TimingSummary::from_values(
                &[proof
                    .snapshot_prepare_us
                    .saturating_add(proof.queue_wait_us)
                    .saturating_add(proof.worker_us)
                    .saturating_add(proof.apply_us)],
                500_000,
            ),
        }
    });

    GateEvidence {
        checks,
        producer: None,
        profile: Some(profile_evidence),
        native,
        product_ux_timings,
        async_proof_timing,
        async_lanes: async_lane_evidence(events),
        artifacts,
    }
}

#[cfg(target_os = "linux")]
fn scroll_proof_key(events: &[ObserverEvent], ordinal: u32) -> Option<&FrameEvidenceKey> {
    events.iter().find_map(|event| match event {
        ObserverEvent::ScrollProofFrame {
            ordinal: observed,
            key,
        } if *observed == ordinal
            && frame_key_matches_metadata(events, key, ObserverRole::Preview)
            && exact_proof_for_key(events, key).is_some() =>
        {
            Some(key)
        }
        _ => None,
    })
}

#[cfg(target_os = "linux")]
fn native_evidence(
    metadata: &RoleMetadata,
    dev_pid: u32,
    launch_isolation: Vec<LaunchIsolationEvidence>,
    capture_method: String,
) -> Option<NativeEvidence> {
    let adapter_backend = match metadata.adapter_backend.as_str() {
        "vulkan" | "metal" | "dx12" | "gl" => metadata.adapter_backend.clone(),
        _ => return None,
    };
    let adapter_device_type = match metadata.adapter_device_type.as_str() {
        "integrated-gpu" | "discrete-gpu" | "virtual-gpu" | "cpu" | "other" => {
            metadata.adapter_device_type.clone()
        }
        _ => return None,
    };
    let present_mode = match metadata.present_mode.as_str() {
        "fifo" | "fifo-relaxed" | "immediate" | "mailbox" | "auto-vsync" | "auto-no-vsync" => {
            metadata.present_mode.clone()
        }
        _ => return None,
    };
    Some(NativeEvidence {
        adapter_name: metadata.adapter_name.clone(),
        adapter_backend,
        adapter_device_type,
        software_adapter: metadata.software_adapter,
        present_mode,
        surface_format: metadata.surface_format.clone(),
        window_backend: metadata.window_backend.clone(),
        preview_pid: metadata.pid,
        dev_pid,
        input_delivery: "native-os-app-window-callback",
        scenario_boundary: "public-host-event",
        capture_method,
        private_runtime_dispatch_used: false,
        launch_isolation,
    })
}

#[cfg(target_os = "linux")]
fn role_metadata(events: &[ObserverEvent]) -> BTreeMap<ObserverRole, RoleMetadata> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::RoleMetadata(metadata) => Some((metadata.role, metadata.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn frame_key_matches_metadata(
    events: &[ObserverEvent],
    key: &FrameEvidenceKey,
    role: ObserverRole,
) -> bool {
    key.is_complete()
        && events.iter().any(|event| {
            matches!(event, ObserverEvent::RoleMetadata(metadata)
                if metadata.role == role
                    && metadata.pid == key.process_id
                    && metadata.surface_id == key.surface_id
                    && metadata.session_id == key.session_id)
        })
}

#[cfg(target_os = "linux")]
fn frame_key_matches_session(
    events: &[ObserverEvent],
    key: &FrameEvidenceKey,
    session: &NativeSession,
    role: ObserverRole,
) -> bool {
    key.session_id == session.session_id
        && session.observed_roles.contains(&key.process_id)
        && frame_key_matches_metadata(events, key, role)
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct ExactProof {
    key: FrameEvidenceKey,
    completed_after_key: FrameEvidenceKey,
    snapshot_prepare_us: u64,
    queue_wait_us: u64,
    worker_us: u64,
    apply_us: u64,
    artifact: ProofArtifact,
}

#[cfg(target_os = "linux")]
fn exact_proof(events: &[ObserverEvent]) -> Option<ExactProof> {
    exact_proof_matching(events, None)
}

#[cfg(target_os = "linux")]
fn exact_proofs(events: &[ObserverEvent]) -> Vec<ExactProof> {
    let mut keys = Vec::<FrameEvidenceKey>::new();
    for event in events {
        let ObserverEvent::ProofCompleted { key, .. } = event else {
            continue;
        };
        if !keys.contains(key) && exact_proof_for_key(events, key).is_some() {
            keys.push(key.clone());
        }
    }
    keys.into_iter()
        .filter_map(|key| exact_proof_for_key(events, &key))
        .collect()
}

#[cfg(target_os = "linux")]
fn exact_proof_for_key(
    events: &[ObserverEvent],
    required: &FrameEvidenceKey,
) -> Option<ExactProof> {
    exact_proof_matching(events, Some(required))
}

#[cfg(target_os = "linux")]
fn exact_proof_matching(
    events: &[ObserverEvent],
    required: Option<&FrameEvidenceKey>,
) -> Option<ExactProof> {
    events.iter().enumerate().find_map(|(index, event)| {
        let ObserverEvent::ProofCompleted {
            key,
            completed_after_key,
            elapsed_us,
            artifact: Some(artifact),
            error: None,
            ..
        } = event
        else {
            return None;
        };
        if required.is_some_and(|required| required != key) {
            return None;
        }
        let presented_before = events[..index]
            .iter()
            .find_map(|candidate| match candidate {
                ObserverEvent::FramePresented(frame) if frame.key == *key => Some(frame),
                _ => None,
            });
        let snapshot_prepare_us =
            events[..index]
                .iter()
                .enumerate()
                .find_map(|(request_index, candidate)| {
                    match candidate {
                ObserverEvent::ProofRequested {
                    key: requested,
                    snapshot_prepare_us,
                } if requested == key
                    && events[..request_index].iter().any(|prior| {
                        matches!(prior, ObserverEvent::FramePresented(frame) if frame.key == *key)
                    }) => Some(*snapshot_prepare_us),
                _ => None,
            }
                });
        let request_id = format!("proof-{}", key.proof_id);
        let proof_lane = events[index + 1..].iter().find_map(|candidate| {
            let ObserverEvent::AsyncLaneCompleted {
                lane: AsyncLaneKind::ProofReadback,
                request_id: candidate_request_id,
                revision,
                queue_depth,
                queue_wait_us,
                worker_us,
                apply_us,
                end_to_end_us,
                outcome: AsyncLaneOutcome::Applied,
                key: candidate_completed_key,
            } = candidate
            else {
                return None;
            };
            (candidate_request_id == &request_id
                && *revision == key.frame_id
                && *queue_depth > 0
                && *worker_us == *elapsed_us
                && *end_to_end_us
                    >= queue_wait_us
                        .saturating_add(*worker_us)
                        .saturating_add(*apply_us)
                && candidate_completed_key == completed_after_key)
                .then_some((*queue_wait_us, *worker_us, *apply_us))
        });
        (presented_before.is_some_and(|frame| frame_key_matches_metadata(events, key, frame.role))
            && snapshot_prepare_us.is_some()
            && proof_lane.is_some()
            && key.is_complete()
            && completed_after_key.is_complete()
            && key.same_producer_surface(completed_after_key)
            && completed_after_key.frame_id >= key.frame_id
            && completed_after_key.present_id >= key.present_id
            && artifact.byte_len > 0
            && artifact.capture_method == "app-owned-render-target-readback"
            && artifact.capture_token_digest == frame_capture_token_digest(key)
            && artifact.nonblank_samples > 0
            && artifact.unique_rgba_values > 1)
            .then(|| ExactProof {
                key: key.clone(),
                completed_after_key: completed_after_key.clone(),
                snapshot_prepare_us: snapshot_prepare_us.unwrap_or_default(),
                queue_wait_us: proof_lane.unwrap_or_default().0,
                worker_us: proof_lane.unwrap_or_default().1,
                apply_us: proof_lane.unwrap_or_default().2,
                artifact: artifact.clone(),
            })
    })
}

#[cfg(target_os = "linux")]
fn async_lane_evidence(events: &[ObserverEvent]) -> Vec<AsyncLaneEvidence> {
    let mut selected = BTreeMap::<(AsyncLaneKind, AsyncLaneOutcome), AsyncLaneEvidence>::new();
    for event in events {
        let ObserverEvent::AsyncLaneCompleted {
            lane,
            request_id,
            revision,
            queue_depth,
            queue_wait_us,
            worker_us,
            apply_us,
            end_to_end_us,
            outcome,
            key,
        } = event
        else {
            continue;
        };
        if !async_lane_event_is_valid(event, events) {
            continue;
        }
        let candidate = AsyncLaneEvidence {
            lane: async_lane_name(*lane),
            request_id: request_id.clone(),
            revision: *revision,
            queue_depth: *queue_depth,
            queue_wait_us: *queue_wait_us,
            worker_us: *worker_us,
            apply_us: *apply_us,
            end_to_end_us: *end_to_end_us,
            outcome: match outcome {
                AsyncLaneOutcome::Applied => "applied",
                AsyncLaneOutcome::StaleRejected => "stale-rejected",
                AsyncLaneOutcome::Failed => "failed",
            },
            frame: key.clone().into(),
        };
        let selection_key = (*lane, *outcome);
        let replace = selected
            .get(&selection_key)
            .is_none_or(|current| candidate.end_to_end_us > current.end_to_end_us);
        if replace {
            selected.insert(selection_key, candidate);
        }
    }
    selected.into_values().collect()
}

#[cfg(target_os = "linux")]
fn async_lane_event_is_valid(event: &ObserverEvent, events: &[ObserverEvent]) -> bool {
    let ObserverEvent::AsyncLaneCompleted {
        revision,
        queue_depth,
        queue_wait_us,
        worker_us,
        apply_us,
        end_to_end_us,
        key,
        ..
    } = event
    else {
        return false;
    };
    let Some(event_index) = events
        .iter()
        .position(|candidate| std::ptr::eq(candidate, event))
    else {
        return false;
    };
    *revision > 0
        && *queue_depth > 0
        && *end_to_end_us
            >= queue_wait_us
                .saturating_add(*worker_us)
                .saturating_add(*apply_us)
        && frame_key_matches_metadata(events, key, ObserverRole::Preview)
        && events[..event_index].iter().any(
            |candidate| matches!(candidate, ObserverEvent::FramePresented(frame) if frame.role == ObserverRole::Preview && frame.key == *key),
        )
}

#[cfg(target_os = "linux")]
fn async_lane_name(lane: AsyncLaneKind) -> &'static str {
    match lane {
        AsyncLaneKind::ChildProgramCompile => "child-program-compile",
        AsyncLaneKind::PersistenceTurn => "persistence-turn",
        AsyncLaneKind::ProgramArtifactStore => "program-artifact-store",
        AsyncLaneKind::ProgramArtifactLoad => "program-artifact-load",
        AsyncLaneKind::ProofReadback => "proof-readback",
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct CallbackSample {
    role: ObserverRole,
    input: InputAccepted,
}

#[cfg(target_os = "linux")]
fn callback_samples(events: &[ObserverEvent], sequences: &BTreeSet<u64>) -> Vec<CallbackSample> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::InputAccepted(input)
                if input.real_os
                    && input.role == ObserverRole::Preview
                    && sequences.contains(&input.event_sequence)
                    && matches!(
                        input.kind,
                        InputKind::PointerMove
                            | InputKind::PointerButton
                            | InputKind::Wheel
                            | InputKind::Keyboard
                            | InputKind::Text
                    ) =>
            {
                Some(CallbackSample {
                    role: input.role,
                    input: input.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn preview_visible_frames(
    events: &[ObserverEvent],
    sequences: &BTreeSet<u64>,
) -> Vec<FramePresented> {
    real_frames(events, |frame| {
        frame.role == ObserverRole::Preview
            && frame
                .event_sequence
                .is_some_and(|value| sequences.contains(&value))
            && frame
                .input_kind
                .is_some_and(|kind| kind != InputKind::Wheel)
    })
}

#[cfg(target_os = "linux")]
fn preview_scroll_frames(
    events: &[ObserverEvent],
    sequences: &BTreeSet<u64>,
) -> Vec<FramePresented> {
    real_frames(events, |frame| {
        frame.role == ObserverRole::Preview
            && frame
                .event_sequence
                .is_some_and(|value| sequences.contains(&value))
            && frame.input_kind == Some(InputKind::Wheel)
    })
}

#[cfg(target_os = "linux")]
fn real_frames(
    events: &[ObserverEvent],
    predicate: impl Fn(&FramePresented) -> bool,
) -> Vec<FramePresented> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::FramePresented(frame) if predicate(frame) => {
                let real = events.iter().any(|candidate| {
                    matches!(candidate, ObserverEvent::InputAccepted(input)
                        if input.role == frame.role
                            && input.real_os
                            && Some(input.event_sequence) == frame.event_sequence
                            && input.surface_epoch == frame.key.surface_epoch)
                });
                (real && frame_key_matches_metadata(events, &frame.key, frame.role))
                    .then(|| frame.clone())
            }
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn switch_ack_samples(events: &[ObserverEvent]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::SourceSwitchAcknowledged { elapsed_us, .. } => Some(*elapsed_us),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct SwitchFinalSample {
    revision: u64,
    elapsed_us: u64,
    compile_us: u64,
    post_compile_us: u64,
    key: FrameEvidenceKey,
}

#[cfg(target_os = "linux")]
fn switch_final_samples(events: &[ObserverEvent]) -> Vec<SwitchFinalSample> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::SourceSwitchFinal {
                revision,
                elapsed_us,
                compile_us,
                post_compile_us,
                key,
            } => Some(SwitchFinalSample {
                revision: *revision,
                elapsed_us: *elapsed_us,
                compile_us: *compile_us,
                post_compile_us: *post_compile_us,
                key: key.clone(),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn maximum_switch_revision(events: &[ObserverEvent]) -> u64 {
    switch_final_samples(events)
        .into_iter()
        .map(|sample| sample.revision)
        .max()
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
struct RepresentativeCallback<'a> {
    frame: &'a FramePresented,
    ordinal: u32,
}

#[cfg(target_os = "linux")]
fn representative_callback<'a>(
    callbacks: &[CallbackSample],
    frames: &'a [FramePresented],
    preferred: Option<&FrameEvidenceKey>,
) -> Option<RepresentativeCallback<'a>> {
    let frame = preferred
        .and_then(|key| frames.iter().find(|frame| &frame.key == key))
        .or_else(|| frames.get(10))?;
    let sequence = frame.event_sequence?;
    let ordinal = callbacks
        .iter()
        .position(|sample| sample.role == frame.role && sample.input.event_sequence == sequence)?;
    (ordinal >= 10).then(|| RepresentativeCallback {
        frame,
        ordinal: (ordinal + 1).try_into().unwrap_or(u32::MAX),
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn add_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[CallbackSample],
    warmup: usize,
    minimum: usize,
    p95_limit: Option<u64>,
    p99_limit: Option<u64>,
    max_limit: u64,
) {
    let values = samples
        .iter()
        .skip(warmup)
        .map(|sample| sample.input.callback_to_host_ns / 1_000)
        .collect::<Vec<_>>();
    add_summary_check(
        checks, id, &values, minimum, p95_limit, p99_limit, max_limit,
    );
    if values.len() >= minimum
        && let Some((ordinal, worst)) = samples
            .iter()
            .enumerate()
            .skip(warmup)
            .max_by_key(|(_, sample)| sample.input.callback_to_host_ns)
        && let Some(check) = checks.last_mut()
    {
        check.detail = bounded_detail(format!(
            "{}; worst ordinal={} sequence={} kind={:?} callback={}us target={:?}",
            check.detail,
            ordinal + 1,
            worst.input.event_sequence,
            worst.input.kind,
            worst.input.callback_to_host_ns / 1_000,
            worst.input.target,
        ));
    }
}

#[cfg(target_os = "linux")]
fn add_frame_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[FramePresented],
    warmup: usize,
    minimum: usize,
    p95_limit: u64,
    max_limit: u64,
) {
    let values = samples
        .iter()
        .skip(warmup)
        .map(|sample| sample.input_to_present_us)
        .collect::<Vec<_>>();
    add_summary_check(
        checks,
        id,
        &values,
        minimum,
        Some(p95_limit),
        None,
        max_limit,
    );
    if values.len() >= minimum {
        let component_summary = |values: Vec<u64>| TimingSummary::from_values(&values, p95_limit);
        let frame = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.frame_us)
                .collect(),
        );
        let event_dispatch = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.event_dispatch_us)
                .collect(),
        );
        let executor = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.executor_us)
                .collect(),
        );
        let runtime_document = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.runtime_document_us)
                .collect(),
        );
        let document_update = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.document_update_us)
                .collect(),
        );
        let acquire = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| {
                    sample
                        .frame_us
                        .saturating_sub(sample.render_us + sample.submit_us + sample.present_us)
                })
                .collect(),
        );
        let render = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.render_us)
                .collect(),
        );
        let render_component_p95 = |select: fn(&FramePresented) -> u64| {
            component_summary(samples.iter().skip(warmup).map(select).collect::<Vec<_>>()).p95_us
        };
        let scene_convert = render_component_p95(|sample| sample.document_scene_convert_us);
        let scene_key = render_component_p95(|sample| sample.scene_key_us);
        let rect_vertices = render_component_p95(|sample| sample.rect_vertices_us);
        let asset_prepare = render_component_p95(|sample| sample.asset_prepare_us);
        let quad_batch_key = render_component_p95(|sample| sample.quad_batch_key_us);
        let quad_upload = render_component_p95(|sample| sample.quad_upload_us);
        let draw_pass = render_component_p95(|sample| sample.draw_pass_us);
        let retained_metrics = render_component_p95(|sample| sample.retained_metrics_us);
        let text_render = render_component_p95(|sample| sample.text_render_us);
        let submit = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.submit_us)
                .collect(),
        );
        let present = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.present_us)
                .collect(),
        );
        if let Some(check) = checks.last_mut() {
            check.detail = bounded_detail(format!(
                "{}; component p95/max: dispatch={}/{}us executor={}/{}us runtime_document={}/{}us retained={}/{}us frame={}/{}us acquire={}us render={}us submit={}us present={}us; render p95: convert={}us scene_key={}us rects={}us assets={}us batch_key={}us upload={}us draw={}us metrics={}us text={}us",
                check.detail,
                event_dispatch.p95_us,
                event_dispatch.max_us,
                executor.p95_us,
                executor.max_us,
                runtime_document.p95_us,
                runtime_document.max_us,
                document_update.p95_us,
                document_update.max_us,
                frame.p95_us,
                frame.max_us,
                acquire.p95_us,
                render.p95_us,
                submit.p95_us,
                present.p95_us,
                scene_convert,
                scene_key,
                rect_vertices,
                asset_prepare,
                quad_batch_key,
                quad_upload,
                draw_pass,
                retained_metrics,
                text_render,
            ));
        }
    }
}

#[cfg(target_os = "linux")]
fn add_switch_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[u64],
    warmup: usize,
    minimum: usize,
    p95_limit: u64,
    max_limit: u64,
) {
    let values = samples.iter().skip(warmup).copied().collect::<Vec<_>>();
    add_summary_check(
        checks,
        id,
        &values,
        minimum,
        Some(p95_limit),
        None,
        max_limit,
    );
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn add_switch_final_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[SwitchFinalSample],
    events: &[ObserverEvent],
    warmup: usize,
    minimum: usize,
    p95_limit: u64,
    max_limit: u64,
) {
    let values = samples
        .iter()
        .skip(warmup)
        .map(|sample| sample.elapsed_us)
        .collect::<Vec<_>>();
    add_summary_check(
        checks,
        id,
        &values,
        minimum,
        Some(p95_limit),
        None,
        max_limit,
    );
    if values.len() < minimum {
        return;
    }
    let Some((ordinal, worst)) = samples
        .iter()
        .enumerate()
        .skip(warmup)
        .max_by_key(|(_, sample)| sample.elapsed_us)
    else {
        return;
    };
    let frame = events.iter().find_map(|event| match event {
        ObserverEvent::FramePresented(frame) if frame.key == worst.key => Some(frame),
        _ => None,
    });
    if let Some(check) = checks.last_mut() {
        let frame_detail = frame.map_or_else(
            || "frame unavailable".to_owned(),
            |frame| {
                format!(
                    "frame={}us render={}us submit={}us present={}us",
                    frame.frame_us, frame.render_us, frame.submit_us, frame.present_us
                )
            },
        );
        check.detail = bounded_detail(format!(
            "{}; worst ordinal={} revision={} total={}us compile={}us post_compile={}us {frame_detail}",
            check.detail,
            ordinal + 1,
            worst.revision,
            worst.elapsed_us,
            worst.compile_us,
            worst.post_compile_us,
        ));
    }
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn add_summary_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    values: &[u64],
    minimum: usize,
    p95_limit: Option<u64>,
    p99_limit: Option<u64>,
    max_limit: u64,
) {
    if values.len() < minimum {
        checks.push(Check::fail(
            id,
            format!(
                "{} samples after warmup; minimum is {minimum}",
                values.len()
            ),
        ));
        return;
    }
    let summary = TimingSummary::from_values(values, p95_limit.unwrap_or(max_limit));
    let pass = p95_limit.is_none_or(|limit| summary.p95_us <= limit)
        && p99_limit.is_none_or(|limit| summary.p99_us <= limit)
        && summary.max_us <= max_limit;
    checks.push(check_result(
        id,
        pass,
        format!(
            "{} samples: p95={}us p99={}us max={}us",
            summary.sample_count, summary.p95_us, summary.p99_us, summary.max_us
        ),
        format!(
            "{} samples exceed budget: p95={}us p99={}us max={}us",
            summary.sample_count, summary.p95_us, summary.p99_us, summary.max_us
        ),
    ));
}

#[cfg(target_os = "linux")]
struct ObserverServer {
    socket_path: PathBuf,
    receiver: mpsc::Receiver<Result<ObserverEvent, String>>,
    closing: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl ObserverServer {
    fn bind(path: &Path) -> Result<Self, String> {
        let _ = fs::remove_file(path);
        let listener = UnixListener::bind(path)
            .map_err(|error| format!("bind observer {}: {error}", path.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::sync_channel(OBSERVER_QUEUE_DEPTH);
        let closing = Arc::new(AtomicBool::new(false));
        let accept_closing = Arc::clone(&closing);
        let acceptor = thread::Builder::new()
            .name("boon-verifier-observer-server".to_owned())
            .spawn(move || {
                while !accept_closing.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let sender = sender.clone();
                            let _ = thread::Builder::new()
                                .name("boon-verifier-observer-reader".to_owned())
                                .spawn(move || observer_reader(stream, sender));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => {
                            let _ = sender.send(Err(format!("observer accept failed: {error}")));
                            return;
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            socket_path: path.to_owned(),
            receiver,
            closing,
            acceptor: Some(acceptor),
        })
    }

    fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Result<ObserverEvent, String>, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

#[cfg(target_os = "linux")]
impl Drop for ObserverServer {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Relaxed);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[cfg(target_os = "linux")]
fn observer_reader(
    mut stream: std::os::unix::net::UnixStream,
    sender: mpsc::SyncSender<Result<ObserverEvent, String>>,
) {
    loop {
        match read_event(&mut stream) {
            Ok(Some(event)) => {
                if sender.send(Ok(event)).is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Err(error) => {
                let _ = sender.send(Err(error.to_string()));
                return;
            }
        }
    }
}

#[cfg(target_os = "linux")]
struct ScratchDir {
    path: PathBuf,
}

#[cfg(target_os = "linux")]
impl ScratchDir {
    fn create(run_id: &str, gate: &str) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;

        let parent = std::env::temp_dir().join("boon-native-v2");
        fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stem = safe_component(&format!("{run_id}-{gate}-{}-{nonce}", std::process::id()));
        for suffix in 0..16_u8 {
            let path = parent.join(format!("{stem}-{suffix}"));
            match fs::create_dir(&path) {
                Ok(()) => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                        .map_err(|error| error.to_string())?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("create {}: {error}", path.display())),
            }
        }
        Err("cannot allocate a unique native verifier scratch directory".to_owned())
    }
}

#[cfg(target_os = "linux")]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        if std::env::var_os("BOON_VERIFY_KEEP_SCRATCH").is_none() {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            eprintln!("kept verifier scratch at {}", self.path.display());
        }
    }
}

#[cfg(target_os = "linux")]
struct NativeSession {
    desktop_pid: u32,
    launch_id: String,
    workspace_name: String,
    isolated_seat_name: String,
    session_id: String,
    phase: NativeSessionPhase,
    observed_roles: Vec<u32>,
    input: Option<NativeInput>,
    workspace: Option<WorkspaceGuard>,
    pointer_space: Option<(i32, i32)>,
    input_authorization: Option<IsolationStatus>,
    input_started_after_authorization: bool,
    closed: bool,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeSessionPhase {
    Primary,
    Restart,
}

#[cfg(target_os = "linux")]
impl NativeSession {
    fn start(
        workspace: &Path,
        runtime_dir: &Path,
        executable: &Path,
        example: &str,
        observer_socket: &Path,
        artifact_dir: &Path,
        state_root: Option<&Path>,
        profile: &VerifierProfile,
        phase: NativeSessionPhase,
    ) -> Result<Self, String> {
        let ipc = runtime_dir.join("desktop.sock");
        let launch_log = runtime_dir.join("desktop-launch.log");
        let role_log = runtime_dir.join("native-roles.log");
        let workspace_name = format!(
            "{NATIVE_WORKSPACE}-verify-{}-{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let session_id = format!(
            "{workspace_name}:{}",
            match phase {
                NativeSessionPhase::Primary => "primary",
                NativeSessionPhase::Restart => "restart",
            }
        );
        let mut environment = vec![
            (
                OBSERVER_SOCKET_ENV,
                observer_socket.to_string_lossy().into_owned(),
            ),
            (PROOF_MODE_ENV, "readback".to_owned()),
            (
                PROOF_ARTIFACT_DIR_ENV,
                artifact_dir.to_string_lossy().into_owned(),
            ),
            (PROOF_SAMPLE_ORDINAL_ENV, "128".to_owned()),
            (PRODUCT_PROOF_AFTER_TEST_ENV, "1".to_owned()),
            (NATIVE_SESSION_ID_ENV, session_id.clone()),
            (crate::protocol::VERIFY_BOUNDED_WINDOWS_ENV, "1".to_owned()),
            (
                "BOON_NATIVE_ROLE_LOG",
                role_log.to_string_lossy().into_owned(),
            ),
        ];
        if let Some(state_root) = state_root {
            environment.push((
                "BOON_PLAYGROUND_STATE_ROOT",
                state_root.to_string_lossy().into_owned(),
            ));
            environment.push((STATE_MOUNT_EVIDENCE_ENV, "1".to_owned()));
        }
        match phase {
            NativeSessionPhase::Primary => {
                let checkpoint_steps = profile.scenario_checkpoint_steps();
                if !checkpoint_steps.is_empty() {
                    environment.push((STATE_EVIDENCE_STEPS_ENV, checkpoint_steps.join(",")));
                }
                if profile.requires_persistence_exercise() {
                    environment.push((PERSISTENCE_EVIDENCE_ENV, "1".to_owned()));
                }
                if profile.requires_migration_exercise() && !profile.restart_required {
                    environment.push((MIGRATION_EVIDENCE_ENV, "1".to_owned()));
                }
                if profile.required_checkpoints.iter().any(|checkpoint| {
                    matches!(
                        checkpoint.evidence,
                        VerifierCheckpointRequirementKind::StaleCompileRejection
                    )
                }) {
                    environment.push((STALE_PROGRAM_EVIDENCE_ENV, "1".to_owned()));
                }
                if let Some(width) = profile.required_checkpoints.iter().find_map(|checkpoint| {
                    match checkpoint.evidence {
                        VerifierCheckpointRequirementKind::ResponsiveLayout {
                            logical_width,
                            ..
                        } => Some(logical_width),
                        _ => None,
                    }
                }) {
                    environment.push((RESPONSIVE_EVIDENCE_WIDTH_ENV, width.to_string()));
                }
                if !profile.required_budget_metrics.is_empty() {
                    environment.push((PROFILE_BENCHMARK_ENV, "120".to_owned()));
                    environment.push((
                        PROFILE_BENCHMARK_STEPS_ENV,
                        profile.profile_benchmark_steps.join(","),
                    ));
                }
                if !profile.required_native_workflow_steps.is_empty() {
                    environment.push((
                        NATIVE_WORKFLOW_STEPS_ENV,
                        profile.required_native_workflow_steps.join(","),
                    ));
                    environment.push((
                        NATIVE_WORKFLOW_PROOF_STEPS_ENV,
                        profile
                            .required_native_workflow_proof_steps
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(","),
                    ));
                }
                if profile.scroll_samples > 0 {
                    environment.push((SCROLL_PROOF_ORDINAL_ENV, "21".to_owned()));
                }
            }
            NativeSessionPhase::Restart if profile.requires_migration_exercise() => {
                environment.push((MIGRATION_EVIDENCE_ENV, "1".to_owned()));
            }
            NativeSessionPhase::Restart => {}
        }
        let mut launcher = Command::new("cosmic-background-launch");
        launcher
            .current_dir(workspace)
            .arg("--workspace")
            .arg(&workspace_name)
            .args(["--frame-pacing", "demand", "--isolated-input", "--", "env"]);
        for (name, value) in environment {
            launcher.arg(format!("{name}={value}"));
        }
        launcher
            .arg(executable)
            .args(["--role", "desktop", "--example", example, "--ipc-path"])
            .arg(ipc);
        let result = run_logged(&mut launcher, &launch_log, Duration::from_secs(10))
            .map_err(|error| format!("launch isolated COSMIC windows: {error}"))?;
        if !result.success() {
            return Err(process_failure(
                "cosmic-background-launch",
                &result,
                &launch_log,
            ));
        }
        let mut launch_fields = result.output.split_whitespace();
        let desktop_pid = launch_fields
            .next()
            .ok_or("cosmic-background-launch omitted the desktop PID")?
            .parse::<u32>()
            .map_err(|error| {
                format!("invalid desktop PID from cosmic-background-launch: {error}")
            })?;
        let launch_id = launch_fields
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("cosmic-background-launch omitted the launch ID")?
            .to_owned();
        let isolated_seat_name = launch_fields
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("cosmic-background-launch omitted the isolated seat name")?
            .to_owned();
        if launch_fields.next().is_some() {
            terminate_process(desktop_pid, "TERM");
            let _ = release_background_launch(&launch_id);
            return Err("cosmic-background-launch returned unexpected fields".to_owned());
        }
        let input = match NativeInput::start(executable, &isolated_seat_name) {
            Ok(input) => input,
            Err(error) => {
                terminate_process(desktop_pid, "TERM");
                let _ = release_background_launch(&launch_id);
                return Err(error);
            }
        };
        if let Err(error) = wait_for_isolated_input(&launch_id, &isolated_seat_name) {
            let mut input = input;
            let _ = input.shutdown();
            terminate_process(desktop_pid, "TERM");
            let _ = release_background_launch(&launch_id);
            return Err(error);
        }
        Ok(Self {
            desktop_pid,
            launch_id,
            workspace_name,
            isolated_seat_name,
            session_id,
            phase,
            observed_roles: Vec::new(),
            input: Some(input),
            workspace: None,
            pointer_space: None,
            input_authorization: None,
            input_started_after_authorization: false,
            closed: false,
        })
    }

    fn desktop_id(&self) -> u32 {
        self.desktop_pid
    }

    fn launch_isolation_evidence(&self) -> Result<LaunchIsolationEvidence, String> {
        let status = self
            .input_authorization
            .as_ref()
            .ok_or("launch isolation was not authorized")?;
        status.require_safe(&self.isolated_seat_name)?;
        status.require_layout(self.observed_roles.len())?;
        let input_owned = self.input.is_some() && status.device_count == 2;
        Ok(LaunchIsolationEvidence {
            phase: match self.phase {
                NativeSessionPhase::Primary => "primary",
                NativeSessionPhase::Restart => "restart",
            },
            session_id: self.session_id.clone(),
            seat_name: status.seat_name.clone(),
            pointer_device_owned: input_owned,
            keyboard_device_owned: input_owned,
            owned_device_count: status.device_count,
            workspace_inactive: !status.workspace_active,
            mapped_surface_count: status.mapped_surface_count,
            tiling_enabled: status.tiling_enabled,
            tiled_window_count: status.tiled_window_count,
            floating_window_count: status.floating_window_count,
            maximized_window_count: status.maximized_window_count,
            ownership_and_layout_preceded_input: self.input_started_after_authorization,
        })
    }

    fn prepare_background_workspace(&mut self, executable: &Path) -> Result<(), String> {
        let isolation = self.reconcile_background_layout()?;
        let workspace = WorkspaceGuard::start(executable, &self.workspace_name)?;
        let (width, height) = workspace.output_size();
        let input = self
            .input
            .as_mut()
            .ok_or("kernel virtual input process is unavailable")?;
        input.set_pointer_space(width, height)?;
        self.input_authorization = Some(isolation);
        input.prepare_pointer()?;
        self.input_started_after_authorization = true;
        self.pointer_space = Some((width, height));
        self.workspace = Some(workspace);
        Ok(())
    }

    fn reconcile_background_layout(&self) -> Result<IsolationStatus, String> {
        let output = Command::new("cosmic-background-launch")
            .args(["--reconcile", &self.launch_id])
            .output()
            .map_err(|error| format!("reconcile COSMIC background launch: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "COSMIC background launch reconciliation failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let reconciled = String::from_utf8(output.stdout)
            .map_err(|error| format!("invalid COSMIC reconciliation output: {error}"))?
            .trim()
            .parse::<usize>()
            .map_err(|error| format!("invalid COSMIC reconciliation count: {error}"))?;
        if reconciled < self.observed_roles.len() {
            return Err(format!(
                "COSMIC reconciled only {reconciled} of {} native role windows",
                self.observed_roles.len()
            ));
        }
        let isolation = query_isolation_status(&self.launch_id)?;
        isolation.require_safe(&self.isolated_seat_name)?;
        isolation.require_layout(self.observed_roles.len())?;
        Ok(isolation)
    }

    fn wait_for_roles(&mut self, timeout: Duration) -> Result<RolePids, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if !process_exists(self.desktop_id()) {
                return Err("desktop exited before preview and dev connected".to_owned());
            }
            let descendants = process_descendants(self.desktop_id());
            let preview = descendants
                .iter()
                .copied()
                .find(|pid| process_role(*pid).as_deref() == Some("preview"));
            let dev = descendants
                .iter()
                .copied()
                .find(|pid| process_role(*pid).as_deref() == Some("dev"));
            if let (Some(preview), Some(dev)) = (preview, dev) {
                self.observed_roles = vec![preview, dev];
                return Ok(RolePids { preview, dev });
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "did not observe distinct preview/dev children within {}ms; descendants={descendants:?}",
                    timeout.as_millis()
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn run_driver(&mut self, arguments: &[&str]) -> Result<DriverAck, String> {
        let authorization = self
            .input_authorization
            .as_ref()
            .ok_or("native input is forbidden before launch isolation and layout authorization")?;
        authorization.require_safe(&self.isolated_seat_name)?;
        authorization.require_layout(self.observed_roles.len())?;
        if !self.input_started_after_authorization {
            return Err("native input authorization was not established before input".to_owned());
        }
        let input = self
            .input
            .as_mut()
            .ok_or("kernel virtual input process is unavailable")?;
        match arguments {
            ["move", x, y] => {
                let point = (
                    x.parse::<i32>().map_err(|error| error.to_string())?,
                    y.parse::<i32>().map_err(|error| error.to_string())?,
                );
                let actual = input.move_pointer(point)?;
                Ok(DriverAck {
                    output: format!("x={} y={}", actual.0, actual.1),
                })
            }
            ["click", button] => {
                input.click(pointer_button_code(button)?)?;
                Ok(DriverAck::default())
            }
            ["button", state, button] => {
                input.button(pointer_button_code(button)?, *state == "down")?;
                Ok(DriverAck::default())
            }
            ["button", state] => {
                input.button(0x110, *state == "down")?;
                Ok(DriverAck::default())
            }
            ["axis", axis, amount] => {
                input.wheel(
                    *axis == "horizontal",
                    amount.parse().map_err(|error| {
                        format!("invalid virtual wheel amount `{amount}`: {error}")
                    })?,
                )?;
                Ok(DriverAck::default())
            }
            ["chord", modifier, key] => {
                input.chord(&[key_code(modifier)?], key_code(key)?)?;
                Ok(DriverAck::default())
            }
            ["key", state, key] => {
                input.key(key_code(key)?, *state == "down")?;
                Ok(DriverAck::default())
            }
            ["text", text] => {
                input.ascii_text_batch(text)?;
                Ok(DriverAck::default())
            }
            _ => Err(format!(
                "unsupported kernel virtual input command: {}",
                arguments.join(" ")
            )),
        }
    }

    fn move_pointer(&mut self, point: (i32, i32)) -> Result<(i32, i32), String> {
        let x = point.0.to_string();
        let y = point.1.to_string();
        let result = self.run_driver(&["move", &x, &y])?;
        let coordinate = |prefix: &str| -> Result<i32, String> {
            let value = result
                .output
                .split_whitespace()
                .find_map(|part| part.strip_prefix(prefix))
                .ok_or_else(|| format!("driver move acknowledgement omitted {prefix}"))?
                .parse::<f64>()
                .map_err(|error| format!("invalid driver move {prefix} coordinate: {error}"))?;
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err(format!("driver move {prefix} coordinate is out of range"));
            }
            Ok(value.round() as i32)
        };
        Ok((coordinate("x=")?, coordinate("y=")?))
    }

    fn pointer_space(&self) -> Result<(i32, i32), String> {
        self.pointer_space.ok_or(
            "native pointer space is unavailable before isolated layout preparation".to_owned(),
        )
    }

    fn shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut errors = Vec::new();
        if let Some(mut input) = self.input.take()
            && let Err(error) = input.shutdown()
        {
            errors.push(error);
        }
        if let Some(mut workspace) = self.workspace.take()
            && let Err(error) = workspace.shutdown()
        {
            errors.push(error);
        }
        let desktop_id = self.desktop_id();
        let mut pids = process_descendants(desktop_id);
        pids.extend(self.observed_roles.iter().copied());
        pids.push(desktop_id);
        pids.sort_unstable();
        pids.dedup();
        for pid in pids.iter().rev().copied().filter(|pid| *pid != 0) {
            terminate_process(pid, "TERM");
        }
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        while pids.iter().copied().any(process_exists) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        for pid in pids
            .iter()
            .rev()
            .copied()
            .filter(|pid| process_exists(*pid))
        {
            terminate_process(pid, "KILL");
        }
        if let Err(error) = release_background_launch(&self.launch_id) {
            errors.push(error);
        }
        self.desktop_pid = 0;
        let leaked = pids
            .into_iter()
            .filter(|pid| *pid != 0 && process_exists(*pid))
            .collect::<Vec<_>>();
        if !leaked.is_empty() {
            errors.push(format!(
                "native verifier process cleanup left live PIDs {leaked:?}"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct IsolationStatus {
    seat_name: String,
    device_count: usize,
    workspace_active: bool,
    mapped_surface_count: usize,
    tiling_enabled: bool,
    floating_window_count: usize,
    tiled_window_count: usize,
    maximized_window_count: usize,
}

#[cfg(target_os = "linux")]
impl IsolationStatus {
    fn require_safe(&self, expected_seat: &str) -> Result<(), String> {
        if self.seat_name != expected_seat {
            return Err(format!(
                "isolated input status named seat `{}`, expected `{expected_seat}`",
                self.seat_name
            ));
        }
        if self.device_count != 2 {
            return Err(format!(
                "isolated seat `{expected_seat}` owns {} devices, expected pointer and keyboard",
                self.device_count
            ));
        }
        if self.workspace_active {
            return Err(format!(
                "isolated seat `{expected_seat}` targets the active workspace; refusing input"
            ));
        }
        Ok(())
    }

    fn require_layout(&self, expected_windows: usize) -> Result<(), String> {
        if !self.tiling_enabled
            || self.floating_window_count != 0
            || self.tiled_window_count != expected_windows
            || self.maximized_window_count != 0
            || self.mapped_surface_count != expected_windows
        {
            return Err(format!(
                "isolated workspace layout is not independently tiled: mapped={}, tiled={}, \
                 floating={}, maximized={}, tiling_enabled={}, expected_windows={expected_windows}",
                self.mapped_surface_count,
                self.tiled_window_count,
                self.floating_window_count,
                self.maximized_window_count,
                self.tiling_enabled,
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn wait_for_isolated_input(
    launch_id: &str,
    expected_seat: &str,
) -> Result<IsolationStatus, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let error = match query_isolation_status(launch_id) {
            Ok(status) => match status.require_safe(expected_seat) {
                Ok(()) => return Ok(status),
                Err(error) => error,
            },
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            return Err(error);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
fn query_isolation_status(launch_id: &str) -> Result<IsolationStatus, String> {
    let output = Command::new("cosmic-background-launch")
        .args(["--isolation-status", launch_id])
        .output()
        .map_err(|error| format!("query COSMIC input isolation: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "COSMIC input isolation query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|error| format!("invalid COSMIC isolation status: {error}"))?;
    let value = |name: &str| {
        output
            .split_whitespace()
            .find_map(|field| field.strip_prefix(name))
            .ok_or_else(|| format!("COSMIC isolation status omitted {name}"))
    };
    Ok(IsolationStatus {
        seat_name: value("seat=")?.to_owned(),
        device_count: value("devices=")?
            .parse()
            .map_err(|error| format!("invalid isolated device count: {error}"))?,
        workspace_active: value("workspace_active=")?
            .parse()
            .map_err(|error| format!("invalid isolated workspace state: {error}"))?,
        mapped_surface_count: value("mapped_surfaces=")?
            .parse()
            .map_err(|error| format!("invalid isolated mapped-surface count: {error}"))?,
        tiling_enabled: value("tiling_enabled=")?
            .parse()
            .map_err(|error| format!("invalid isolated tiling state: {error}"))?,
        floating_window_count: value("floating_windows=")?
            .parse()
            .map_err(|error| format!("invalid isolated floating-window count: {error}"))?,
        tiled_window_count: value("tiled_windows=")?
            .parse()
            .map_err(|error| format!("invalid isolated tiled-window count: {error}"))?,
        maximized_window_count: value("maximized_windows=")?
            .parse()
            .map_err(|error| format!("invalid isolated maximized-window count: {error}"))?,
    })
}

#[cfg(target_os = "linux")]
fn release_background_launch(launch_id: &str) -> Result<(), String> {
    let output = Command::new("cosmic-background-launch")
        .args(["--release", launch_id])
        .output()
        .map_err(|error| format!("release COSMIC background launch: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "release COSMIC background launch failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct DriverAck {
    output: String,
}

#[cfg(target_os = "linux")]
fn pointer_button_code(name: &str) -> Result<u16, String> {
    match name {
        "left" => Ok(0x110),
        "right" => Ok(0x111),
        "middle" => Ok(0x112),
        value => value
            .parse()
            .map_err(|error| format!("invalid pointer button `{value}`: {error}")),
    }
}

#[cfg(target_os = "linux")]
fn key_code(name: &str) -> Result<u16, String> {
    match name {
        "ctrl" => Ok(29),
        "a" => Ok(30),
        "tab" => Ok(15),
        "enter" => Ok(28),
        "escape" => Ok(1),
        "left" => Ok(105),
        "right" => Ok(106),
        "i" => Ok(23),
        "u" => Ok(22),
        "y" => Ok(21),
        value => value
            .parse()
            .map_err(|error| format!("invalid keyboard key `{value}`: {error}")),
    }
}

#[cfg(target_os = "linux")]
impl Drop for NativeSession {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct RolePids {
    preview: u32,
    dev: u32,
}

#[cfg(target_os = "linux")]
struct LoggedProcess {
    status: ExitStatus,
    timed_out: bool,
    output: String,
}

#[cfg(target_os = "linux")]
impl LoggedProcess {
    fn success(&self) -> bool {
        self.status.success() && !self.timed_out
    }
}

#[cfg(target_os = "linux")]
fn cleanup_check(result: Result<(), String>) -> Check {
    match result {
        Ok(()) => Check::pass(
            "native-os-input-cleanup",
            "isolated virtual devices and desktop/preview/dev process tree stopped without leaks or workspace activation",
        ),
        Err(error) => Check::fail("native-os-input-cleanup", error),
    }
}

#[cfg(target_os = "linux")]
fn run_logged(
    command: &mut Command,
    log_path: &Path,
    timeout: Duration,
) -> std::io::Result<LoggedProcess> {
    let log = File::create(log_path)?;
    command
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    let started = Instant::now();
    let mut child = command.spawn()?;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ok(LoggedProcess {
        status,
        timed_out,
        output: fs::read_to_string(log_path).unwrap_or_default(),
    })
}

#[cfg(target_os = "linux")]
fn process_failure(label: &str, process: &LoggedProcess, log: &Path) -> String {
    bounded_detail(format!(
        "{label} failed{} with {}; {}",
        if process.timed_out {
            " after timeout"
        } else {
            ""
        },
        process.status,
        tail(log, 2_000)
    ))
}

#[cfg(target_os = "linux")]
fn process_descendants(root: u32) -> Vec<u32> {
    if root == 0 {
        return Vec::new();
    }
    let mut found = BTreeSet::new();
    let mut pending = VecDeque::from([root]);
    while let Some(parent) = pending.pop_front() {
        let children_path = format!("/proc/{parent}/task/{parent}/children");
        let Ok(children) = fs::read_to_string(children_path) else {
            continue;
        };
        for child in children
            .split_whitespace()
            .filter_map(|value| value.parse::<u32>().ok())
        {
            if found.insert(child) {
                pending.push_back(child);
            }
        }
    }
    found.into_iter().collect()
}

#[cfg(target_os = "linux")]
fn process_role(pid: u32) -> Option<String> {
    let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let arguments = bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .collect::<Vec<_>>();
    arguments
        .windows(2)
        .find(|pair| pair[0] == "--role")
        .map(|pair| pair[1].clone())
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    pid != 0 && Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(target_os = "linux")]
fn terminate_process(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(target_os = "linux")]
fn tail(path: &Path, maximum: usize) -> String {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return format!("cannot read {}: {error}", path.display()),
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.read_to_end(&mut bytes) {
        return format!("cannot read {}: {error}", path.display());
    }
    let start = bytes.len().saturating_sub(maximum);
    String::from_utf8_lossy(&bytes[start..]).trim().to_owned()
}

fn write_envelope(path: &Path, envelope: &ProducerEnvelope) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(envelope).map_err(|error| error.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err(format!(
            "producer evidence is unbounded at {} bytes",
            bytes.len()
        ));
    }
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn required<'a>(args: &'a [String], flag: &str) -> Result<&'a str, String> {
    optional(args, flag).ok_or_else(|| format!("{flag} requires a value"))
}

fn optional<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn repeated<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}

fn parse_usize(args: &[String], flag: &str, default: usize) -> Result<usize, String> {
    optional(args, flag).map_or(Ok(default), |value| {
        value
            .parse::<usize>()
            .map_err(|error| format!("invalid {flag} value `{value}`: {error}"))
    })
}

fn parse_bool(args: &[String], flag: &str, default: bool) -> Result<bool, String> {
    optional(args, flag).map_or(Ok(default), |value| match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid {flag} boolean `{value}`")),
    })
}

fn parse_csv(args: &[String], flag: &str) -> Result<Vec<String>, String> {
    let Some(value) = optional(args, flag) else {
        return Ok(Vec::new());
    };
    let values = value
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() || values.iter().any(String::is_empty) {
        return Err(format!("{flag} requires non-empty comma-separated values"));
    }
    let mut unique = BTreeSet::new();
    if values.iter().any(|value| !unique.insert(value.as_str())) {
        return Err(format!("{flag} contains duplicate values"));
    }
    Ok(values)
}

fn parse_async_lanes(args: &[String]) -> Result<Vec<AsyncLaneKind>, String> {
    parse_csv(args, "--required-async-lanes")?
        .into_iter()
        .map(|lane| match lane.as_str() {
            "child-program-compile" => Ok(AsyncLaneKind::ChildProgramCompile),
            "persistence-turn" => Ok(AsyncLaneKind::PersistenceTurn),
            "program-artifact-store" => Ok(AsyncLaneKind::ProgramArtifactStore),
            "program-artifact-load" => Ok(AsyncLaneKind::ProgramArtifactLoad),
            "proof-readback" => Ok(AsyncLaneKind::ProofReadback),
            _ => Err(format!("unsupported async lane `{lane}`")),
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
fn safe_component(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(48)
        .collect::<String>();
    if value.is_empty() {
        "native-v2".to_owned()
    } else {
        value
    }
}

fn bounded_detail(value: impl Into<String>) -> String {
    let value = value.into();
    if value.len() <= MAX_DETAIL_BYTES {
        return value;
    }
    let mut end = MAX_DETAIL_BYTES.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn check_result(
    id: &'static str,
    pass: bool,
    pass_detail: impl Into<String>,
    fail_detail: impl Into<String>,
) -> Check {
    if pass {
        Check::pass(id, pass_detail)
    } else {
        Check::fail(id, fail_detail)
    }
}

#[derive(Serialize)]
struct ProducerEnvelope {
    format: u16,
    protocol: &'static str,
    gate: String,
    run_id: String,
    source_digest: String,
    evidence: GateEvidence,
}

#[derive(Serialize)]
struct GateEvidence {
    checks: Vec<Check>,
    producer: Option<()>,
    profile: Option<VerificationProfileEvidence>,
    native: Option<NativeEvidence>,
    product_ux_timings: Vec<ProductTimingEvidence>,
    async_proof_timing: Option<AsyncProofTimingEvidence>,
    async_lanes: Vec<AsyncLaneEvidence>,
    artifacts: Vec<ArtifactMetadata>,
}

impl GateEvidence {
    #[cfg(not(target_os = "linux"))]
    fn failed(profile: &VerifierProfile, check: Check) -> Self {
        Self {
            checks: vec![check],
            producer: None,
            profile: Some(profile_evidence(profile, None)),
            native: None,
            product_ux_timings: Vec::new(),
            async_proof_timing: None,
            async_lanes: Vec::new(),
            artifacts: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct VerificationProfileEvidence {
    profile_id: String,
    profile_digest: String,
    scenario: Option<ScenarioProof>,
    budget: Option<BudgetProof>,
    state_root: Option<StateRootProof>,
    native_workflow: Option<NativeWorkflowProof>,
    checkpoints: Vec<StateCheckpointProof>,
}

#[derive(Serialize)]
struct NativeWorkflowProof {
    input_delivery: &'static str,
    scenario_boundary: &'static str,
    test_request_id: u64,
    initial_state_digest: String,
    final_state_digest: String,
    ready_frame: ReportFrameEvidenceKey,
    final_frame: ReportFrameEvidenceKey,
    steps: Vec<NativeWorkflowStepProof>,
}

#[derive(Serialize)]
struct NativeWorkflowStepProof {
    request_id: u64,
    ordinal: u32,
    scenario_step: String,
    source_path: String,
    action_kind: String,
    action_digest: String,
    input_first_sequence: u64,
    input_last_sequence: u64,
    input_event_count: u32,
    input_event_digest: String,
    assertion_count: u32,
    source_revision: u64,
    runtime_sequence: u64,
    durable_epoch: u64,
    durable_turn_sequence: u64,
    durable_acked: bool,
    before_state_digest: String,
    state_digest: String,
    frame: ReportFrameEvidenceKey,
}

#[derive(Serialize)]
struct ScenarioProof {
    path: String,
    sha256: String,
    boundary: &'static str,
    request_id: Option<u64>,
    declared_steps: u32,
    executable_steps: u32,
    completed_steps: u32,
    passed: bool,
    semantic_assertions_proven: bool,
}

#[derive(Serialize)]
struct BudgetProof {
    path: String,
    sha256: String,
    observations: Vec<BudgetObservation>,
}

#[derive(Serialize)]
struct BudgetObservation {
    metric: String,
    unit: &'static str,
    comparison: &'static str,
    observed: u64,
    limit: u64,
}

#[derive(Serialize)]
struct StateRootProof {
    root: String,
    policy: String,
    clean_at_start: bool,
    durable_file_count: u32,
    restart_count: u32,
    restored_after_restart: bool,
}

#[derive(Serialize)]
struct StateCheckpointProof {
    id: String,
    source_revision: u64,
    runtime_sequence: u64,
    durable_epoch: u64,
    durable_turn_sequence: u64,
    state_digest: String,
    frame: ReportFrameEvidenceKey,
    #[serde(flatten)]
    evidence: StateCheckpointEvidence,
}

#[derive(Serialize)]
#[serde(tag = "boundary", rename_all = "kebab-case")]
enum StateCheckpointEvidence {
    ScenarioSemanticFrame {
        scenario_step: String,
        assertion_count: u32,
    },
    RestartRestore {
        baseline_checkpoint: String,
        before_restart_digest: String,
        baseline_durable_epoch: u64,
        baseline_durable_turn_sequence: u64,
        baseline_frame: ReportFrameEvidenceKey,
        process_replaced: bool,
        session_replaced: bool,
        first_observable_frame: bool,
        startup_restored: bool,
    },
    ResponsiveLayout {
        baseline_checkpoint: String,
        logical_width: u32,
        logical_height: u32,
        action_count: u32,
        action_digest: String,
    },
    StaleCompileRejection {
        session: String,
        stale_revision: u64,
        latest_revision: u64,
    },
    PersistenceOperation {
        operation: VerifierPersistenceOperation,
        before_state_digest: String,
    },
    NativeWorkflowFrame {
        scenario_step: String,
        action_kind: String,
        request_id: u64,
        action_digest: String,
        input_first_sequence: u64,
        input_last_sequence: u64,
        input_event_count: u32,
        input_event_digest: String,
        durable_acked: bool,
        assertion_count: u32,
    },
}

#[derive(Serialize)]
struct NativeEvidence {
    adapter_name: String,
    adapter_backend: String,
    adapter_device_type: String,
    software_adapter: bool,
    present_mode: String,
    surface_format: String,
    window_backend: String,
    preview_pid: u32,
    dev_pid: u32,
    input_delivery: &'static str,
    scenario_boundary: &'static str,
    capture_method: String,
    private_runtime_dispatch_used: bool,
    launch_isolation: Vec<LaunchIsolationEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct LaunchIsolationEvidence {
    phase: &'static str,
    session_id: String,
    seat_name: String,
    pointer_device_owned: bool,
    keyboard_device_owned: bool,
    owned_device_count: usize,
    workspace_inactive: bool,
    mapped_surface_count: usize,
    tiling_enabled: bool,
    tiled_window_count: usize,
    floating_window_count: usize,
    maximized_window_count: usize,
    ownership_and_layout_preceded_input: bool,
}

impl LaunchIsolationEvidence {
    fn is_fail_closed(&self) -> bool {
        !self.session_id.is_empty()
            && self.session_id.len() <= 1_000
            && !self.seat_name.is_empty()
            && self.seat_name.len() <= 1_000
            && self.pointer_device_owned
            && self.keyboard_device_owned
            && self.owned_device_count == 2
            && self.workspace_inactive
            && self.mapped_surface_count == self.tiled_window_count
            && self.tiling_enabled
            && self.tiled_window_count > 0
            && self.floating_window_count == 0
            && self.maximized_window_count == 0
            && self.ownership_and_layout_preceded_input
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReportFrameEvidenceKey {
    surface_id: String,
    process_id: u32,
    session_id: String,
    frame_id: u64,
    input_id: u64,
    content_id: u64,
    layout_id: u64,
    render_id: u64,
    surface_epoch: u64,
    present_id: u64,
    proof_id: u64,
}

#[cfg(target_os = "linux")]
impl From<FrameEvidenceKey> for ReportFrameEvidenceKey {
    fn from(value: FrameEvidenceKey) -> Self {
        Self {
            surface_id: value.surface_id,
            process_id: value.process_id,
            session_id: value.session_id,
            frame_id: value.frame_id,
            input_id: value.input_id,
            content_id: value.content_id,
            layout_id: value.layout_id,
            render_id: value.render_id,
            surface_epoch: value.surface_epoch,
            present_id: value.present_id,
            proof_id: value.proof_id,
        }
    }
}

#[cfg(target_os = "linux")]
impl From<ReportFrameEvidenceKey> for FrameEvidenceKey {
    fn from(value: ReportFrameEvidenceKey) -> Self {
        Self {
            surface_id: value.surface_id,
            process_id: value.process_id,
            session_id: value.session_id,
            frame_id: value.frame_id,
            input_id: value.input_id,
            content_id: value.content_id,
            layout_id: value.layout_id,
            render_id: value.render_id,
            surface_epoch: value.surface_epoch,
            present_id: value.present_id,
            proof_id: value.proof_id,
        }
    }
}

#[derive(Serialize)]
struct ProductTimingEvidence {
    metric: &'static str,
    representative_frame: ReportFrameEvidenceKey,
    representative_sample_ordinal: u32,
    summary: TimingSummary,
}

#[derive(Serialize)]
struct AsyncProofTimingEvidence {
    linked_product_metric: &'static str,
    captured_frame: ReportFrameEvidenceKey,
    completed_after_frame: ReportFrameEvidenceKey,
    proof_lag_frames: u32,
    artifact_id: String,
    snapshot_prepare_us: u64,
    queue_wait_us: u64,
    worker_us: u64,
    apply_us: u64,
    summary: TimingSummary,
}

#[derive(Serialize)]
struct AsyncLaneEvidence {
    lane: &'static str,
    request_id: String,
    revision: u64,
    queue_depth: u32,
    queue_wait_us: u64,
    worker_us: u64,
    apply_us: u64,
    end_to_end_us: u64,
    outcome: &'static str,
    frame: ReportFrameEvidenceKey,
}

#[derive(Serialize)]
struct ArtifactMetadata {
    artifact_id: String,
    kind: &'static str,
    path: String,
    sha256: String,
    byte_len: u64,
    capture_method: String,
    capture_token_digest: String,
    nonblank_samples: u64,
    unique_rgba_values: u64,
    frame: ReportFrameEvidenceKey,
}

#[derive(Clone, Debug, Serialize)]
struct TimingSummary {
    sample_count: u32,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
    outlier_count: u32,
}

impl TimingSummary {
    fn from_values(values: &[u64], outlier_threshold_us: u64) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        Self {
            sample_count: sorted.len().try_into().unwrap_or(u32::MAX),
            p50_us: nearest_rank(&sorted, 50),
            p95_us: nearest_rank(&sorted, 95),
            p99_us: nearest_rank(&sorted, 99),
            max_us: sorted.last().copied().unwrap_or(0),
            outlier_count: sorted
                .iter()
                .filter(|value| **value > outlier_threshold_us)
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
        }
    }
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = percentile.saturating_mul(sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Serialize)]
struct Check {
    id: &'static str,
    outcome: &'static str,
    detail: String,
}

impl Check {
    fn pass(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            outcome: "pass",
            detail: bounded_detail(detail),
        }
    }

    fn fail(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            outcome: "fail",
            detail: bounded_detail(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_arguments_select_generic_harness_behavior() {
        let args = profile_args(&[
            ("--gate", "future-product"),
            ("--profile", "stateful-workflow-v1"),
            ("--profile-digest", &"a".repeat(64)),
            ("--harness", "timed"),
            ("--example", "persons_pro"),
            ("--visible-mode", "hover"),
            ("--visible-samples", "120"),
            ("--alternate-target", "any"),
            ("--selection-samples", "0"),
            ("--scroll-samples", "148"),
            ("--switch-samples", "0"),
        ]);
        let profile = VerifierProfile::parse(&args).unwrap();
        assert_eq!(profile.gate, "future-product");
        assert_eq!(profile.example(), "persons_pro");
        assert_eq!(profile.scroll_samples, 148);
        assert_eq!(profile.selection_samples, 0);
        assert_eq!(profile.visible_mode, VisibleSampleMode::Hover);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_workflow_cursor_keeps_events_buffered_during_prior_proof_waits() {
        let step = |request_id, ordinal, step_id: &str| ObserverEvent::NativeWorkflowStep {
            request_id,
            ordinal,
            step_id: step_id.to_owned(),
            source_path: "assertion-only".to_owned(),
            action_kind: "assertion_only".to_owned(),
            action_digest: "a".repeat(64),
            input_first_sequence: 0,
            input_last_sequence: 0,
            input_event_count: 0,
            input_event_digest: "b".repeat(64),
            assertion_count: 1,
            source_revision: 1,
            runtime_sequence: u64::from(ordinal),
            durable_epoch: u64::from(ordinal),
            durable_turn_sequence: u64::from(ordinal),
            durable_acked: true,
            before_state_digest: "c".repeat(64),
            state_digest: "d".repeat(64),
            key: FrameEvidenceKey {
                surface_id: "preview".to_owned(),
                process_id: 1,
                session_id: "session".to_owned(),
                frame_id: u64::from(ordinal),
                input_id: u64::from(ordinal),
                content_id: u64::from(ordinal),
                layout_id: u64::from(ordinal),
                render_id: u64::from(ordinal),
                surface_epoch: 1,
                present_id: u64::from(ordinal),
                proof_id: u64::from(ordinal),
            },
        };
        let events = vec![step(65, 1, "first"), step(66, 2, "second")];

        let cursor = next_native_workflow_event_cursor(&events, 0, 65, 1, "first").unwrap();
        assert_eq!(cursor, 1);
        assert_eq!(
            next_native_workflow_event_cursor(&events, cursor, 66, 2, "second").unwrap(),
            2
        );
        assert!(next_native_workflow_event_cursor(&events, events.len(), 66, 2, "second").is_err());
    }

    #[test]
    fn flattened_checkpoint_arguments_remain_strict_and_reference_semantic_baselines() {
        let args = profile_args(&[
            ("--gate", "stateful-product"),
            ("--profile", "stateful-workflow-v1"),
            ("--profile-digest", &"c".repeat(64)),
            ("--harness", "timed"),
            ("--example", "counter"),
            ("--visible-mode", "hover"),
            ("--visible-samples", "120"),
            ("--alternate-target", "any"),
            ("--state-root-policy", "launch-scoped-clean"),
            ("--restart-required", "true"),
            (
                "--required-checkpoint",
                r#"{"id":"baseline","kind":"scenario-step","scenario_step":"step-a"}"#,
            ),
            (
                "--required-checkpoint",
                r#"{"id":"restart","kind":"restart-restore","baseline_checkpoint":"baseline"}"#,
            ),
        ]);
        let profile = VerifierProfile::parse(&args).unwrap();
        assert_eq!(profile.required_checkpoints.len(), 2);
        assert!(profile.restart_required);

        let mut invalid = args;
        let restart = invalid
            .iter_mut()
            .find(|value| value.contains("restart-restore"))
            .unwrap();
        *restart = r#"{"id":"restart","kind":"restart-restore","baseline_checkpoint":"missing","extra":true}"#.to_owned();
        assert!(VerifierProfile::parse(&invalid).is_err());
    }

    #[test]
    fn declaration_identity_is_not_reported_as_completed_profile_proof() {
        let args = profile_args(&[
            ("--gate", "persons-pro"),
            ("--profile", "stateful-workflow-v1"),
            ("--profile-digest", &"b".repeat(64)),
            ("--harness", "timed"),
            ("--example", "persons_pro"),
            ("--visible-mode", "hover"),
            ("--visible-samples", "120"),
            ("--alternate-target", "any"),
            ("--selection-samples", "0"),
            ("--scroll-samples", "148"),
            ("--switch-samples", "0"),
            ("--scenario-proof", "examples/persons_pro.scn"),
            ("--require-semantic-scenario", "true"),
            ("--budget-proof", "examples/persons_pro.budget.toml"),
            (
                "--profile-benchmark-steps",
                "valid-edit-preview,corrected-edit-preview",
            ),
            (
                "--required-budget-metrics",
                "keystroke-to-editor-visible-p95",
            ),
            ("--state-root-policy", "launch-scoped-clean"),
            ("--restart-required", "true"),
            ("--required-checkpoints", "fresh-anonymous-workspace"),
        ]);
        let profile = VerifierProfile::parse(&args).unwrap();
        let evidence = profile_evidence(&profile, None);
        let scenario = evidence.scenario.expect("scenario declaration identity");
        assert!(!scenario.passed);
        assert!(!scenario.semantic_assertions_proven);
        assert_eq!(scenario.boundary, "native-test-playback");
        let observations = evidence.budget.unwrap().observations;
        assert!(observations.is_empty());
        assert!(
            !observations
                .iter()
                .any(|value| value.metric == "trusted-parent-rebuilds-per-edit")
        );
        assert!(evidence.state_root.is_none());
        assert!(evidence.checkpoints.is_empty());

        let completion = ScenarioCompletion {
            request_id: 41,
            passed: true,
            semantic_assertions_proven: true,
            completed_steps: 21,
        };
        let scenario = profile_evidence(&profile, Some(completion))
            .scenario
            .expect("completed scenario evidence");
        assert!(scenario.passed);
        assert!(scenario.semantic_assertions_proven);
        assert_eq!(
            scenario.boundary,
            "native-test-playback-and-semantic-assertions"
        );
        assert_eq!(scenario.request_id, Some(41));
    }

    fn profile_args(values: &[(&str, &str)]) -> Vec<String> {
        values
            .iter()
            .flat_map(|(flag, value)| [(*flag).to_owned(), (*value).to_owned()])
            .collect()
    }

    #[test]
    fn details_remain_valid_utf8_and_schema_bounded() {
        let detail = bounded_detail("cells euro".repeat(400));
        assert!(detail.len() <= MAX_DETAIL_BYTES);
        assert!(detail.ends_with("..."));
    }

    #[test]
    fn summaries_retain_outliers_and_use_nearest_rank() {
        let summary = TimingSummary::from_values(&[1, 2, 3, 4, 100], 10);
        assert_eq!(summary.p50_us, 3);
        assert_eq!(summary.p95_us, 100);
        assert_eq!(summary.outlier_count, 1);

        let mut p99_samples = vec![50; 109];
        p99_samples.push(1_500);
        let summary = TimingSummary::from_values(&p99_samples, 1_000);
        assert_eq!(summary.p99_us, 50);
        assert_eq!(summary.max_us, 1_500);
    }

    #[test]
    fn scratch_names_cannot_escape_the_runtime_directory() {
        assert_eq!(safe_component("../../run/id"), "______run_id");
        assert_eq!(safe_component("cells-01"), "cells-01");
    }

    #[test]
    fn window_scan_covers_the_entire_reported_output() {
        let candidates = window_scan_candidates((5_120, 1_440));
        assert!(candidates.iter().any(|(x, _)| *x > 3_500));
        assert!(candidates.iter().any(|(_, y)| *y > 1_000));
        assert!(
            candidates
                .iter()
                .all(|(x, y)| (0..5_120).contains(x) && (0..1_440).contains(y))
        );
    }

    #[test]
    fn retained_target_lookup_uses_the_latest_configured_center() {
        let events = vec![
            ObserverEvent::RoleTarget {
                role: ObserverRole::Dev,
                node: DEV_EDITOR_INPUT_TARGET.to_owned(),
                x: 0.5,
                y: 184.0,
            },
            ObserverEvent::RoleTarget {
                role: ObserverRole::Dev,
                node: DEV_EDITOR_INPUT_TARGET.to_owned(),
                x: 520.0,
                y: 420.0,
            },
        ];
        assert_eq!(
            observed_role_target(&events, ObserverRole::Dev, DEV_EDITOR_INPUT_TARGET),
            Some((520.0, 420.0))
        );
    }

    #[test]
    fn budget_observations_use_the_loaded_typed_contract_and_fail_closed() {
        let source = "[latency_ms]\nkeystroke_to_editor_visible_p95 = 12.5\n";
        let loaded = LoadedBudgetContract {
            declared_path: PathBuf::from("bounded.budget.toml"),
            source: source.to_owned(),
            contract: BudgetContract::parse(source).unwrap(),
        };
        let observation = budget_observation(
            &loaded,
            "keystroke-to-editor-visible-p95",
            BudgetUnit::Microseconds,
            11_000,
        )
        .unwrap();
        assert_eq!(observation.unit, "microseconds");
        assert_eq!(observation.limit, 12_500);
        assert_eq!(
            budget_proof(&loaded, Vec::new()).sha256,
            sha256(source.as_bytes())
        );
        assert!(
            budget_observation(
                &loaded,
                "keystroke-to-editor-visible-p95",
                BudgetUnit::Count,
                1,
            )
            .is_err()
        );
        assert!(budget_observation(&loaded, "missing", BudgetUnit::Count, 1).is_err());
    }

    #[test]
    fn launch_isolation_structured_values_fail_closed() {
        let valid = LaunchIsolationEvidence {
            phase: "primary",
            session_id: "opaque-session".to_owned(),
            seat_name: "opaque-seat".to_owned(),
            pointer_device_owned: true,
            keyboard_device_owned: true,
            owned_device_count: 2,
            workspace_inactive: true,
            mapped_surface_count: 2,
            tiling_enabled: true,
            tiled_window_count: 2,
            floating_window_count: 0,
            maximized_window_count: 0,
            ownership_and_layout_preceded_input: true,
        };
        assert!(valid.is_fail_closed());
        let mut shared_seat = valid.clone();
        shared_seat.workspace_inactive = false;
        assert!(!shared_seat.is_fail_closed());
        let mut missing_keyboard = valid.clone();
        missing_keyboard.keyboard_device_owned = false;
        assert!(!missing_keyboard.is_fail_closed());
        let mut input_before_layout = valid.clone();
        input_before_layout.ownership_and_layout_preceded_input = false;
        assert!(!input_before_layout.is_fail_closed());
        let mut floating = valid;
        floating.floating_window_count = 1;
        assert!(!floating.is_fail_closed());
    }

    #[test]
    fn tiled_divider_drag_is_derived_from_role_rectangles() {
        let preview = RoleRectangle {
            x: 0,
            y: 0,
            width: 900,
            height: 844,
        };
        let dev = RoleRectangle {
            x: 900,
            y: 0,
            width: 900,
            height: 844,
        };
        let (from, to) = divider_drag_points(preview, dev, 390, 844).unwrap();
        assert_eq!(from, (900, 422));
        assert_eq!(to, (390, 422));
        assert!(divider_drag_points(preview, dev, 390, 700).is_err());
    }

    #[test]
    fn report_frame_keys_preserve_surface_process_and_session_identity() {
        let key = FrameEvidenceKey {
            surface_id: "surface-a".to_owned(),
            process_id: 42,
            session_id: "primary".to_owned(),
            frame_id: 1,
            input_id: 2,
            content_id: 3,
            layout_id: 4,
            render_id: 5,
            surface_epoch: 6,
            present_id: 7,
            proof_id: 8,
        };
        let report: ReportFrameEvidenceKey = key.clone().into();
        let restored: FrameEvidenceKey = report.into();
        assert_eq!(restored, key);
        let mut restart = restored;
        restart.session_id = "restart".to_owned();
        assert!(!key.same_producer_surface(&restart));
    }
}
