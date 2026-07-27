use boon_phase0_baseline::manifest::FixtureManifest;
use boon_phase0_baseline::report::{
    BaselineReport, DEFAULT_BUDGET, DEFAULT_MANIFEST, DEFAULT_REPORT, MAX_REPORT_BYTES,
    ReportStatus as BaselineStatus, SourceIdentity,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::report_v2::{ReportStatus, ToolResult, current_identity, sha256_file};

pub fn run(
    workspace: &Path,
    check_existing: bool,
    output: Option<PathBuf>,
) -> ToolResult<ReportStatus> {
    let report_path = output.unwrap_or_else(|| workspace.join(DEFAULT_REPORT));
    if !check_existing {
        collect(workspace, &report_path)?;
    }
    let report = validate_existing(workspace, &report_path)?;
    println!(
        "{} packed baseline {}: {} fixtures, {} aggregate metrics",
        if check_existing { "checked" } else { "wrote" },
        report_path.display(),
        report.fixtures.len(),
        report.metrics.len()
    );
    Ok(match report.status {
        BaselineStatus::Pass => ReportStatus::Pass,
        BaselineStatus::Fail => ReportStatus::Fail,
    })
}

fn collect(workspace: &Path, report_path: &Path) -> ToolResult<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let build = Command::new(cargo)
        .current_dir(workspace)
        .args([
            "build",
            "--locked",
            "--release",
            "-p",
            "boon_phase0_baseline",
            "--bin",
            "boon_phase0_baseline",
        ])
        .status()?;
    if !build.success() {
        return Err(format!("release packed-baseline producer build failed with {build}").into());
    }

    let before = current_identity(workspace)?;
    let binary = workspace.join("target/release/boon_phase0_baseline");
    if !binary.is_file() {
        return Err(format!(
            "release packed-baseline producer is missing at {}",
            binary.display()
        )
        .into());
    }
    let manifest = workspace.join(DEFAULT_MANIFEST);
    let budget = workspace.join(DEFAULT_BUDGET);
    let producer = Command::new(&binary)
        .current_dir(workspace)
        .arg("--manifest")
        .arg(&manifest)
        .arg("--report")
        .arg(report_path)
        .arg("--budget")
        .arg(&budget)
        .args([
            "--source-head",
            before.source.head.as_str(),
            "--workspace-digest",
            before.source.workspace_digest.as_str(),
            "--dirty",
            if before.source.dirty { "true" } else { "false" },
        ])
        .status()?;
    if !producer.success() {
        return Err(format!("packed-baseline producer failed with {producer}").into());
    }
    let after = current_identity(workspace)?;
    if before != after {
        return Err(
            "workspace identity changed while the packed baseline was being measured; the written report is stale"
                .into(),
        );
    }
    Ok(())
}

pub fn validate_existing(workspace: &Path, path: &Path) -> ToolResult<BaselineReport> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_REPORT_BYTES as u64 {
        return Err(format!(
            "{} is not a regular packed-baseline report of 1..={MAX_REPORT_BYTES} bytes",
            path.display()
        )
        .into());
    }
    let bytes = fs::read(path)?;
    let report: BaselineReport = serde_json::from_slice(&bytes)?;
    report
        .validate()
        .map_err(|error| format!("{}: {error}", path.display()))?;

    let expected = current_identity(workspace)?;
    let source = SourceIdentity {
        head: expected.source.head.as_str().to_owned(),
        workspace_digest: expected.source.workspace_digest.as_str().to_owned(),
        dirty: expected.source.dirty,
    };
    if report.source != source {
        return Err(format!(
            "{} source identity is stale for the current workspace",
            path.display()
        )
        .into());
    }
    if report.fixture_manifest_path != DEFAULT_MANIFEST {
        return Err(format!(
            "{} fixture manifest is `{}`; expected `{DEFAULT_MANIFEST}`",
            path.display(),
            report.fixture_manifest_path
        )
        .into());
    }
    if report.budget_manifest_path != DEFAULT_BUDGET {
        return Err(format!(
            "{} budget manifest is `{}`; expected `{DEFAULT_BUDGET}`",
            path.display(),
            report.budget_manifest_path
        )
        .into());
    }
    let manifest_digest = sha256_file(&workspace.join(DEFAULT_MANIFEST))?;
    if report.fixture_manifest_sha256 != manifest_digest.as_str() {
        return Err(format!(
            "{} fixture manifest digest does not match the current deterministic manifest",
            path.display()
        )
        .into());
    }
    let budget_digest = sha256_file(&workspace.join(DEFAULT_BUDGET))?;
    if report.budget_manifest_sha256 != budget_digest.as_str() {
        return Err(format!(
            "{} budget manifest digest does not match the current checked budget",
            path.display()
        )
        .into());
    }
    let (fixture_manifest, _) = FixtureManifest::load(&workspace.join(DEFAULT_MANIFEST))
        .map_err(|error| format!("{DEFAULT_MANIFEST}: {error}"))?;
    for definition in &fixture_manifest.fixtures {
        let fixture = report
            .fixtures
            .iter()
            .find(|fixture| fixture.id == definition.id)
            .ok_or_else(|| format!("report omits fixture {}", definition.id))?;
        if fixture.source_path != definition.source
            || fixture.expected_authoritative_rows != definition.expected_authoritative_rows
            || fixture.actions.len() != definition.actions.len()
        {
            return Err(format!(
                "report fixture {} differs from the deterministic fixture manifest",
                definition.id
            )
            .into());
        }
        for action_definition in &definition.actions {
            let action = fixture
                .actions
                .iter()
                .find(|action| action.id == action_definition.id)
                .ok_or_else(|| {
                    format!(
                        "report fixture {} omits action {}",
                        definition.id, action_definition.id
                    )
                })?;
            if action.class != action_definition.class
                || action.target != action_definition.target
                || action.warmup_turns != action_definition.warmup_turns
                || action.measured_turns != action_definition.measured_turns
                || action.semantic_rows_per_turn != action_definition.semantic_rows_per_turn
            {
                return Err(format!(
                    "report fixture {} action {} differs from the deterministic fixture manifest",
                    definition.id, action_definition.id
                )
                .into());
            }
        }
    }
    let producer = workspace.join("target/release/boon_phase0_baseline");
    let producer_digest = sha256_file(&producer)?;
    if report.binary_sha256 != producer_digest.as_str() {
        return Err(format!(
            "{} producer digest does not match {}",
            path.display(),
            producer.display()
        )
        .into());
    }
    Ok(report)
}
