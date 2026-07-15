use super::*;
use crate::report_v2::{
    AdapterBackend, AdapterDeviceType, AggregateGateResult, AggregateIdentity, AggregateMode,
    AggregateReport, AggregateReportKind, ArtifactKind, ArtifactMetadata, AsyncProofTimingEvidence,
    BoundedId, BoundedString, BudgetComparison, BudgetObservation, BudgetProof, BudgetUnit,
    CaptureMethod, CheckOutcome, CheckpointEvidenceRequirement, ChildValidation, ExpectedIdentity,
    FORMAT_VERSION, FrameEvidenceKey, GateCommand, GateEvidence, GateName, GateRunner,
    HostBoundary, InputDelivery, LaunchIsolationEvidence, LaunchIsolationPhase, ManifestGate,
    ManifestIdentity, MeasurementContract, NativeEvidence, PresentMode, ProducerEvidence,
    ProductTimingEvidence, RelativePath, ReportFileMetadata, ReportStatus, ScenarioBoundary,
    ScenarioProof, Sha256Digest, ShortText, SourceIdentity, StateCheckpointEvidence,
    StateCheckpointProof, StateRootProof, TimingSummary, ToolIdentity, VerificationProfileEvidence,
    WindowBackend, check, detail, gate_report, load_manifest, measurement_contract, protocol_name,
};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn gate(value: &str) -> GateName {
    GateName::new(value).unwrap()
}

fn manifest_gate<'a>(
    manifest: &'a crate::report_v2::HandoffManifest,
    value: &str,
) -> &'a ManifestGate {
    manifest
        .gates
        .iter()
        .find(|entry| entry.gate.slug() == value)
        .unwrap_or_else(|| panic!("manifest gate {value} is missing"))
}

#[test]
fn command_parser_exposes_manifest_gates_and_fixed_tools() {
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    let mut names = vec!["shaders"];
    names.extend(manifest.gates.iter().map(|entry| entry.verifier.as_str()));
    names.push(manifest.aggregate.as_str());
    assert_eq!(names.len(), manifest.gates.len() + 2);
    for name in names {
        assert!(parse_command(&strings(&[name]), &manifest).is_ok());
    }
    assert!(parse_command(&strings(&["help"]), &manifest).is_err());
    assert!(parse_command(&strings(&["verify-native-gpu-all"]), &manifest).is_err());
    assert!(parse_command(&strings(&["verify-report-schema"]), &manifest).is_err());
}

#[test]
fn command_parser_accepts_only_v2_options() {
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    assert_eq!(
        parse_command(&strings(&["shaders", "--check"]), &manifest).unwrap(),
        ParsedCommand::Shaders { check: true }
    );
    assert_eq!(
        parse_command(
            &strings(&[
                "verify-all",
                "--check-existing",
                "--report",
                "target/custom.json",
            ]),
            &manifest
        )
        .unwrap(),
        ParsedCommand::VerifyAll {
            check_existing: true,
            report: Some(PathBuf::from("target/custom.json")),
        }
    );
    assert_eq!(
        parse_command(
            &strings(&["verify-cells", "--report", "target/cells.json"]),
            &manifest,
        )
        .unwrap(),
        ParsedCommand::Gate {
            gate: gate("cells"),
            report: Some(PathBuf::from("target/cells.json")),
        }
    );
    assert!(parse_command(&strings(&["verify-cells", "--check-existing"]), &manifest).is_err());
    assert!(parse_command(&strings(&["verify-all", "--refresh"]), &manifest).is_err());
    assert!(parse_command(&strings(&["help"]), &manifest).is_err());
}

#[test]
fn handoff_manifest_defines_the_ordered_v2_gate_inventory() {
    let (manifest, _) = load_manifest(&workspace_root()).unwrap();
    assert_eq!(
        manifest
            .gates
            .iter()
            .map(|entry| usize::from(entry.order))
            .collect::<Vec<_>>(),
        (0..manifest.gates.len()).collect::<Vec<_>>()
    );
    assert!(
        manifest
            .gates
            .iter()
            .all(|entry| !entry.output.as_str().contains("sidecar"))
    );
    let persons = manifest_gate(&manifest, "persons-pro");
    let profile = persons.profile.as_ref().expect("Persons.pro profile");
    assert_eq!(profile.argument("--example"), Some("persons_pro"));
    assert_eq!(profile.proof_requirements.checkpoints.len(), 25);
    let checkpoint_ids = profile
        .proof_requirements
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for required in [
        "published-child-visible",
        "passkey-failure-preserves-anonymous",
        "duplicate-passkey-rejected",
        "authentication-cancel-preserves-sign-out",
        "authentication-failure-preserves-sign-out",
    ] {
        assert!(checkpoint_ids.contains(required));
    }
    assert!(
        profile
            .proof_requirements
            .scenario
            .as_ref()
            .is_some_and(|scenario| scenario.semantic_assertions)
    );
}

#[test]
fn manifest_accepts_a_new_product_gate_without_a_rust_inventory_change() {
    let mut manifest = load_manifest(&workspace_root()).unwrap().0;
    let insert_at = manifest.gates.len();
    manifest.gates.push(ManifestGate {
        order: u16::try_from(insert_at).unwrap(),
        gate: gate("future-product"),
        verifier: GateCommand::new("verify-future-product").unwrap(),
        runner: GateRunner::NativeProduct,
        output: RelativePath::new("target/reports/report-v2/future-product.json").unwrap(),
        byte_limit: 262_144,
        sidecar_byte_limit: 67_108_864,
        profile: manifest_gate(&manifest, "counter-dev").profile.clone(),
    });
    manifest.validate().unwrap();
    assert_eq!(
        manifest
            .gate_for_verifier("verify-future-product")
            .unwrap()
            .gate
            .slug(),
        "future-product"
    );
}

#[test]
fn manifest_rejects_duplicate_identifiers_and_non_contiguous_order() {
    let original = load_manifest(&workspace_root()).unwrap().0;

    let mut duplicate_gate = original.clone();
    duplicate_gate.gates[1].gate = duplicate_gate.gates[0].gate.clone();
    assert!(
        duplicate_gate
            .validate()
            .unwrap_err()
            .contains("duplicated")
    );

    let mut duplicate_verifier = original.clone();
    duplicate_verifier.gates[1].verifier = duplicate_verifier.gates[0].verifier.clone();
    assert!(
        duplicate_verifier
            .validate()
            .unwrap_err()
            .contains("duplicated")
    );

    let mut wrong_order = original;
    wrong_order.gates[1].order = 7;
    assert!(wrong_order.validate().unwrap_err().contains("expected 1"));
}

#[test]
fn manifest_rejects_duplicate_outputs_and_invalid_gate_identifiers() {
    let mut duplicate_output = load_manifest(&workspace_root()).unwrap().0;
    duplicate_output.gates[1].output = duplicate_output.gates[0].output.clone();
    assert!(duplicate_output.validate().unwrap_err().contains("output"));

    assert!(GateName::new("Invalid_Product").is_err());
    assert!(GateName::new("x".repeat(65)).is_err());
    assert!(GateCommand::new("future-product").is_err());
    assert!(GateCommand::new("verify-Invalid").is_err());
}

#[test]
fn manifest_rejects_profile_arguments_that_do_not_match_measurement_contract() {
    let mut manifest = load_manifest(&workspace_root()).unwrap().0;
    let persons = manifest
        .gates
        .iter_mut()
        .find(|entry| entry.gate.slug() == "persons-pro")
        .unwrap();
    persons
        .profile
        .as_mut()
        .unwrap()
        .arguments
        .iter_mut()
        .find(|argument| argument.flag.as_str() == "--scroll-samples")
        .unwrap()
        .value = RelativePath::new("0").unwrap();
    assert!(
        manifest
            .validate()
            .unwrap_err()
            .contains("measurements do not match")
    );
}

#[test]
fn valid_fail_report_is_structurally_distinct_from_invalid_report() {
    let blocker = detail("producer has not implemented v2 evidence");
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    let report = gate_report(
        manifest_gate(&manifest, "negative"),
        bounded_id("run-one"),
        expected_identity(),
        ReportStatus::Fail,
        GateEvidence {
            checks: vec![check(
                "producer-v2-evidence",
                CheckOutcome::Fail,
                blocker.as_str(),
            )],
            producer: Some(producer(Some(2))),
            profile: None,
            native: None,
            product_ux_timings: Vec::new(),
            async_proof_timing: None,
            artifacts: Vec::new(),
        },
        vec![blocker],
    )
    .unwrap();
    assert_eq!(report.status, ReportStatus::Fail);
    report.validate_shape().unwrap();

    let mut old_shape = serde_json::to_value(&report).unwrap();
    old_shape["report_version"] = serde_json::json!(1);
    assert!(serde_json::from_value::<crate::report_v2::GateReport>(old_shape).is_err());
}

#[test]
fn stale_identity_is_rejected() {
    let report = passing_timed_report("cells");
    let mut stale = expected_identity();
    stale.source.workspace_digest = digest('9');
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    assert!(
        report
            .validate_current(manifest_gate(&manifest, "cells"), &stale)
            .unwrap_err()
            .contains("stale source identity")
    );
}

#[test]
fn mismatched_and_first_frame_proofs_are_rejected() {
    let mut mismatched = passing_timed_report("cells");
    mismatched
        .evidence
        .async_proof_timing
        .as_mut()
        .unwrap()
        .captured_frame
        .present_id += 1;
    assert!(
        mismatched
            .validate_shape()
            .unwrap_err()
            .contains("frame identity")
    );

    let mut first_frame = passing_timed_report("cells");
    first_frame.evidence.product_ux_timings[0]
        .representative_frame
        .frame_id = 1;
    assert!(
        first_frame
            .validate_shape()
            .unwrap_err()
            .contains("stale first/warmup frame")
    );
}

#[test]
fn hash_only_proof_is_rejected_by_strict_artifact_shape() {
    let report = passing_timed_report("cells");
    let mut value = serde_json::to_value(report).unwrap();
    value["evidence"]["artifacts"][0]
        .as_object_mut()
        .unwrap()
        .remove("path");
    assert!(serde_json::from_value::<crate::report_v2::GateReport>(value).is_err());
}

#[test]
fn persons_profile_rejects_unproven_semantic_scenario_and_missing_checkpoint() {
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    let entry = manifest_gate(&manifest, "persons-pro");
    let expected = expected_identity();

    let mut missing_semantics = passing_timed_report("persons-pro");
    let scenario = missing_semantics
        .evidence
        .profile
        .as_mut()
        .unwrap()
        .scenario
        .as_mut()
        .unwrap();
    scenario.semantic_assertions_proven = false;
    scenario.boundary = ScenarioBoundary::NativeTestPlayback;
    assert!(
        missing_semantics
            .validate_current(entry, &expected)
            .unwrap_err()
            .contains("semantic scenario proof")
    );

    let mut missing_checkpoint = passing_timed_report("persons-pro");
    let removed = missing_checkpoint
        .evidence
        .profile
        .as_mut()
        .unwrap()
        .checkpoints
        .pop()
        .unwrap();
    assert!(
        missing_checkpoint
            .validate_current(entry, &expected)
            .unwrap_err()
            .contains(removed.id.as_str())
    );
}

#[test]
fn persons_profile_rejects_budget_observation_over_its_limit() {
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    let entry = manifest_gate(&manifest, "persons-pro");
    let expected = expected_identity();
    let mut report = passing_timed_report("persons-pro");
    let observation = &mut report
        .evidence
        .profile
        .as_mut()
        .unwrap()
        .budget
        .as_mut()
        .unwrap()
        .observations[0];
    observation.observed = observation.limit + 1;
    assert!(
        report
            .validate_current(entry, &expected)
            .unwrap_err()
            .contains("exceeds its limit")
    );
}

#[test]
fn aggregate_requires_exact_current_manifest_gate_semantics() {
    let (manifest, _) = load_manifest(&workspace_root()).unwrap();
    let expected = expected_identity();
    let manifest_digest = digest('7');
    let mut aggregate = passing_aggregate(&manifest, &expected, manifest_digest.clone());
    aggregate
        .validate(&manifest, &manifest_digest, &expected)
        .unwrap();

    aggregate.gates[2].outcome = Some(ReportStatus::Fail);
    assert!(
        aggregate
            .validate(&manifest, &manifest_digest, &expected)
            .unwrap_err()
            .contains("passing aggregate")
    );
    aggregate.status = ReportStatus::Fail;
    aggregate.blockers = vec![detail("todomvc physical failed")];
    aggregate
        .validate(&manifest, &manifest_digest, &expected)
        .unwrap();

    aggregate.gates.pop();
    assert!(
        aggregate
            .validate(&manifest, &manifest_digest, &expected)
            .unwrap_err()
            .contains("exactly 7")
    );
}

#[test]
fn fresh_aggregate_rejects_reports_from_another_run() {
    let (manifest, _) = load_manifest(&workspace_root()).unwrap();
    let expected = expected_identity();
    let manifest_digest = digest('8');
    let mut aggregate = passing_aggregate(&manifest, &expected, manifest_digest.clone());
    aggregate.gates[0].run_id = Some(bounded_id("older-run"));
    assert!(
        aggregate
            .validate(&manifest, &manifest_digest, &expected)
            .unwrap_err()
            .contains("run identity mismatch")
    );
}

fn passing_timed_report(gate: &str) -> crate::report_v2::GateReport {
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    let entry = manifest_gate(&manifest, gate);
    let frame = frame_key();
    let MeasurementContract::Timed {
        product_ux,
        async_proof,
    } = measurement_contract(entry)
    else {
        panic!("test gate must be timed");
    };
    let product_ux_timings = product_ux
        .iter()
        .map(|definition| ProductTimingEvidence {
            metric: definition.metric,
            representative_frame: frame.clone(),
            representative_sample_ordinal: definition.samples.warmup_samples + 1,
            summary: summary(definition.samples.minimum_samples, 500),
        })
        .collect::<Vec<_>>();
    let linked_product_metric = product_ux_timings[0].metric;
    let artifact_id = bounded_id("proof-png");
    gate_report(
        entry,
        bounded_id("fresh-run"),
        expected_identity(),
        ReportStatus::Pass,
        GateEvidence {
            checks: vec![check(
                "product-contract",
                CheckOutcome::Pass,
                "all evidence passed",
            )],
            producer: Some(producer(Some(0))),
            profile: Some(complete_profile_evidence(entry)),
            native: Some(native_evidence()),
            product_ux_timings,
            async_proof_timing: Some(AsyncProofTimingEvidence {
                linked_product_metric,
                captured_frame: frame.clone(),
                completed_after_frame_id: frame.frame_id + 2,
                proof_lag_frames: 2,
                artifact_id: artifact_id.clone(),
                snapshot_prepare_us: 100,
                worker_us: 900,
                summary: summary(async_proof.samples.minimum_samples, 1_000),
            }),
            artifacts: vec![ArtifactMetadata {
                artifact_id,
                kind: ArtifactKind::WgpuPngReadback,
                path: RelativePath::new("target/reports/report-v2/artifacts/proof.png").unwrap(),
                sha256: digest('3'),
                byte_len: 64,
                frame,
            }],
        },
        Vec::new(),
    )
    .unwrap()
}

fn complete_profile_evidence(entry: &ManifestGate) -> VerificationProfileEvidence {
    let profile = entry.profile.as_ref().expect("product verifier profile");
    let requirements = &profile.proof_requirements;
    let checkpoint_count = requirements.checkpoints.len().max(1) as u32;
    VerificationProfileEvidence {
        profile_id: profile.id.clone(),
        profile_digest: profile.digest(),
        scenario: requirements
            .scenario
            .as_ref()
            .map(|scenario| ScenarioProof {
                path: scenario.path.clone(),
                sha256: digest('4'),
                boundary: if scenario.semantic_assertions {
                    ScenarioBoundary::NativeTestPlaybackAndSemanticAssertions
                } else {
                    ScenarioBoundary::NativeTestPlayback
                },
                request_id: Some(1),
                declared_steps: checkpoint_count,
                executable_steps: checkpoint_count,
                completed_steps: checkpoint_count,
                passed: true,
                semantic_assertions_proven: scenario.semantic_assertions,
            }),
        budget: requirements.budget.as_ref().map(|budget| BudgetProof {
            path: budget.path.clone(),
            sha256: digest('5'),
            observations: budget
                .metrics
                .iter()
                .cloned()
                .map(|metric| BudgetObservation {
                    metric,
                    unit: BudgetUnit::Count,
                    comparison: BudgetComparison::AtMost,
                    observed: 1,
                    limit: 1,
                })
                .collect(),
        }),
        state_root: requirements
            .state_root
            .as_ref()
            .map(|state_root| StateRootProof {
                root: ShortText::new("target/reports/report-v2/state/test-run").unwrap(),
                policy: state_root.policy,
                clean_at_start: true,
                durable_file_count: 1,
                restart_count: u32::from(state_root.restart_required),
                restored_after_restart: state_root.restart_required,
            }),
        checkpoints: requirements
            .checkpoints
            .iter()
            .cloned()
            .map(|requirement| StateCheckpointProof {
                id: requirement.id,
                source_revision: 1,
                runtime_sequence: 1,
                durable_epoch: 1,
                state_digest: digest('6'),
                frame: frame_key(),
                evidence: match requirement.evidence {
                    CheckpointEvidenceRequirement::ScenarioStep { scenario_step } => {
                        StateCheckpointEvidence::ScenarioSemanticFrame {
                            scenario_step,
                            assertion_count: 1,
                        }
                    }
                    CheckpointEvidenceRequirement::RestartRestore {
                        baseline_checkpoint,
                    } => StateCheckpointEvidence::RestartRestore {
                        baseline_checkpoint,
                        before_restart_digest: digest('6'),
                        startup_restored: true,
                    },
                    CheckpointEvidenceRequirement::ResponsiveLayout {
                        baseline_checkpoint,
                        logical_width,
                    } => StateCheckpointEvidence::ResponsiveLayout {
                        baseline_checkpoint,
                        logical_width,
                        logical_height: 844,
                        action_count: 1,
                        action_digest: digest('7'),
                    },
                    CheckpointEvidenceRequirement::StaleCompileRejection => {
                        StateCheckpointEvidence::StaleCompileRejection {
                            session: bounded_id("test-program"),
                            stale_revision: 1,
                            latest_revision: 2,
                        }
                    }
                    CheckpointEvidenceRequirement::PersistenceOperation { operation } => {
                        StateCheckpointEvidence::PersistenceOperation {
                            operation,
                            before_state_digest: digest('6'),
                        }
                    }
                },
            })
            .collect(),
    }
}

fn passing_aggregate(
    manifest: &crate::report_v2::HandoffManifest,
    expected: &ExpectedIdentity,
    manifest_digest: Sha256Digest,
) -> AggregateReport {
    let run_id = bounded_id("aggregate-run");
    let gates = manifest
        .gates
        .iter()
        .map(|entry| AggregateGateResult {
            gate: entry.gate.clone(),
            verifier: entry.verifier.clone(),
            report: Some(ReportFileMetadata {
                path: entry.output.clone(),
                sha256: digest('6'),
                byte_len: 512,
            }),
            validation: ChildValidation::Valid,
            outcome: Some(ReportStatus::Pass),
            report_id: Some(bounded_id(&format!("{}-report", entry.gate.slug()))),
            run_id: Some(run_id.clone()),
            issue: None,
        })
        .collect();
    AggregateReport {
        format: FORMAT_VERSION,
        kind: AggregateReportKind::Aggregate,
        identity: AggregateIdentity {
            report_id: bounded_id("aggregate-report"),
            run_id,
            source: expected.source.clone(),
            tooling: expected.tooling.clone(),
            generated_unix_ms: 1,
        },
        mode: AggregateMode::Fresh,
        manifest: ManifestIdentity {
            id: manifest.id.clone(),
            digest: manifest_digest,
        },
        status: ReportStatus::Pass,
        gates,
        blockers: Vec::new(),
    }
}

fn expected_identity() -> ExpectedIdentity {
    ExpectedIdentity {
        source: SourceIdentity {
            head: crate::report_v2::GitCommit::new("0".repeat(40)).unwrap(),
            workspace_digest: digest('1'),
            dirty: true,
        },
        tooling: ToolIdentity {
            contract: BoundedString::new("boon-xtask-report-v2").unwrap(),
            contract_digest: digest('2'),
        },
    }
}

fn native_evidence() -> NativeEvidence {
    NativeEvidence {
        adapter_name: ShortText::new("test hardware adapter").unwrap(),
        adapter_backend: AdapterBackend::Vulkan,
        adapter_device_type: AdapterDeviceType::DiscreteGpu,
        software_adapter: false,
        present_mode: PresentMode::Fifo,
        surface_format: BoundedString::new("bgra8unorm-srgb").unwrap(),
        window_backend: WindowBackend::Wayland,
        preview_pid: 100,
        dev_pid: 101,
        input_delivery: InputDelivery::NativeOsAppWindowCallback,
        scenario_boundary: HostBoundary::PublicHostEvent,
        capture_method: CaptureMethod::AppOwnedWgpuReadback,
        private_runtime_dispatch_used: false,
        launch_isolation: vec![LaunchIsolationEvidence {
            phase: LaunchIsolationPhase::Primary,
            session_id: ShortText::new("session-primary").unwrap(),
            seat_name: ShortText::new("boon-verifier-seat").unwrap(),
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
        }],
    }
}

fn producer(exit_code: Option<i32>) -> ProducerEvidence {
    ProducerEvidence {
        program: ShortText::new("target/release/boon_native_playground").unwrap(),
        protocol: protocol_name(),
        exit_code,
        elapsed_ms: 10,
    }
}

fn frame_key() -> FrameEvidenceKey {
    FrameEvidenceKey {
        surface_id: ShortText::new("preview-surface").unwrap(),
        process_id: 100,
        session_id: ShortText::new("session-primary").unwrap(),
        frame_id: 100,
        input_id: 20,
        content_id: 30,
        layout_id: 40,
        render_id: 50,
        surface_epoch: 2,
        present_id: 60,
        proof_id: 70,
    }
}

fn summary(sample_count: u32, base: u64) -> TimingSummary {
    TimingSummary {
        sample_count,
        p50_us: base,
        p95_us: base,
        p99_us: base,
        max_us: base,
        outlier_count: 0,
    }
}

fn bounded_id(value: &str) -> BoundedId {
    BoundedId::new(value).unwrap()
}

fn digest(character: char) -> Sha256Digest {
    Sha256Digest::new(character.to_string().repeat(64)).unwrap()
}
