use super::*;
use crate::report_v2::{
    AdapterBackend, AdapterDeviceType, AggregateGateResult, AggregateIdentity, AggregateMode,
    AggregateReport, AggregateReportKind, ArtifactKind, ArtifactMetadata, AsyncProofTimingEvidence,
    BoundedId, BoundedString, CaptureMethod, CheckOutcome, ChildValidation, ExpectedIdentity,
    FORMAT_VERSION, FrameEvidenceKey, GateEvidence, GateName, HostBoundary, InputDelivery,
    ManifestIdentity, MeasurementContract, NativeEvidence, PresentMode, ProducerEvidence,
    ProductTimingEvidence, RelativePath, ReportFileMetadata, ReportStatus, Sha256Digest, ShortText,
    SourceIdentity, TimingSummary, ToolIdentity, WindowBackend, check, detail, gate_report,
    load_manifest, measurement_contract, protocol_name,
};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn command_parser_exposes_exactly_eight_commands() {
    let names = PublicCommand::ALL
        .iter()
        .map(|command| command.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "shaders",
            "verify-architecture",
            "verify-counter-dev",
            "verify-todomvc-physical",
            "verify-cells",
            "verify-novywave",
            "verify-negative",
            "verify-all",
        ]
    );
    for name in names {
        assert!(PublicCommand::parse(name).is_some());
    }
    assert!(PublicCommand::parse("help").is_none());
    assert!(PublicCommand::parse("verify-native-gpu-all").is_none());
    assert!(PublicCommand::parse("verify-report-schema").is_none());
}

#[test]
fn command_parser_accepts_only_v2_options() {
    assert_eq!(
        parse_command(&strings(&["shaders", "--check"])).unwrap(),
        ParsedCommand::Shaders { check: true }
    );
    assert_eq!(
        parse_command(&strings(&[
            "verify-all",
            "--check-existing",
            "--report",
            "target/custom.json",
        ]))
        .unwrap(),
        ParsedCommand::VerifyAll {
            check_existing: true,
            report: Some(PathBuf::from("target/custom.json")),
        }
    );
    assert_eq!(
        parse_command(&strings(
            &["verify-cells", "--report", "target/cells.json",]
        ))
        .unwrap(),
        ParsedCommand::Gate {
            command: PublicCommand::VerifyCells,
            report: Some(PathBuf::from("target/cells.json")),
        }
    );
    assert!(parse_command(&strings(&["verify-cells", "--check-existing"])).is_err());
    assert!(parse_command(&strings(&["verify-all", "--refresh"])).is_err());
    assert!(parse_command(&strings(&["help"])).is_err());
}

#[test]
fn handoff_manifest_is_exactly_the_six_v2_gates() {
    let (manifest, _) = load_manifest(&workspace_root()).unwrap();
    assert_eq!(manifest.gates.len(), 6);
    assert_eq!(
        manifest
            .gates
            .iter()
            .map(|entry| (entry.gate, entry.verifier.as_str()))
            .collect::<Vec<_>>(),
        GateName::ALL
            .iter()
            .map(|gate| (*gate, gate.command().as_str()))
            .collect::<Vec<_>>()
    );
    assert!(
        manifest
            .gates
            .iter()
            .all(|entry| !entry.output.as_str().contains("sidecar"))
    );
}

#[test]
fn valid_fail_report_is_structurally_distinct_from_invalid_report() {
    let blocker = detail("producer has not implemented v2 evidence");
    let report = gate_report(
        GateName::Negative,
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
    let report = passing_timed_report(GateName::Cells);
    let mut stale = expected_identity();
    stale.source.workspace_digest = digest('9');
    let manifest = load_manifest(&workspace_root()).unwrap().0;
    assert!(
        report
            .validate_current(manifest.gate(GateName::Cells), &stale)
            .unwrap_err()
            .contains("stale source identity")
    );
}

#[test]
fn mismatched_and_first_frame_proofs_are_rejected() {
    let mut mismatched = passing_timed_report(GateName::Cells);
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

    let mut first_frame = passing_timed_report(GateName::Cells);
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
    let report = passing_timed_report(GateName::Cells);
    let mut value = serde_json::to_value(report).unwrap();
    value["evidence"]["artifacts"][0]
        .as_object_mut()
        .unwrap()
        .remove("path");
    assert!(serde_json::from_value::<crate::report_v2::GateReport>(value).is_err());
}

#[test]
fn aggregate_requires_exact_current_six_gate_semantics() {
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
            .contains("exactly six")
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

fn passing_timed_report(gate: GateName) -> crate::report_v2::GateReport {
    let frame = frame_key();
    let MeasurementContract::Timed {
        product_ux,
        async_proof,
    } = measurement_contract(gate)
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
        gate,
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
            gate: entry.gate,
            verifier: entry.verifier,
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
