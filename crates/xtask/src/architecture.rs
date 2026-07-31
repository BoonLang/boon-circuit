use crate::report_v2::{CheckOutcome, GateEvidence, check, empty_evidence};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::visit::Visit;

const TOTAL_RUST_CAP: usize = 240_000;
const TEST_RUST_CAP: usize = 32_000;
const PLAYGROUND_RUST_CAP: usize = 32_000;
const XTASK_RUST_CAP: usize = 25_000;
const RUNTIME_EXECUTOR_RUST_CAP: usize = 42_000;
const APP_WINDOW_FORK_NET_LOC_CAP: usize = 1_200;

pub fn collect_architecture_evidence(workspace: &Path) -> GateEvidence {
    let mut checks = Vec::new();

    push_check(
        &mut checks,
        "no-vendored-app-window",
        no_vendored_app_window(workspace),
    );
    push_check(
        &mut checks,
        "immutable-app-window-fork",
        immutable_app_window_fork(workspace),
    );
    push_check(
        &mut checks,
        "app-window-fork-net-loc-cap",
        app_window_fork_net_lines(workspace).and_then(|(observed, detail)| {
            (observed <= APP_WINDOW_FORK_NET_LOC_CAP)
                .then_some(format!(
                    "{detail}; net code lines: {observed}; cap: {APP_WINDOW_FORK_NET_LOC_CAP}"
                ))
                .ok_or_else(|| {
                    format!(
                        "{detail}; net code lines: {observed}; cap: {APP_WINDOW_FORK_NET_LOC_CAP}"
                    )
                })
        }),
    );
    push_check(
        &mut checks,
        "no-report-schema-crate",
        no_report_schema(workspace),
    );
    push_check(
        &mut checks,
        "no-executable-3d-manufacturing-island",
        no_executable_island(workspace),
    );
    push_check(
        &mut checks,
        "no-product-serde-json",
        no_forbidden_product_json(workspace),
    );
    push_check(
        &mut checks,
        "single-machine-plan-executor-path",
        single_execution_path(workspace),
    );
    push_check(
        &mut checks,
        "verified-semantic-compiler-spine",
        verified_semantic_compiler_spine(workspace),
    );
    push_check(
        &mut checks,
        "dependency-classifier-schema-v1",
        crate::dependency_classifier::verify(workspace),
    );
    push_check(
        &mut checks,
        "canonical-checked-parameter-semantics",
        crate::dependency_classifier::verify_parameter_semantics_deletion(workspace),
    );
    push_check(
        &mut checks,
        "no-example-specific-engine-branches",
        no_example_specific_engine_branches(workspace),
    );
    push_check(
        &mut checks,
        "isolated-native-input-path",
        isolated_native_input_path(workspace),
    );

    match rust_line_counts(workspace) {
        Ok(counts) => {
            push_cap(
                &mut checks,
                "tracked-rust-loc-cap",
                counts.total,
                TOTAL_RUST_CAP,
                "tracked Rust",
            );
            push_cap(
                &mut checks,
                "test-rust-loc-cap",
                counts.tests,
                TEST_RUST_CAP,
                "test Rust",
            );
            push_cap(
                &mut checks,
                "playground-rust-loc-cap",
                counts.playground,
                PLAYGROUND_RUST_CAP,
                "playground production Rust",
            );
            push_cap(
                &mut checks,
                "xtask-rust-loc-cap",
                counts.xtask,
                XTASK_RUST_CAP,
                "xtask Rust",
            );
            push_cap(
                &mut checks,
                "runtime-executor-rust-loc-cap",
                counts.runtime_executor,
                RUNTIME_EXECUTOR_RUST_CAP,
                "runtime plus executor Rust",
            );
        }
        Err(error) => push_check(
            &mut checks,
            "architecture-loc-caps",
            Err(format!("could not count Rust lines: {error}")),
        ),
    }

    empty_evidence(checks)
}

fn isolated_native_input_path(workspace: &Path) -> Result<String, String> {
    let verifier =
        fs::read_to_string(workspace.join("crates/boon_native_playground/src/verify.rs"))
            .map_err(|error| format!("read native verifier: {error}"))?;
    let input =
        fs::read_to_string(workspace.join("crates/boon_native_playground/src/native_input.rs"))
            .map_err(|error| format!("read native input role: {error}"))?;
    let workspace_control = fs::read_to_string(
        workspace.join("crates/boon_native_playground/src/workspace_control.rs"),
    )
    .map_err(|error| format!("read workspace control role: {error}"))?;

    let required = [
        (verifier.as_str(), "\"--isolated-input\""),
        (verifier.as_str(), "\"--isolation-status\""),
        (verifier.as_str(), "wait_for_isolated_input"),
        (verifier.as_str(), "maximized_windows="),
        (verifier.as_str(), "require_layout"),
        (
            verifier.as_str(),
            "window_scan_candidates(session.pointer_space()?",
        ),
        (input.as_str(), "COSMIC Isolated {seat_name} {kind}"),
    ];
    if let Some((_, missing)) = required
        .into_iter()
        .find(|(source, needle)| !source.contains(needle))
    {
        return Err(format!(
            "native input isolation contract omitted `{missing}`"
        ));
    }
    if workspace_control.contains(".activate()") {
        return Err(
            "workspace-control must not activate or restore a user workspace during automation"
                .to_owned(),
        );
    }
    Ok(
        "native verifier requires a launch-scoped COSMIC seat and keeps its workspace inactive"
            .to_owned(),
    )
}

fn push_check(
    checks: &mut Vec<crate::report_v2::CheckEvidence>,
    id: &'static str,
    result: Result<String, String>,
) {
    match result {
        Ok(detail) => checks.push(check(id, CheckOutcome::Pass, detail)),
        Err(detail) => checks.push(check(id, CheckOutcome::Fail, detail)),
    }
}

fn push_cap(
    checks: &mut Vec<crate::report_v2::CheckEvidence>,
    id: &'static str,
    observed: usize,
    cap: usize,
    label: &str,
) {
    let detail = format!("{label}: {observed} lines; cap: {cap}");
    push_check(
        checks,
        id,
        (observed <= cap).then_some(detail.clone()).ok_or(detail),
    );
}

fn no_vendored_app_window(workspace: &Path) -> Result<String, String> {
    let vendor = workspace.join("vendor/app_window");
    let root = read_text(&workspace.join("Cargo.toml"))?;
    let has_local_reference =
        root.contains("vendor/app_window") || root.contains("path = \"vendor/app_window\"");
    if vendor.exists() || has_local_reference {
        return Err(format!(
            "vendor/app_window exists={} root manifest references it={has_local_reference}",
            vendor.exists()
        ));
    }
    Ok("no workspace-local app_window copy or path reference".to_owned())
}

fn immutable_app_window_fork(workspace: &Path) -> Result<String, String> {
    let root = parse_toml(&workspace.join("Cargo.toml"))?;
    let dependency = root
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .and_then(|dependencies| dependencies.get("app_window"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "workspace app_window dependency must be a git table".to_owned())?;
    let git = dependency
        .get("git")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "app_window dependency is not pinned to a git fork".to_owned())?;
    let revision = dependency
        .get("rev")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "app_window dependency has no immutable rev".to_owned())?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("app_window rev must be a full 40-character commit".to_owned());
    }
    if dependency.contains_key("path")
        || dependency.contains_key("branch")
        || dependency.contains_key("tag")
    {
        return Err("app_window dependency must use only git plus immutable rev".to_owned());
    }
    Ok(format!("immutable app_window fork {git}@{revision}"))
}

fn app_window_fork_net_lines(workspace: &Path) -> Result<(usize, String), String> {
    let root = parse_toml(&workspace.join("Cargo.toml"))?;
    let dependency = root
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
        .and_then(|dependencies| dependencies.get("app_window"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "workspace app_window dependency must be a git table".to_owned())?;
    let git = dependency
        .get("git")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "cannot count fork lines for a non-git app_window dependency".to_owned())?;
    let revision = dependency
        .get("rev")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "cannot count fork lines without an immutable app_window rev".to_owned())?;
    let repository = local_git_repository(workspace, git)?;
    let parent = format!("{revision}^");
    let base = git_text(&repository, &["describe", "--tags", "--abbrev=0", &parent])?;
    let range = format!("{base}..{revision}");
    let numstat = git_text(
        &repository,
        &[
            "diff",
            "--numstat",
            &range,
            "--",
            "*.rs",
            "*.swift",
            "*.c",
            "*.cc",
            "*.cpp",
            "*.h",
            "*.hpp",
            "*.m",
            "*.mm",
            "*.java",
            "*.kt",
        ],
    )?;
    let mut additions = 0_usize;
    let mut deletions = 0_usize;
    for line in numstat.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split('\t');
        let added = fields
            .next()
            .ok_or_else(|| format!("invalid app_window numstat line: {line}"))?
            .parse::<usize>()
            .map_err(|_| format!("binary/invalid app_window numstat line: {line}"))?;
        let deleted = fields
            .next()
            .ok_or_else(|| format!("invalid app_window numstat line: {line}"))?
            .parse::<usize>()
            .map_err(|_| format!("binary/invalid app_window numstat line: {line}"))?;
        additions = additions.saturating_add(added);
        deletions = deletions.saturating_add(deleted);
    }
    Ok((
        additions.saturating_sub(deletions),
        format!("app_window {base}..{}", &revision[..revision.len().min(12)]),
    ))
}

fn local_git_repository(workspace: &Path, git: &str) -> Result<PathBuf, String> {
    let direct = git.strip_prefix("file://").map(PathBuf::from);
    let sibling = git
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .map(|name| name.trim_end_matches(".git"))
        .and_then(|name| workspace.parent().map(|parent| parent.join(name)));
    direct
        .into_iter()
        .chain(sibling)
        .find(|path| path.join(".git").exists())
        .ok_or_else(|| format!("no local checkout is available to inspect app_window fork {git}"))
}

fn git_text(repository: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "git {} in {} failed: {}",
            args.join(" "),
            repository.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|error| error.to_string())
}

fn no_report_schema(workspace: &Path) -> Result<String, String> {
    let crate_path = workspace.join("crates/boon_report_schema");
    let mut references = Vec::new();
    for relative in workspace_files(workspace)? {
        if relative.starts_with("crates/xtask/")
            || !(relative == "Cargo.toml"
                || relative == "Cargo.lock"
                || relative.ends_with("Cargo.toml")
                || relative.ends_with(".rs"))
        {
            continue;
        }
        let path = workspace.join(&relative);
        if path.is_file()
            && fs::read_to_string(&path)
                .map(|text| text.contains("boon_report_schema"))
                .unwrap_or(false)
        {
            references.push(relative);
        }
    }
    if crate_path.exists() || !references.is_empty() {
        return Err(format!(
            "schema directory exists={} references={}",
            crate_path.exists(),
            bounded_list(&references)
        ));
    }
    Ok("boon_report_schema directory and product references are absent".to_owned())
}

fn no_executable_island(workspace: &Path) -> Result<String, String> {
    let offenders = workspace_files(workspace)?
        .into_iter()
        .filter(|path| path.starts_with("crates/") || path.starts_with("examples/"))
        .filter(|path| {
            path.split('/').any(|component| {
                let normalized = component.to_ascii_lowercase().replace(['-', '.'], "_");
                normalized == "3d"
                    || normalized.starts_with("3d_")
                    || normalized.ends_with("_3d")
                    || normalized.contains("three_d")
                    || normalized.contains("manufactur")
                    || normalized.starts_with("cad_")
                    || normalized.contains("_cad_")
            })
        })
        .collect::<Vec<_>>();
    if offenders.is_empty() {
        Ok("no executable 3D/manufacturing crate or example paths".to_owned())
    } else {
        Err(format!(
            "executable island paths: {}",
            bounded_list(&offenders)
        ))
    }
}

fn no_forbidden_product_json(workspace: &Path) -> Result<String, String> {
    let mut offenders = Vec::new();
    for entry in fs::read_dir(workspace.join("crates")).map_err(|error| error.to_string())? {
        let path = entry
            .map_err(|error| error.to_string())?
            .path()
            .join("Cargo.toml");
        if !path.is_file() {
            continue;
        }
        let manifest = parse_toml(&path)?;
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or("<unknown>");
        if matches!(package, "boon_cli" | "boon_phase0_baseline" | "xtask") {
            continue;
        }
        if package == "boon_native_playground" {
            let source_dir = workspace.join("crates/boon_native_playground/src");
            let mut product_json = Vec::new();
            for source in fs::read_dir(&source_dir).map_err(|error| error.to_string())? {
                let source = source.map_err(|error| error.to_string())?.path();
                if source.extension().and_then(|value| value.to_str()) != Some("rs")
                    || source.file_name().and_then(|value| value.to_str()) == Some("verify.rs")
                {
                    continue;
                }
                if fs::read_to_string(&source)
                    .map_err(|error| error.to_string())?
                    .contains("serde_json")
                {
                    product_json.push(source.display().to_string());
                }
            }
            if product_json.is_empty() {
                continue;
            }
            offenders.extend(product_json);
            continue;
        }
        if manifest_has_production_dependency(&manifest, "serde_json") {
            offenders.push(package.to_owned());
        }
    }
    offenders.sort();
    if offenders.is_empty() {
        Ok("serde_json is limited to CLI, xtask, and the native verifier role".to_owned())
    } else {
        Err(format!(
            "product crates depending on serde_json: {}",
            bounded_list(&offenders)
        ))
    }
}

fn manifest_has_production_dependency(value: &toml::Value, dependency: &str) -> bool {
    let Some(table) = value.as_table() else {
        return false;
    };
    for key in ["dependencies", "build-dependencies"] {
        if table
            .get(key)
            .and_then(toml::Value::as_table)
            .is_some_and(|dependencies| dependencies.contains_key(dependency))
        {
            return true;
        }
    }
    table.iter().any(|(key, child)| {
        key != "dev-dependencies"
            && !key.ends_with(".dev-dependencies")
            && manifest_has_production_dependency(child, dependency)
    })
}

fn single_execution_path(workspace: &Path) -> Result<String, String> {
    let files = workspace_files(workspace)?;
    let mut machine_plan_definitions = Vec::new();
    let mut machine_instance_definitions = Vec::new();
    let mut machine_template_definitions = Vec::new();
    let mut forbidden_runtime_types = Vec::new();
    for relative in &files {
        if !relative.starts_with("crates/")
            || relative.starts_with("crates/xtask/")
            || !relative.ends_with(".rs")
            || relative.contains("/tests/")
            || relative.ends_with("/tests.rs")
        {
            continue;
        }
        let path = workspace.join(relative);
        if !path.is_file() {
            continue;
        }
        let text = read_text(&path)?;
        for _ in exact_struct_definition_lines(&text, "MachinePlan") {
            machine_plan_definitions.push(relative.clone());
        }
        for _ in exact_struct_definition_lines(&text, "MachineInstance") {
            machine_instance_definitions.push(relative.clone());
        }
        for _ in exact_struct_definition_lines(&text, "MachineTemplate") {
            machine_template_definitions.push(relative.clone());
        }
        for marker in [
            "struct PlanExecutorLiveSession",
            "struct PlanExecutorRuntimeState",
            "struct PlanExecutorOutputEvaluator",
            "struct PackedExecutor",
            "struct ReferenceExecutor",
        ] {
            for _ in text.match_indices(marker) {
                forbidden_runtime_types.push(format!("{relative}:{marker}"));
            }
        }
    }
    let executor_source = read_text(&workspace.join("crates/boon_plan_executor/src/lib.rs"))?;
    let executor_machine = read_text(&workspace.join("crates/boon_plan_executor/src/machine.rs"))?;
    let document_runtime = read_text(&workspace.join("crates/boon_document/src/runtime.rs"))?;
    let runtime_source = read_text(&workspace.join("crates/boon_runtime/src/lib.rs"))?;
    let expected_executor_machine = vec!["crates/boon_plan_executor/src/machine.rs".to_owned()];
    let machine_owned_by_executor = machine_instance_definitions == expected_executor_machine
        && machine_template_definitions == expected_executor_machine
        && executor_source.contains("MachineInstance")
        && executor_source.contains("MachineTemplate")
        && executor_machine.contains("struct MachineInstance")
        && executor_machine.contains("struct MachineTemplate");
    let runtime_uses_machine = runtime_source.contains("boon_plan_executor::MachineInstance")
        || (runtime_source.contains("use boon_plan_executor")
            && runtime_source.contains("MachineInstance"));
    let document_uses_machine = document_runtime.contains("use boon_plan_executor")
        && document_runtime.contains("MachineInstance");

    let mut direct_executor_dependents = Vec::new();
    let mut instrumentation_dependents = Vec::new();
    for entry in fs::read_dir(workspace.join("crates")).map_err(|error| error.to_string())? {
        let manifest_path = entry
            .map_err(|error| error.to_string())?
            .path()
            .join("Cargo.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = parse_toml(&manifest_path)?;
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or("<unknown>");
        let executor_dependency = manifest
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .and_then(|dependencies| dependencies.get("boon_plan_executor"));
        if package != "boon_plan_executor" && executor_dependency.is_some() {
            let optional_instrumentation = package == "boon_phase0_baseline"
                && executor_dependency
                    .and_then(toml::Value::as_table)
                    .and_then(|dependency| dependency.get("optional"))
                    .and_then(toml::Value::as_bool)
                    == Some(true);
            if optional_instrumentation {
                instrumentation_dependents.push(package.to_owned());
            } else {
                direct_executor_dependents.push(package.to_owned());
            }
        }
    }
    direct_executor_dependents.sort();
    instrumentation_dependents.sort();

    let valid = machine_plan_definitions == vec!["crates/boon_plan/src/lib.rs".to_owned()]
        && forbidden_runtime_types.is_empty()
        && machine_owned_by_executor
        && runtime_uses_machine
        && document_uses_machine
        && direct_executor_dependents
            == vec!["boon_document".to_owned(), "boon_runtime".to_owned()]
        && instrumentation_dependents == vec!["boon_phase0_baseline".to_owned()];
    if valid {
        Ok("one MachinePlan definition and one boon_plan_executor MachineTemplate/MachineInstance path; only the document evaluator and runtime consume it directly, plus the optional Phase 0 instrumentation producer".to_owned())
    } else {
        Err(format!(
            "MachinePlan defs={}; MachineInstance defs={}; MachineTemplate defs={}; forbidden runtime executors={}; executor machine={machine_owned_by_executor}; runtime uses machine={runtime_uses_machine}; document uses machine={document_uses_machine}; direct dependents={}; optional instrumentation dependents={}",
            bounded_list(&machine_plan_definitions),
            bounded_list(&machine_instance_definitions),
            bounded_list(&machine_template_definitions),
            bounded_list(&forbidden_runtime_types),
            bounded_list(&direct_executor_dependents),
            bounded_list(&instrumentation_dependents)
        ))
    }
}

fn exact_struct_definition_lines<'a>(
    source: &'a str,
    name: &'a str,
) -> impl Iterator<Item = &'a str> {
    source.lines().filter(move |line| {
        let line = line.trim_start();
        line == format!("struct {name} {{")
            || line == format!("pub struct {name} {{")
            || line.starts_with(&format!("pub(crate) struct {name} {{"))
    })
}

fn verified_semantic_compiler_spine(workspace: &Path) -> Result<String, String> {
    let root = parse_toml(&workspace.join("Cargo.toml"))?;
    let members = root
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "workspace members are missing".to_owned())?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect::<BTreeSet<_>>();
    for required in ["crates/boon_semantic", "crates/boon_verify"] {
        if !members.contains(required) {
            return Err(format!(
                "workspace omits mandatory artifact crate `{required}`"
            ));
        }
    }

    let semantic_manifest = parse_toml(&workspace.join("crates/boon_semantic/Cargo.toml"))?;
    let verify_manifest = parse_toml(&workspace.join("crates/boon_verify/Cargo.toml"))?;
    let ir_manifest = parse_toml(&workspace.join("crates/boon_ir/Cargo.toml"))?;
    let semantic_dependencies = production_dependency_names(&semantic_manifest);
    let verify_dependencies = production_dependency_names(&verify_manifest);
    let ir_dependencies = production_dependency_names(&ir_manifest);
    for forbidden in ["boon_ir", "boon_verify"] {
        if semantic_dependencies.contains(forbidden) {
            return Err(format!(
                "boon_semantic has forbidden production dependency `{forbidden}`"
            ));
        }
    }
    if !semantic_dependencies.contains("boon_typecheck")
        || !semantic_dependencies.contains("boon_contract")
    {
        return Err("boon_semantic must depend on boon_typecheck and boon_contract".to_owned());
    }
    if !verify_dependencies.contains("boon_semantic")
        || !verify_dependencies.contains("boon_contract")
        || verify_dependencies.contains("boon_ir")
    {
        return Err(
            "boon_verify must depend on semantic/contract DTOs and remain below boon_ir".to_owned(),
        );
    }
    if !ir_dependencies.contains("boon_semantic") || !ir_dependencies.contains("boon_verify") {
        return Err("boon_ir must consume both semantic and verified artifacts".to_owned());
    }
    verify_semantic_dependency_allowlist(workspace, &members)?;

    let semantic = read_text(&workspace.join("crates/boon_semantic/src/lib.rs"))?;
    let verify = read_text(&workspace.join("crates/boon_verify/src/lib.rs"))?;
    let ir = read_text(&workspace.join("crates/boon_ir/src/lib.rs"))?;
    let parser = read_text(&workspace.join("crates/boon_parser/src/lib.rs"))?;
    let typecheck = read_text(&workspace.join("crates/boon_typecheck/src/lib.rs"))?;
    let compiler = read_text(&workspace.join("crates/boon_compiler/src/lib.rs"))?;
    let distributed =
        read_text(&workspace.join("crates/boon_compiler/src/distributed_compiler.rs"))?;

    verify_boon_ir_phase1_boundary(workspace)?;
    verify_opaque_artifact(&parser, "ParsedProgram", true)?;
    verify_opaque_artifact(&typecheck, "CheckedProgram", true)?;
    verify_opaque_artifact(&semantic, "SemanticProgram", false)?;
    verify_opaque_artifact(&verify, "ContractVerifiedProgram", false)?;
    verify_opaque_artifact(&ir, "ErasedProgram", true)?;
    verify_required_direct_field(
        &parser,
        "ParsedProgramFields",
        "source_bundle_digest_v1",
        "SourceBundleDigestV1",
    )?;
    verify_required_direct_field(
        &typecheck,
        "CheckedProgramFields",
        "source_bundle_digest_v1",
        "SourceBundleDigestV1",
    )?;
    verify_required_direct_field(
        &semantic,
        "SemanticProgram",
        "source_bundle_digest_v1",
        "SourceBundleDigestV1",
    )?;
    verify_required_direct_field(
        &ir,
        "ErasedProgram",
        "source_bundle_digest_v1",
        "SourceBundleDigestV1",
    )?;
    verify_required_direct_field(
        &ir,
        "ErasedProgram",
        "semantic_program_digest",
        "SemanticProgramDigestV1",
    )?;
    verify_required_direct_field(
        &ir,
        "ErasedProgram",
        "verification_manifest_digest",
        "VerificationManifestDigestV1",
    )?;

    if semantic.contains("boon_ir::") || verify.contains("boon_ir::") {
        return Err(
            "semantic or verifier source reaches upward into executable boon_ir types".to_owned(),
        );
    }
    for required in [
        "pub struct SemanticProgram",
        "pub fn elaborate(",
        "CallableDependencyManifestV1",
    ] {
        if !semantic.contains(required) {
            return Err(format!("boon_semantic omits `{required}`"));
        }
    }
    for required in [
        "pub struct ContractVerifiedProgram",
        "pub fn verify_explicit_contracts(",
        "RequiredObligationManifestV1",
        "VerificationManifestV1",
    ] {
        if !verify.contains(required) {
            return Err(format!("boon_verify omits `{required}`"));
        }
    }
    if !ir.contains("pub fn erase_and_lower(")
        || !ir.contains("verified: boon_verify::ContractVerifiedProgram")
    {
        return Err("boon_ir lacks the sole ContractVerifiedProgram erasure entrypoint".to_owned());
    }
    for forbidden in [
        "pub fn lower(",
        "pub fn lower_runtime(",
        "pub fn lower_with_external_types(",
        "pub fn lower_runtime_with_external_types(",
        "pub fn lower_runtime_with_external_types_and_producer_functions(",
        "pub fn lower_checked(",
    ] {
        if ir.contains(forbidden) {
            return Err(format!("boon_ir still exposes raw lowering `{forbidden}`"));
        }
    }
    verify_exhaustive_boundary_callers(workspace)?;
    for forbidden in ["boon_ir::lower_checked(", "boon_ir::lower_runtime("] {
        if compiler.contains(forbidden) || distributed.contains(forbidden) {
            return Err(format!(
                "compiler retains raw lowering bypass `{forbidden}`"
            ));
        }
    }

    Ok(
        "CheckedProgram -> SemanticProgram -> ContractVerifiedProgram -> opaque ErasedProgram is the only production compiler spine"
            .to_owned(),
    )
}

fn production_dependency_names(manifest: &toml::Value) -> BTreeSet<&str> {
    fn collect<'a>(value: &'a toml::Value, output: &mut BTreeSet<&'a str>) {
        let Some(table) = value.as_table() else {
            return;
        };
        for (key, child) in table {
            if key == "dev-dependencies" || key.ends_with(".dev-dependencies") {
                continue;
            }
            if matches!(key.as_str(), "dependencies" | "build-dependencies") {
                if let Some(dependencies) = child.as_table() {
                    output.extend(dependencies.keys().map(String::as_str));
                }
                continue;
            }
            collect(child, output);
        }
    }

    let mut dependencies = BTreeSet::new();
    collect(manifest, &mut dependencies);
    dependencies
}

fn verify_semantic_dependency_allowlist(
    workspace: &Path,
    members: &BTreeSet<&str>,
) -> Result<(), String> {
    let semantic_consumers = BTreeSet::from(["boon_verify", "boon_ir", "boon_compiler"]);
    let verifier_consumers = BTreeSet::from(["boon_ir", "boon_compiler"]);
    let mut unexpected = Vec::new();
    for member in members {
        let manifest_path = workspace.join(member).join("Cargo.toml");
        if !manifest_path.is_file() {
            return Err(format!(
                "workspace member `{member}` has no readable Cargo.toml"
            ));
        }
        let manifest = parse_toml(&manifest_path)?;
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("workspace member `{member}` has no package name"))?;
        let dependencies = production_dependency_names(&manifest);
        if dependencies.contains("boon_semantic") && !semantic_consumers.contains(package) {
            unexpected.push(format!("{package}->boon_semantic"));
        }
        if dependencies.contains("boon_verify") && !verifier_consumers.contains(package) {
            unexpected.push(format!("{package}->boon_verify"));
        }
    }
    if unexpected.is_empty() {
        Ok(())
    } else {
        unexpected.sort();
        Err(format!(
            "production semantic/verifier dependency allowlist differs: {}",
            bounded_list(&unexpected)
        ))
    }
}

fn verify_opaque_artifact(
    source: &str,
    artifact: &str,
    must_serialize: bool,
) -> Result<(), String> {
    use syn::parse::Parser as _;

    let syntax =
        syn::parse_file(source).map_err(|error| format!("cannot parse `{artifact}`: {error}"))?;
    let definition = syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Struct(definition) if definition.ident == artifact => Some(definition),
            _ => None,
        })
        .ok_or_else(|| format!("source omits opaque artifact `{artifact}`"))?;
    if !matches!(definition.vis, syn::Visibility::Public(_)) {
        return Err(format!("opaque artifact `{artifact}` is not public"));
    }
    if definition
        .fields
        .iter()
        .any(|field| matches!(field.vis, syn::Visibility::Public(_)))
    {
        return Err(format!("opaque artifact `{artifact}` has a public field"));
    }

    let mut derives = BTreeSet::new();
    for attribute in &definition.attrs {
        if !attribute.path().is_ident("derive") {
            continue;
        }
        let syn::Meta::List(list) = &attribute.meta else {
            continue;
        };
        let paths = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|error| format!("cannot parse `{artifact}` derives: {error}"))?;
        derives.extend(paths.iter().filter_map(|path| {
            path.segments
                .last()
                .map(|segment| segment.ident.to_string())
        }));
    }
    if must_serialize && !derives.contains("Serialize") {
        return Err(format!("opaque artifact `{artifact}` is not serializable"));
    }
    for forbidden in ["Deserialize", "Default"] {
        if derives.contains(forbidden) {
            return Err(format!(
                "opaque artifact `{artifact}` derives forgeable `{forbidden}`"
            ));
        }
    }

    for item in &syntax.items {
        let syn::Item::Impl(implementation) = item else {
            continue;
        };
        let syn::Type::Path(self_type) = implementation.self_ty.as_ref() else {
            continue;
        };
        if self_type
            .path
            .segments
            .last()
            .is_none_or(|segment| segment.ident != artifact)
        {
            continue;
        }
        if let Some((_, trait_path, _)) = &implementation.trait_
            && let Some(trait_name) = trait_path.segments.last()
            && matches!(
                trait_name.ident.to_string().as_str(),
                "Deserialize" | "Default" | "DerefMut" | "From" | "TryFrom"
            )
        {
            return Err(format!(
                "opaque artifact `{artifact}` implements forgeable trait `{}`",
                trait_name.ident
            ));
        }
        if implementation.trait_.is_none() {
            for implementation_item in &implementation.items {
                let syn::ImplItem::Fn(function) = implementation_item else {
                    continue;
                };
                if matches!(function.vis, syn::Visibility::Public(_))
                    && function.sig.receiver().is_none()
                {
                    return Err(format!(
                        "opaque artifact `{artifact}` exposes public associated constructor `{}`",
                        function.sig.ident
                    ));
                }
            }
        }
    }
    Ok(())
}

fn verify_required_direct_field(
    source: &str,
    structure: &str,
    field: &str,
    expected_type: &str,
) -> Result<(), String> {
    let syntax =
        syn::parse_file(source).map_err(|error| format!("cannot parse `{structure}`: {error}"))?;
    let definition = syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Struct(definition) if definition.ident == structure => Some(definition),
            _ => None,
        })
        .ok_or_else(|| format!("source omits `{structure}`"))?;
    let field = definition
        .fields
        .iter()
        .find(|candidate| {
            candidate
                .ident
                .as_ref()
                .is_some_and(|identifier| identifier == field)
        })
        .ok_or_else(|| format!("`{structure}` omits mandatory field `{field}`"))?;
    let syn::Type::Path(field_type) = &field.ty else {
        return Err(format!(
            "`{structure}.{}` is not the required direct `{expected_type}`",
            field.ident.as_ref().expect("named field")
        ));
    };
    if field_type.qself.is_some()
        || field_type
            .path
            .segments
            .last()
            .is_none_or(|segment| segment.ident != expected_type || !segment.arguments.is_empty())
    {
        return Err(format!(
            "`{structure}.{}` must be a nonoptional direct `{expected_type}`",
            field.ident.as_ref().expect("named field")
        ));
    }
    Ok(())
}

const BOON_IR_LIB: &str = "crates/boon_ir/src/lib.rs";
const BOON_IR_SEMANTIC_MAPPING: &str = "crates/boon_ir/src/semantic_mapping.rs";

const FORBIDDEN_IR_ARTIFACT_IDENTIFIERS: [&str; 3] =
    ["CheckedProgram", "CheckedProgramFields", "ResolvedOutGraph"];

const FORBIDDEN_IR_DISCOVERY_IDENTIFIERS: [(&str, &str); 24] = [
    (
        "legacy_checked_semantic_lowering_pending_extraction",
        "legacy semantic lowering",
    ),
    ("derive_contextual_materializations", "contextual discovery"),
    ("derive_executable_program", "contextual discovery"),
    ("contextual_materializations", "contextual discovery"),
    (
        "lower_semantic_memory_and_migrations",
        "migration discovery",
    ),
    (
        "classify_transitive_row_resource_fields",
        "resource discovery",
    ),
    (
        "canonicalize_runtime_resource_metadata",
        "resource discovery",
    ),
    ("bind_executable_source_resources", "resource discovery"),
    ("bind_executable_state_resources", "resource discovery"),
    (
        "collect_detached_state_capture_requests",
        "reactive discovery",
    ),
    ("materialization_target_lists", "reactive discovery"),
    (
        "bind_contextual_materialization_targets",
        "reactive discovery",
    ),
    (
        "bind_contextual_materialization_storage",
        "reactive discovery",
    ),
    (
        "bind_contextual_materialization_lineage",
        "reactive discovery",
    ),
    ("collect_input_materializations", "reactive discovery"),
    (
        "collect_input_materializations_at_projection",
        "reactive discovery",
    ),
    ("exact_list_mutations", "reactive discovery"),
    ("collect_exact_list_mutations", "reactive discovery"),
    ("exact_dependency_edges", "reactive discovery"),
    ("exact_possible_causes", "reactive discovery"),
    ("state_update_arms", "reactive discovery"),
    ("distributed_references", "distributed discovery"),
    ("concrete_distributed_calls", "distributed discovery"),
    (
        "bind_distributed_reference_aliases",
        "distributed discovery",
    ),
];

fn verify_boon_ir_phase1_boundary(workspace: &Path) -> Result<(), String> {
    let files = boon_ir_rust_file_inventory(workspace)?;
    verify_boon_ir_file_inventory(&files)?;

    let semantic_execution_types = semantic_execution_type_inventory(workspace)?;
    let mut violations = Vec::new();
    let mut mapping_delegation_functions = BTreeSet::new();
    let mut resource_mapping_delegation_functions = BTreeSet::new();
    let mut graph_handoff_functions = BTreeSet::new();
    let mut totality_validation_functions = BTreeSet::new();

    for relative in files.iter().filter(|relative| !is_test_path(relative)) {
        let source = read_text(&workspace.join(relative))?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            format!("cannot parse production boon_ir source `{relative}`: {error}")
        })?;
        if relative == BOON_IR_LIB {
            verify_semantic_mapping_module_declaration(&syntax)?;
        }
        if relative == BOON_IR_SEMANTIC_MAPPING {
            verify_semantic_mapping_contract(&source)?;
        }

        let mut collector = IrPhase1BoundaryCollector::new(
            relative == BOON_IR_SEMANTIC_MAPPING,
            &semantic_execution_types,
        );
        collector.visit_file(&syntax);
        violations.extend(
            collector
                .violations
                .into_iter()
                .map(|violation| format!("{relative}:{violation}")),
        );
        mapping_delegation_functions.extend(
            collector
                .mapping_delegation_functions
                .into_iter()
                .map(|function| format!("{relative}:{function}")),
        );
        resource_mapping_delegation_functions.extend(
            collector
                .resource_mapping_delegation_functions
                .into_iter()
                .map(|function| format!("{relative}:{function}")),
        );
        graph_handoff_functions.extend(
            collector
                .graph_handoff_functions
                .into_iter()
                .map(|function| format!("{relative}:{function}")),
        );
        totality_validation_functions.extend(
            collector
                .totality_validation_functions
                .into_iter()
                .map(|function| format!("{relative}:{function}")),
        );
    }

    violations.sort();
    violations.dedup();
    if !violations.is_empty() {
        return Err(format!(
            "boon_ir retains forbidden Phase-1 semantic authority: {}",
            bounded_list(&violations)
        ));
    }

    let expected_handoff =
        BTreeSet::from([format!("{BOON_IR_LIB}:lower_verified_semantic_execution")]);
    if graph_handoff_functions != expected_handoff {
        return Err(format!(
            "SemanticExecutionGraphV1 handoff inventory differs; expected: {}; observed: {}",
            bounded_list(&expected_handoff.iter().cloned().collect::<Vec<_>>()),
            bounded_list(&graph_handoff_functions.iter().cloned().collect::<Vec<_>>())
        ));
    }
    if mapping_delegation_functions != expected_handoff {
        return Err(format!(
            "semantic-to-executable conversion must delegate exactly once from the verified handoff to `semantic_mapping`; expected: {}; observed: {}",
            bounded_list(&expected_handoff.iter().cloned().collect::<Vec<_>>()),
            bounded_list(
                &mapping_delegation_functions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            )
        ));
    }
    if resource_mapping_delegation_functions != expected_handoff {
        return Err(format!(
            "semantic resource conversion must delegate exactly once from the verified handoff to `semantic_mapping`; expected: {}; observed: {}",
            bounded_list(&expected_handoff.iter().cloned().collect::<Vec<_>>()),
            bounded_list(
                &resource_mapping_delegation_functions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            )
        ));
    }
    if totality_validation_functions != expected_handoff {
        return Err(format!(
            "the verified semantic handoff must validate mapping totality exactly once; expected: {}; observed: {}",
            bounded_list(&expected_handoff.iter().cloned().collect::<Vec<_>>()),
            bounded_list(
                &totality_validation_functions
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            )
        ));
    }
    Ok(())
}

fn boon_ir_rust_file_inventory(workspace: &Path) -> Result<BTreeSet<String>, String> {
    fn collect(
        workspace: &Path,
        directory: &Path,
        files: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        let entries = fs::read_dir(directory)
            .map_err(|error| format!("cannot inventory `{}`: {error}", directory.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!(
                    "cannot inventory entry below `{}`: {error}",
                    directory.display()
                )
            })?;
            let file_type = entry
                .file_type()
                .map_err(|error| format!("cannot inspect `{}`: {error}", entry.path().display()))?;
            let path = entry.path();
            if file_type.is_dir() {
                collect(workspace, &path, files)?;
            } else if file_type.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
            {
                let relative = path.strip_prefix(workspace).map_err(|error| {
                    format!(
                        "cannot make `{}` workspace-relative: {error}",
                        path.display()
                    )
                })?;
                files.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }

    let mut files = BTreeSet::new();
    collect(workspace, &workspace.join("crates/boon_ir/src"), &mut files)?;
    Ok(files)
}

fn verify_boon_ir_file_inventory(files: &BTreeSet<String>) -> Result<(), String> {
    for required in [BOON_IR_LIB, BOON_IR_SEMANTIC_MAPPING] {
        if !files.contains(required) {
            return Err(format!(
                "boon_ir production source inventory omits `{required}`"
            ));
        }
    }

    let old_module_files = files
        .iter()
        .filter(|relative| {
            [
                "crates/boon_ir/src/contextual_expansion.rs",
                "crates/boon_ir/src/semantic_migration.rs",
            ]
            .contains(&relative.as_str())
                || [
                    "crates/boon_ir/src/contextual_expansion/",
                    "crates/boon_ir/src/semantic_migration/",
                ]
                .iter()
                .any(|prefix| relative.starts_with(prefix))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !old_module_files.is_empty() {
        return Err(format!(
            "old production contextual/migration module files remain: {}",
            bounded_list(&old_module_files)
        ));
    }

    let mapping_files = files
        .iter()
        .filter(|relative| {
            relative.as_str() == BOON_IR_SEMANTIC_MAPPING
                || relative.starts_with("crates/boon_ir/src/semantic_mapping/")
        })
        .cloned()
        .collect::<Vec<_>>();
    if mapping_files != [BOON_IR_SEMANTIC_MAPPING.to_owned()] {
        return Err(format!(
            "semantic-to-executable conversion must occupy exactly `{BOON_IR_SEMANTIC_MAPPING}`; observed: {}",
            bounded_list(&mapping_files)
        ));
    }
    Ok(())
}

fn verify_semantic_mapping_module_declaration(syntax: &syn::File) -> Result<(), String> {
    let declarations = syntax
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Mod(module) = item else {
                return None;
            };
            if cfg_is_test_only(&module.attrs) || module.ident != "semantic_mapping" {
                return None;
            }
            Some(module)
        })
        .collect::<Vec<_>>();
    let [declaration] = declarations.as_slice() else {
        return Err(format!(
            "boon_ir must declare exactly one production `mod semantic_mapping;`; observed {}",
            declarations.len()
        ));
    };
    if declaration.content.is_some()
        || declaration
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("path"))
    {
        return Err(
            "boon_ir `semantic_mapping` must be the exact external `src/semantic_mapping.rs` module"
                .to_owned(),
        );
    }
    Ok(())
}

fn semantic_execution_type_inventory(workspace: &Path) -> Result<BTreeSet<String>, String> {
    let path = workspace.join("crates/boon_semantic/src/execution.rs");
    let source = read_text(&path)?;
    let syntax = syn::parse_file(&source)
        .map_err(|error| format!("cannot parse `{}`: {error}", path.display()))?;
    let mut types = BTreeSet::new();
    for item in &syntax.items {
        if item_attributes(item).is_some_and(cfg_is_test_only) {
            continue;
        }
        let identifier = match item {
            syn::Item::Enum(item) => Some(&item.ident),
            syn::Item::Struct(item) => Some(&item.ident),
            syn::Item::Type(item) => Some(&item.ident),
            syn::Item::Union(item) => Some(&item.ident),
            syn::Item::Macro(item)
                if item
                    .mac
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "typed_semantic_id") =>
            {
                for identifier in rust_token_identifiers(&item.mac.tokens.to_string()) {
                    if identifier.starts_with("Semantic") {
                        types.insert(identifier);
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(identifier) = identifier
            && identifier.to_string().starts_with("Semantic")
        {
            types.insert(identifier.to_string());
        }
    }
    if !types.contains("SemanticExecutionGraphV1")
        || !types.contains("SemanticExprId")
        || !types.contains("SemanticExpression")
    {
        return Err(format!(
            "semantic execution DTO inventory is incomplete: {}",
            bounded_list(&types.iter().cloned().collect::<Vec<_>>())
        ));
    }
    Ok(types)
}

struct IrPhase1BoundaryCollector<'a> {
    in_semantic_mapping: bool,
    semantic_execution_types: &'a BTreeSet<String>,
    current_function: Option<String>,
    violations: Vec<String>,
    mapping_delegation_functions: BTreeSet<String>,
    resource_mapping_delegation_functions: BTreeSet<String>,
    graph_handoff_functions: BTreeSet<String>,
    totality_validation_functions: BTreeSet<String>,
}

impl<'a> IrPhase1BoundaryCollector<'a> {
    fn new(in_semantic_mapping: bool, semantic_execution_types: &'a BTreeSet<String>) -> Self {
        Self {
            in_semantic_mapping,
            semantic_execution_types,
            current_function: None,
            violations: Vec::new(),
            mapping_delegation_functions: BTreeSet::new(),
            resource_mapping_delegation_functions: BTreeSet::new(),
            graph_handoff_functions: BTreeSet::new(),
            totality_validation_functions: BTreeSet::new(),
        }
    }

    fn function(&self) -> String {
        self.current_function
            .clone()
            .unwrap_or_else(|| "<module>".to_owned())
    }

    fn inspect_identifier(&mut self, identifier: &str, kind: &str, macro_tokens: bool) {
        if FORBIDDEN_IR_ARTIFACT_IDENTIFIERS.contains(&identifier) {
            self.violations
                .push(format!("{kind} references forbidden `{identifier}`"));
        }
        if matches!(
            identifier,
            "contextual_expansion" | "semantic_migration" | "lower_semantic_unverified" | "out_net"
        ) {
            self.violations
                .push(format!("{kind} references forbidden `{identifier}`"));
        }
        if let Some((_, category)) = FORBIDDEN_IR_DISCOVERY_IDENTIFIERS
            .iter()
            .find(|(forbidden, _)| *forbidden == identifier)
        {
            self.violations.push(format!(
                "{kind} retains old {category} helper `{identifier}`"
            ));
        }
        if !self.in_semantic_mapping && self.semantic_execution_types.contains(identifier) {
            let verified_graph_handoff = !macro_tokens
                && identifier == "SemanticExecutionGraphV1"
                && self.current_function.as_deref() == Some("lower_verified_semantic_execution");
            if verified_graph_handoff {
                self.graph_handoff_functions.insert(self.function());
            } else {
                self.violations.push(format!(
                    "{kind} references semantic execution DTO `{identifier}` outside `semantic_mapping`"
                ));
            }
        }
    }

    fn inspect_path(&mut self, path: &syn::Path, kind: &str) {
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        for identifier in &segments {
            self.inspect_identifier(identifier, kind, false);
        }
        if !self.in_semantic_mapping
            && segments
                .last()
                .is_some_and(|segment| segment == "map_semantic_execution_with_reactive")
            && segments.iter().any(|segment| segment == "semantic_mapping")
        {
            self.mapping_delegation_functions.insert(self.function());
        }
        if !self.in_semantic_mapping
            && segments
                .last()
                .is_some_and(|segment| segment == "map_semantic_resources")
            && segments.iter().any(|segment| segment == "semantic_mapping")
        {
            self.resource_mapping_delegation_functions
                .insert(self.function());
        }
    }

    fn inspect_macro_tokens(&mut self, tokens: &str) {
        for identifier in rust_token_identifiers(tokens) {
            self.inspect_identifier(&identifier, "macro tokens", true);
            if !self.in_semantic_mapping
                && matches!(
                    identifier.as_str(),
                    "map_semantic_execution" | "map_semantic_execution_with_reactive"
                )
            {
                self.violations.push(
                    "macro tokens hide semantic-to-executable conversion outside `semantic_mapping`"
                        .to_owned(),
                );
            }
            if !self.in_semantic_mapping && identifier == "map_semantic_resources" {
                self.violations.push(
                    "macro tokens hide semantic resource conversion outside `semantic_mapping`"
                        .to_owned(),
                );
            }
        }
    }

    fn enter_function(&mut self, identifier: &syn::Ident) -> Option<String> {
        let previous = self.current_function.replace(identifier.to_string());
        self.inspect_identifier(&identifier.to_string(), "function", false);
        previous
    }
}

impl<'ast> Visit<'ast> for IrPhase1BoundaryCollector<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if impl_item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if trait_item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_trait_item(self, item);
    }

    fn visit_field(&mut self, field: &'ast syn::Field) {
        if cfg_is_test_only(&field.attrs) {
            return;
        }
        syn::visit::visit_field(self, field);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if cfg_is_test_only(&local.attrs) {
            return;
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.inspect_identifier(&item.ident.to_string(), "module", false);
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let previous = self.enter_function(&item.sig.ident);
        syn::visit::visit_item_fn(self, item);
        self.current_function = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = self.enter_function(&item.sig.ident);
        syn::visit::visit_impl_item_fn(self, item);
        self.current_function = previous;
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        let previous = self.enter_function(&item.sig.ident);
        syn::visit::visit_trait_item_fn(self, item);
        self.current_function = previous;
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut imports = Vec::new();
        flatten_use_tree(&item.tree, &mut Vec::new(), &mut imports);
        for import in imports {
            for identifier in import.path {
                self.inspect_identifier(&identifier, "import", false);
            }
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.inspect_path(path, "path");
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        let method = expression.method.to_string();
        self.inspect_identifier(&method, "method call", false);
        if !self.in_semantic_mapping && method == "validate_totality" {
            self.totality_validation_functions.insert(self.function());
        }
        syn::visit::visit_expr_method_call(self, expression);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.inspect_macro_tokens(&item.tokens.to_string());
        syn::visit::visit_macro(self, item);
    }
}

#[derive(Clone, Debug)]
struct FlattenedImport {
    path: Vec<String>,
}

fn flatten_use_tree(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    output: &mut Vec<FlattenedImport>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            flatten_use_tree(&path.tree, prefix, output);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            output.push(FlattenedImport {
                path: prefix.clone(),
            });
            prefix.pop();
        }
        syn::UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            output.push(FlattenedImport {
                path: prefix.clone(),
            });
            prefix.pop();
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                flatten_use_tree(tree, prefix, output);
            }
        }
        syn::UseTree::Glob(_) => output.push(FlattenedImport {
            path: prefix.clone(),
        }),
    }
}

fn verify_semantic_mapping_contract(source: &str) -> Result<(), String> {
    let syntax = syn::parse_file(source)
        .map_err(|error| format!("cannot parse semantic mapping: {error}"))?;
    let definitions = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(definition)
                if definition.ident == "SemanticToExecutableMap"
                    && !cfg_is_test_only(&definition.attrs) =>
            {
                Some(definition)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return Err(format!(
            "`semantic_mapping` must define exactly one production `SemanticToExecutableMap`; observed {}",
            definitions.len()
        ));
    };
    let syn::Fields::Named(fields) = &definition.fields else {
        return Err("`SemanticToExecutableMap` must use explicit named map fields".to_owned());
    };
    let mut map_fields = BTreeSet::new();
    for field in &fields.named {
        let Some(identifier) = &field.ident else {
            return Err("`SemanticToExecutableMap` contains an unnamed field".to_owned());
        };
        if !matches!(field.vis, syn::Visibility::Inherited) {
            return Err(format!(
                "`SemanticToExecutableMap.{identifier}` must remain private"
            ));
        }
        if !explicit_map_storage_type(&field.ty) {
            return Err(format!(
                "`SemanticToExecutableMap.{identifier}` is not an explicit Vec/BTreeMap allocation table"
            ));
        }
        map_fields.insert(identifier.to_string());
    }

    let required = [
        ("expressions", "expressions", "expression"),
        ("statements", "statements", "statement"),
        ("sources", "sources", "source"),
        ("states", "states", "state"),
        ("callables", "functions", "callable"),
        ("materializations", "materializations", "materialization"),
        ("lists", "lists", "list"),
        ("row_scopes", "row_scopes", "row_scope"),
        (
            "value_list_authorities",
            "value_list_authorities",
            "value_list_authority",
        ),
    ];
    let missing_fields = required
        .iter()
        .filter(|(field, _, _)| !map_fields.contains(*field))
        .map(|(field, _, _)| (*field).to_owned())
        .collect::<Vec<_>>();
    if !missing_fields.is_empty() {
        return Err(format!(
            "`SemanticToExecutableMap` omits required identity domains: {}",
            bounded_list(&missing_fields)
        ));
    }

    let mut collector = SemanticMappingContractCollector::default();
    collector.visit_file(&syntax);
    let missing_initializers = map_fields
        .difference(&collector.allocated_fields)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_initializers.is_empty() {
        return Err(format!(
            "`SemanticToExecutableMap::allocate_with_external_events` does not initialize every explicit map: {}",
            bounded_list(&missing_initializers)
        ));
    }
    let missing_totality = map_fields
        .difference(&collector.totality_fields)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_totality.is_empty() {
        return Err(format!(
            "`validate_totality` does not compare every explicit map with emitted output: {}",
            bounded_list(&missing_totality)
        ));
    }
    let missing_dense_domains = required
        .iter()
        .filter(|(_, graph_field, _)| !collector.dense_graph_domains.contains(*graph_field))
        .map(|(_, graph_field, _)| (*graph_field).to_owned())
        .collect::<Vec<_>>();
    if !missing_dense_domains.is_empty() {
        return Err(format!(
            "`SemanticToExecutableMap::allocate_with_external_events` does not validate dense source identities for: {}",
            bounded_list(&missing_dense_domains)
        ));
    }
    let missing_lookup_methods = required
        .iter()
        .filter(|(_, _, method)| !collector.map_methods.contains(*method))
        .map(|(_, _, method)| (*method).to_owned())
        .collect::<Vec<_>>();
    if !missing_lookup_methods.is_empty() {
        return Err(format!(
            "`SemanticToExecutableMap` omits explicit lookup methods: {}",
            bounded_list(&missing_lookup_methods)
        ));
    }
    if !collector.has_totality_validator {
        return Err("semantic mapping omits production `validate_totality`".to_owned());
    }
    if !collector.entrypoint_allocates_map {
        return Err(
            "`map_semantic_execution_with_external_events` does not allocate `SemanticToExecutableMap`"
                .to_owned(),
        );
    }
    if !(collector.allocate_uses_unique_set && collector.allocate_compares_unique_lengths) {
        return Err(
            "`SemanticToExecutableMap::allocate_with_external_events` lacks explicit one-to-one/bijection validation"
                .to_owned(),
        );
    }
    Ok(())
}

fn explicit_map_storage_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.last().is_some_and(|segment| {
            matches!(segment.ident.to_string().as_str(), "Vec" | "BTreeMap")
                && matches!(
                    segment.arguments,
                    syn::PathArguments::AngleBracketed(ref arguments)
                        if !arguments.args.is_empty()
                )
        })
}

#[derive(Default)]
struct SemanticMappingContractCollector {
    current_impl: Option<String>,
    current_function: Option<String>,
    require_dense_depth: usize,
    allocated_fields: BTreeSet<String>,
    totality_fields: BTreeSet<String>,
    dense_graph_domains: BTreeSet<String>,
    map_methods: BTreeSet<String>,
    has_totality_validator: bool,
    entrypoint_allocates_map: bool,
    allocate_uses_unique_set: bool,
    allocate_compares_unique_lengths: bool,
}

impl SemanticMappingContractCollector {
    fn in_map_allocate(&self) -> bool {
        self.current_impl.as_deref() == Some("SemanticToExecutableMap")
            && self.current_function.as_deref() == Some("allocate_with_external_events")
    }

    fn in_totality_validator(&self) -> bool {
        self.current_function.as_deref() == Some("validate_totality")
    }

    fn enter_function(&mut self, identifier: &syn::Ident) -> Option<String> {
        self.current_function.replace(identifier.to_string())
    }
}

impl<'ast> Visit<'ast> for SemanticMappingContractCollector {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if impl_item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_impl_item(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.current_impl.clone();
        self.current_impl = simple_type_name(item.self_ty.as_ref());
        syn::visit::visit_item_impl(self, item);
        self.current_impl = previous;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let previous = self.enter_function(&item.sig.ident);
        syn::visit::visit_item_fn(self, item);
        self.current_function = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = self.enter_function(&item.sig.ident);
        if self.current_impl.as_deref() == Some("SemanticToExecutableMap") {
            self.map_methods.insert(item.sig.ident.to_string());
        }
        if item.sig.ident == "validate_totality" {
            self.has_totality_validator = true;
        }
        syn::visit::visit_impl_item_fn(self, item);
        self.current_function = previous;
    }

    fn visit_expr_struct(&mut self, expression: &'ast syn::ExprStruct) {
        if self.in_map_allocate()
            && expression.path.segments.last().is_some_and(|segment| {
                matches!(
                    segment.ident.to_string().as_str(),
                    "Self" | "SemanticToExecutableMap"
                )
            })
        {
            self.allocated_fields
                .extend(
                    expression
                        .fields
                        .iter()
                        .filter_map(|field| match &field.member {
                            syn::Member::Named(identifier) => Some(identifier.to_string()),
                            syn::Member::Unnamed(_) => None,
                        }),
                );
        }
        syn::visit::visit_expr_struct(self, expression);
    }

    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        let is_require_dense = self.in_map_allocate()
            && matches!(
                expression.func.as_ref(),
                syn::Expr::Path(path)
                    if path.path.segments.last().is_some_and(|segment| segment.ident == "require_dense")
            );
        if is_require_dense {
            self.require_dense_depth += 1;
        }
        if self.current_impl.is_none()
            && self.current_function.as_deref()
                == Some("map_semantic_execution_with_external_events")
            && matches!(
                expression.func.as_ref(),
                syn::Expr::Path(path)
                    if path.path.segments.last().is_some_and(|segment| segment.ident == "allocate_with_external_events")
                        && path.path.segments.iter().any(|segment| segment.ident == "SemanticToExecutableMap")
            )
        {
            self.entrypoint_allocates_map = true;
        }
        syn::visit::visit_expr_call(self, expression);
        if is_require_dense {
            self.require_dense_depth -= 1;
        }
    }

    fn visit_expr_field(&mut self, expression: &'ast syn::ExprField) {
        if let Some(chain) = expression_field_chain(expression)
            && self.require_dense_depth > 0
            && chain
                .first()
                .is_some_and(|root| root == "graph" || root == "resources")
            && chain.len() >= 2
        {
            self.dense_graph_domains.insert(chain[1].clone());
        }
        syn::visit::visit_expr_field(self, expression);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        if self.in_totality_validator()
            && expression.method == "len"
            && let Some(chain) = expression_chain(expression.receiver.as_ref())
            && chain.len() == 3
            && chain[0] == "self"
            && chain[1] == "id_map"
        {
            self.totality_fields.insert(chain[2].clone());
        }
        syn::visit::visit_expr_method_call(self, expression);
    }

    fn visit_expr_binary(&mut self, expression: &'ast syn::ExprBinary) {
        if self.in_map_allocate()
            && matches!(expression.op, syn::BinOp::Ne(_))
            && expression_is_len_call(expression.left.as_ref())
            && expression_is_len_call(expression.right.as_ref())
        {
            self.allocate_compares_unique_lengths = true;
        }
        syn::visit::visit_expr_binary(self, expression);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.in_map_allocate()
            && path
                .segments
                .iter()
                .any(|segment| segment.ident == "BTreeSet")
        {
            self.allocate_uses_unique_set = true;
        }
        syn::visit::visit_path(self, path);
    }
}

fn simple_type_name(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn expression_field_chain(expression: &syn::ExprField) -> Option<Vec<String>> {
    let mut chain = expression_chain(expression.base.as_ref())?;
    let syn::Member::Named(identifier) = &expression.member else {
        return None;
    };
    chain.push(identifier.to_string());
    Some(chain)
}

fn expression_chain(expression: &syn::Expr) -> Option<Vec<String>> {
    match expression {
        syn::Expr::Path(path) if path.qself.is_none() => Some(
            path.path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
        ),
        syn::Expr::Field(field) => expression_field_chain(field),
        syn::Expr::Paren(paren) => expression_chain(paren.expr.as_ref()),
        _ => None,
    }
}

fn expression_is_len_call(expression: &syn::Expr) -> bool {
    matches!(
        expression,
        syn::Expr::MethodCall(call) if call.method == "len" && call.args.is_empty()
    )
}

fn rust_token_identifiers(tokens: &str) -> Vec<String> {
    let bytes = tokens.as_bytes();
    let mut identifiers = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'"' {
            index = skip_quoted_token(bytes, index + 1, b'"');
            continue;
        }
        if byte == b'\'' && index + 2 < bytes.len() && bytes[index + 2] == b'\'' {
            index += 3;
            continue;
        }
        if byte == b'r' {
            let mut cursor = index + 1;
            while cursor < bytes.len() && bytes[cursor] == b'#' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'"' {
                let hashes = cursor - index - 1;
                cursor += 1;
                while cursor < bytes.len() {
                    if bytes[cursor] == b'"'
                        && bytes
                            .get(cursor + 1..cursor + 1 + hashes)
                            .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
                    {
                        index = cursor + 1 + hashes;
                        break;
                    }
                    cursor += 1;
                }
                if cursor >= bytes.len() {
                    index = bytes.len();
                }
                continue;
            }
        }
        if byte == b'_' || byte.is_ascii_alphabetic() {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            identifiers.push(tokens[start..index].to_owned());
            continue;
        }
        index += 1;
    }
    identifiers
}

fn skip_quoted_token(bytes: &[u8], mut index: usize, quote: u8) -> usize {
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
        } else if bytes[index] == quote {
            return index + 1;
        } else {
            index += 1;
        }
    }
    bytes.len()
}

const VERIFIED_BOUNDARY_FUNCTIONS: [&str; 7] = [
    "elaborate",
    "elaborate_with_external_event_identities",
    "verify_explicit_contracts",
    "erase_and_lower",
    "verify_bundle",
    "erase_and_lower_bundle",
    "into_lowering_parts",
];

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BoundaryReference {
    file: String,
    path: String,
    kind: &'static str,
}

#[derive(Default)]
struct BoundaryReferenceCollector {
    references: Vec<(String, &'static str)>,
}

impl<'ast> Visit<'ast> for BoundaryReferenceCollector {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if impl_item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if trait_item_attributes(item).is_some_and(cfg_is_test_only) {
            return;
        }
        syn::visit::visit_trait_item(self, item);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        let path = rust_path_text(&expression.path);
        if expression
            .path
            .segments
            .last()
            .is_some_and(|segment| is_verified_boundary_function(&segment.ident.to_string()))
        {
            self.references.push((path, "expression"));
        }
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast syn::ExprMethodCall) {
        if is_verified_boundary_function(&expression.method.to_string()) {
            self.references
                .push((expression.method.to_string(), "method"));
        }
        syn::visit::visit_expr_method_call(self, expression);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut imports = Vec::new();
        imported_boundary_functions(&item.tree, &mut Vec::new(), &mut imports);
        self.references
            .extend(imports.into_iter().map(|path| (path, "import")));
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        let crate_name = item.ident.to_string();
        if matches!(
            crate_name.as_str(),
            "boon_semantic" | "boon_verify" | "boon_ir"
        ) {
            self.references
                .push((format!("extern crate {crate_name}"), "extern-crate"));
        }
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        let tokens = item.tokens.to_string();
        for function in VERIFIED_BOUNDARY_FUNCTIONS {
            if rust_token_text_contains_ident(&tokens, function) {
                self.references
                    .push((format!("macro tokens containing `{function}`"), "macro"));
            }
        }
        syn::visit::visit_macro(self, item);
    }
}

fn verify_exhaustive_boundary_callers(workspace: &Path) -> Result<(), String> {
    let mut observed = Vec::new();
    for file in workspace_files(workspace)?
        .into_iter()
        .filter(|path| path.starts_with("crates/") && path.ends_with(".rs"))
    {
        if is_test_path(&file) {
            continue;
        }
        let path = workspace.join(&file);
        if !path.is_file() {
            continue;
        }
        let source = read_text(&path)?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("cannot parse Rust source `{file}`: {error}"))?;
        let mut collector = BoundaryReferenceCollector::default();
        collector.visit_file(&syntax);
        observed.extend(
            collector
                .references
                .into_iter()
                .map(|(path, kind)| BoundaryReference {
                    file: file.clone(),
                    path,
                    kind,
                }),
        );
    }
    observed.sort();

    let expected = [
        BoundaryReference {
            file: "crates/boon_compiler/src/lib.rs".to_owned(),
            path: "boon_semantic::elaborate_with_external_event_identities".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_semantic/src/lib.rs".to_owned(),
            path: "elaborate_with_external_event_identities".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_compiler/src/lib.rs".to_owned(),
            path: "boon_verify::verify_explicit_contracts".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_compiler/src/lib.rs".to_owned(),
            path: "boon_ir::erase_and_lower".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_compiler/src/distributed_compiler.rs".to_owned(),
            path: "boon_verify::verify_bundle".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_compiler/src/distributed_compiler.rs".to_owned(),
            path: "boon_ir::erase_and_lower_bundle".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_ir/src/lib.rs".to_owned(),
            path: "into_lowering_parts".to_owned(),
            kind: "method",
        },
        BoundaryReference {
            file: "crates/boon_ir/src/lib.rs".to_owned(),
            path: "into_lowering_parts".to_owned(),
            kind: "method",
        },
        BoundaryReference {
            file: "crates/boon_ir/src/lib.rs".to_owned(),
            path: "into_lowering_parts".to_owned(),
            kind: "method",
        },
    ];
    let mut expected = expected.into_iter().collect::<Vec<_>>();
    expected.sort();
    if observed == expected {
        return Ok(());
    }

    let missing = expected
        .iter()
        .filter(|reference| !observed.contains(reference))
        .map(boundary_reference_text)
        .collect::<Vec<_>>();
    let unexpected = observed
        .iter()
        .filter(|reference| !expected.contains(reference))
        .map(boundary_reference_text)
        .collect::<Vec<_>>();
    Err(format!(
        "verified boundary caller inventory differs; missing: {}; unexpected: {}",
        bounded_list(&missing),
        bounded_list(&unexpected)
    ))
}

fn boundary_reference_text(reference: &BoundaryReference) -> String {
    format!("{}:{}:{}", reference.file, reference.kind, reference.path)
}

fn is_verified_boundary_function(identifier: &str) -> bool {
    VERIFIED_BOUNDARY_FUNCTIONS.contains(&identifier)
}

fn rust_path_text(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn imported_boundary_functions(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    output: &mut Vec<String>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            imported_boundary_functions(&path.tree, prefix, output);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            if is_verified_boundary_function(prefix.last().map(String::as_str).unwrap_or_default())
            {
                output.push(prefix.join("::"));
            }
            prefix.pop();
        }
        syn::UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            if is_verified_boundary_function(prefix.last().map(String::as_str).unwrap_or_default())
            {
                output.push(format!("{} as {}", prefix.join("::"), rename.rename));
            }
            prefix.pop();
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                imported_boundary_functions(item, prefix, output);
            }
        }
        syn::UseTree::Glob(_) => {
            if prefix.first().is_some_and(|root| {
                matches!(root.as_str(), "boon_semantic" | "boon_verify" | "boon_ir")
            }) {
                output.push(format!("{}::*", prefix.join("::")));
            }
        }
    }
}

fn rust_token_text_contains_ident(tokens: &str, identifier: &str) -> bool {
    tokens
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|token| token == identifier)
}

fn cfg_is_test_only(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let syn::Meta::List(list) = &attribute.meta else {
            return false;
        };
        syn::parse2::<syn::Meta>(list.tokens.clone())
            .ok()
            .is_some_and(|predicate| !cfg_truth_with_test_false(&predicate).can_be_true)
    })
}

#[derive(Clone, Copy)]
struct PossibleTruth {
    can_be_false: bool,
    can_be_true: bool,
}

fn cfg_truth_with_test_false(predicate: &syn::Meta) -> PossibleTruth {
    match predicate {
        syn::Meta::Path(path) if path.is_ident("test") => PossibleTruth {
            can_be_false: true,
            can_be_true: false,
        },
        syn::Meta::Path(_) | syn::Meta::NameValue(_) => PossibleTruth {
            can_be_false: true,
            can_be_true: true,
        },
        syn::Meta::List(list) => {
            use syn::parse::Parser as _;
            let parsed = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                .parse2(list.tokens.clone());
            let Ok(children) = parsed else {
                return PossibleTruth {
                    can_be_false: true,
                    can_be_true: true,
                };
            };
            let children = children
                .iter()
                .map(cfg_truth_with_test_false)
                .collect::<Vec<_>>();
            if list.path.is_ident("all") {
                PossibleTruth {
                    can_be_false: children.iter().any(|child| child.can_be_false),
                    can_be_true: children.iter().all(|child| child.can_be_true),
                }
            } else if list.path.is_ident("any") {
                PossibleTruth {
                    can_be_false: children.iter().all(|child| child.can_be_false),
                    can_be_true: children.iter().any(|child| child.can_be_true),
                }
            } else if list.path.is_ident("not") && children.len() == 1 {
                PossibleTruth {
                    can_be_false: children[0].can_be_true,
                    can_be_true: children[0].can_be_false,
                }
            } else {
                PossibleTruth {
                    can_be_false: true,
                    can_be_true: true,
                }
            }
        }
    }
}

fn item_attributes(item: &syn::Item) -> Option<&[syn::Attribute]> {
    match item {
        syn::Item::Const(item) => Some(&item.attrs),
        syn::Item::Enum(item) => Some(&item.attrs),
        syn::Item::ExternCrate(item) => Some(&item.attrs),
        syn::Item::Fn(item) => Some(&item.attrs),
        syn::Item::ForeignMod(item) => Some(&item.attrs),
        syn::Item::Impl(item) => Some(&item.attrs),
        syn::Item::Macro(item) => Some(&item.attrs),
        syn::Item::Mod(item) => Some(&item.attrs),
        syn::Item::Static(item) => Some(&item.attrs),
        syn::Item::Struct(item) => Some(&item.attrs),
        syn::Item::Trait(item) => Some(&item.attrs),
        syn::Item::TraitAlias(item) => Some(&item.attrs),
        syn::Item::Type(item) => Some(&item.attrs),
        syn::Item::Union(item) => Some(&item.attrs),
        syn::Item::Use(item) => Some(&item.attrs),
        syn::Item::Verbatim(_) | _ => None,
    }
}

fn impl_item_attributes(item: &syn::ImplItem) -> Option<&[syn::Attribute]> {
    match item {
        syn::ImplItem::Const(item) => Some(&item.attrs),
        syn::ImplItem::Fn(item) => Some(&item.attrs),
        syn::ImplItem::Type(item) => Some(&item.attrs),
        syn::ImplItem::Macro(item) => Some(&item.attrs),
        syn::ImplItem::Verbatim(_) | _ => None,
    }
}

fn trait_item_attributes(item: &syn::TraitItem) -> Option<&[syn::Attribute]> {
    match item {
        syn::TraitItem::Const(item) => Some(&item.attrs),
        syn::TraitItem::Fn(item) => Some(&item.attrs),
        syn::TraitItem::Type(item) => Some(&item.attrs),
        syn::TraitItem::Macro(item) => Some(&item.attrs),
        syn::TraitItem::Verbatim(_) | _ => None,
    }
}

fn no_example_specific_engine_branches(workspace: &Path) -> Result<String, String> {
    let manifest = parse_toml(&workspace.join("examples/manifest.toml"))?;
    let examples = manifest
        .get("example")
        .and_then(toml::Value::as_array)
        .ok_or("example manifest has no example array")?;
    let mut names = examples
        .iter()
        .flat_map(|entry| [entry.get("id"), entry.get("label")])
        .flatten()
        .filter_map(toml::Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();

    let prefixes = [
        "crates/boon_parser/",
        "crates/boon_compiler/",
        "crates/boon_typecheck/",
        "crates/boon_semantic/",
        "crates/boon_verify/",
        "crates/boon_ir/",
        "crates/boon_plan/",
        "crates/boon_plan_executor/",
        "crates/boon_distributed_runtime/",
        "crates/boon_runtime/",
        "crates/boon_persistence/",
        "crates/boon_document_model/",
        "crates/boon_document/",
        "crates/boon_native_gpu/",
        "crates/boon_native_app_window/",
        "crates/boon_host/",
    ];
    let mut offenders = Vec::new();
    for relative in workspace_files(workspace)? {
        let generic_source = prefixes.iter().any(|prefix| relative.starts_with(prefix))
            || relative == "crates/boon_native_playground/src/verify.rs";
        if !generic_source || !relative.ends_with(".rs") || is_test_path(&relative) {
            continue;
        }
        let path = workspace.join(&relative);
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        let production_lines = rust_file_line_counts(&relative, &bytes)?.production;
        let source = std::str::from_utf8(&bytes).map_err(|error| error.to_string())?;
        let compact = source
            .lines()
            .take(production_lines)
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" ");
        for name in &names {
            let quoted = format!("\"{name}\"");
            let forbidden = [
                format!("== {quoted}"),
                format!("!= {quoted}"),
                format!("{quoted} =>"),
                format!(".contains({quoted})"),
                format!(".starts_with({quoted})"),
                format!(".ends_with({quoted})"),
            ];
            if forbidden.iter().any(|pattern| compact.contains(pattern)) {
                offenders.push(format!("{relative}:{quoted}"));
            }
        }
    }
    if offenders.is_empty() {
        Ok(
            "generic engine and native verifier control flow contains no built-in example identity"
                .to_owned(),
        )
    } else {
        Err(format!(
            "example-specific production branches: {}",
            bounded_list(&offenders)
        ))
    }
}

#[derive(Default)]
struct RustLineCounts {
    total: usize,
    tests: usize,
    playground: usize,
    xtask: usize,
    runtime_executor: usize,
}

fn rust_line_counts(workspace: &Path) -> Result<RustLineCounts, String> {
    let mut counts = RustLineCounts::default();
    for relative in workspace_files(workspace)?
        .into_iter()
        .filter(|path| path.ends_with(".rs"))
    {
        let path = workspace.join(&relative);
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let lines = rust_file_line_counts(&relative, &bytes)?;
        counts.total += lines.total;
        counts.tests += lines.tests;
        if relative.starts_with("crates/boon_native_playground/") {
            counts.playground += lines.production;
        }
        if relative.starts_with("crates/xtask/") {
            counts.xtask += lines.production;
        }
        if relative.starts_with("crates/boon_runtime/")
            || relative.starts_with("crates/boon_plan_executor/")
        {
            counts.runtime_executor += lines.production;
        }
    }
    Ok(counts)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RustFileLineCounts {
    total: usize,
    tests: usize,
    production: usize,
}

fn rust_file_line_counts(path: &str, bytes: &[u8]) -> Result<RustFileLineCounts, String> {
    let total = bytes.iter().filter(|byte| **byte == b'\n').count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
    if is_test_path(path) {
        return Ok(RustFileLineCounts {
            total,
            tests: total,
            production: 0,
        });
    }
    let source = std::str::from_utf8(bytes)
        .map_err(|error| format!("Rust source `{path}` is not UTF-8: {error}"))?;
    let lines = source.lines().collect::<Vec<_>>();
    let inline_test_start = lines.windows(2).rposition(|pair| {
        let module = pair[1].trim();
        pair[0].trim() == "#[cfg(test)]" && module.starts_with("mod ") && module.ends_with(" {")
    });
    let tests = inline_test_start.map_or(0, |start| total.saturating_sub(start));
    Ok(RustFileLineCounts {
        total,
        tests,
        production: total.saturating_sub(tests),
    })
}

fn is_test_path(path: &str) -> bool {
    path.contains("/tests/")
        || path.ends_with("/tests.rs")
        || path.ends_with("_test.rs")
        || path.ends_with("_tests.rs")
        || path.starts_with("tests/")
}

fn workspace_files(workspace: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    files.dedup();
    Ok(files)
}

fn parse_toml(path: &Path) -> Result<toml::Value, String> {
    let text = read_text(path)?;
    toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn bounded_list(values: &[String]) -> String {
    if values.is_empty() {
        return "none".to_owned();
    }
    let mut text = values
        .iter()
        .take(12)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if values.len() > 12 {
        text.push_str(&format!(", and {} more", values.len() - 12));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary_references(source: &str) -> Vec<(String, &'static str)> {
        let syntax = syn::parse_file(source).unwrap();
        let mut collector = BoundaryReferenceCollector::default();
        collector.visit_file(&syntax);
        collector.references.sort();
        collector.references
    }

    #[test]
    fn boundary_inventory_skips_test_items_and_finds_indirect_bypasses() {
        let references = boundary_references(
            r#"
fn production() {
    boon_semantic::elaborate(checked, &[]);
    semantic.into_lowering_parts();
}

use boon_verify::verify_explicit_contracts as verify;

macro_rules! hidden_bypass {
    () => { boon_ir::erase_and_lower(verified) };
}

#[cfg(test)]
fn test_only() {
    boon_verify::verify_explicit_contracts(semantic);
}
"#,
        );
        assert_eq!(
            references,
            vec![
                ("boon_semantic::elaborate".to_owned(), "expression"),
                (
                    "boon_verify::verify_explicit_contracts as verify".to_owned(),
                    "import"
                ),
                ("into_lowering_parts".to_owned(), "method"),
                (
                    "macro tokens containing `erase_and_lower`".to_owned(),
                    "macro"
                ),
            ]
        );
    }

    #[test]
    fn production_cfg_is_not_mistaken_for_test_only_cfg() {
        let feature_or_test: syn::ItemFn =
            syn::parse_str("#[cfg(any(test, feature = \"audit\"))] fn item() {}").unwrap();
        let test_only: syn::ItemFn =
            syn::parse_str("#[cfg(all(test, feature = \"audit\"))] fn item() {}").unwrap();
        let production: syn::ItemFn = syn::parse_str("#[cfg(not(test))] fn item() {}").unwrap();
        assert!(!cfg_is_test_only(&feature_or_test.attrs));
        assert!(cfg_is_test_only(&test_only.attrs));
        assert!(!cfg_is_test_only(&production.attrs));
    }

    #[test]
    fn production_dependency_inventory_includes_targets_but_not_dev_dependencies() {
        let manifest = toml::from_str(
            r#"
[dependencies]
boon_semantic = "1"

[dev-dependencies]
test_only = "1"

[target.'cfg(unix)'.dependencies]
boon_verify = "1"

[target.'cfg(unix)'.dev-dependencies]
target_test_only = "1"
"#,
        )
        .unwrap();
        assert_eq!(
            production_dependency_names(&manifest),
            BTreeSet::from(["boon_semantic", "boon_verify"])
        );
    }

    #[test]
    fn opaque_artifact_inventory_rejects_deserialization_and_public_constructors() {
        let valid = r#"
#[derive(Serialize)]
pub struct Artifact {
    fields: Fields,
}
struct Fields;
impl Artifact {
    pub fn fields(&self) -> &Fields { &self.fields }
}
"#;
        verify_opaque_artifact(valid, "Artifact", true).unwrap();

        let deserializable = valid.replace("Serialize", "Serialize, Deserialize");
        assert!(
            verify_opaque_artifact(&deserializable, "Artifact", true)
                .unwrap_err()
                .contains("Deserialize")
        );

        let constructible = valid.replace(
            "pub fn fields(&self)",
            "pub fn forge(fields: Fields) -> Self { Self { fields } }\n    pub fn fields(&self)",
        );
        assert!(
            verify_opaque_artifact(&constructible, "Artifact", true)
                .unwrap_err()
                .contains("public associated constructor")
        );

        verify_required_direct_field(
            "struct Provenance { digest: Digest }",
            "Provenance",
            "digest",
            "Digest",
        )
        .unwrap();
        assert!(
            verify_required_direct_field(
                "struct Provenance { digest: Option<Digest> }",
                "Provenance",
                "digest",
                "Digest",
            )
            .unwrap_err()
            .contains("nonoptional")
        );
    }

    fn phase1_boundary_analysis(
        source: &str,
    ) -> (
        Vec<String>,
        BTreeSet<String>,
        BTreeSet<String>,
        BTreeSet<String>,
        BTreeSet<String>,
    ) {
        let semantic_types = BTreeSet::from([
            "SemanticExecutionGraphV1".to_owned(),
            "SemanticExprId".to_owned(),
            "SemanticExpression".to_owned(),
        ]);
        let syntax = syn::parse_file(source).unwrap();
        let mut collector = IrPhase1BoundaryCollector::new(false, &semantic_types);
        collector.visit_file(&syntax);
        collector.violations.sort();
        collector.violations.dedup();
        (
            collector.violations,
            collector.graph_handoff_functions,
            collector.mapping_delegation_functions,
            collector.resource_mapping_delegation_functions,
            collector.totality_validation_functions,
        )
    }

    #[test]
    fn phase1_boundary_accepts_only_the_verified_mapping_handoff() {
        let (
            violations,
            graph_handoffs,
            mapping_delegations,
            resource_mapping_delegations,
            totality_validations,
        ) = phase1_boundary_analysis(
            r#"
fn lower_verified_semantic_execution(
    graph: boon_semantic::SemanticExecutionGraphV1,
    resources: Resources,
    reactive: Reactive,
) -> Result<(), String> {
    let mapped =
        semantic_mapping::map_semantic_execution_with_reactive(&graph, &resources, &reactive)?;
    mapped.validate_totality()?;
    let _resources =
        semantic_mapping::map_semantic_resources(&graph, &resources, &mapped.id_map)?;
    Ok(())
}

fn package_mapped_state_arms(mapped_state_update_arms: Vec<Arm>) -> PulseBatch {
    PulseBatch {
        state_update_arms: mapped_state_update_arms,
    }
}

#[cfg(test)]
mod arbitrary_test_support_name {
    use boon_typecheck::CheckedProgram as CP;
    use boon_semantic::{out_net as hidden, SemanticExpression as Input};

    macro_rules! test_bypass {
        () => { lower_semantic_unverified::<CP, Input, hidden>() };
    }
}

macro_rules! diagnostic_text {
    () => { "CheckedProgram contextual_expansion lower_semantic_unverified" };
}
"#,
        );
        assert!(violations.is_empty(), "{violations:#?}");
        let expected = BTreeSet::from(["lower_verified_semantic_execution".to_owned()]);
        assert_eq!(graph_handoffs, expected);
        assert_eq!(mapping_delegations, expected);
        assert_eq!(resource_mapping_delegations, expected);
        assert_eq!(totality_validations, expected);
    }

    #[test]
    fn phase1_boundary_rejects_aliases_imports_macros_and_production_cfgs() {
        for (label, source, expected) in [
            (
                "renamed checked artifact",
                "use boon_typecheck::{CheckedProgram as CP}; fn bypass(_: CP) {}",
                "CheckedProgram",
            ),
            (
                "renamed out net",
                "use boon_semantic::out_net as hidden; fn bypass() { hidden::build(); }",
                "out_net",
            ),
            (
                "renamed old helper",
                "use crate::contextual_expansion::derive_executable_program as convert;",
                "derive_executable_program",
            ),
            (
                "old reactive discovery helper",
                "fn state_update_arms() {} fn bypass() { state_update_arms(); }",
                "state_update_arms",
            ),
            (
                "semantic DTO outside map",
                "use boon_semantic::SemanticExpression as Input; fn convert(_: Input) {}",
                "SemanticExpression",
            ),
            (
                "macro-hidden artifact",
                "macro_rules! bypass { () => { let _: CheckedProgram = forge!(); }; }",
                "CheckedProgram",
            ),
            (
                "macro-hidden raw lowerer",
                "macro_rules! bypass { () => { lower_semantic_unverified(graph); }; }",
                "lower_semantic_unverified",
            ),
            (
                "macro-hidden resource mapper",
                "macro_rules! bypass { () => { map_semantic_resources(graph); }; }",
                "semantic resource conversion",
            ),
            (
                "production-capable cfg",
                "#[cfg(any(test, feature = \"audit\"))] fn bypass(_: CheckedProgram) {}",
                "CheckedProgram",
            ),
            (
                "not-test cfg",
                "#[cfg(not(test))] fn bypass(_: ResolvedOutGraph) {}",
                "ResolvedOutGraph",
            ),
        ] {
            let (violations, _, _, _, _) = phase1_boundary_analysis(source);
            assert!(
                violations
                    .iter()
                    .any(|violation| violation.contains(expected)),
                "{label}: expected `{expected}` in {violations:#?}"
            );
        }
    }

    #[test]
    fn phase1_file_inventory_is_exact_and_rejects_old_module_files() {
        let valid = BTreeSet::from([
            BOON_IR_LIB.to_owned(),
            BOON_IR_SEMANTIC_MAPPING.to_owned(),
            "crates/boon_ir/src/tests.rs".to_owned(),
        ]);
        verify_boon_ir_file_inventory(&valid).unwrap();

        for old in [
            "crates/boon_ir/src/contextual_expansion.rs",
            "crates/boon_ir/src/semantic_migration/mod.rs",
        ] {
            let mut invalid = valid.clone();
            invalid.insert(old.to_owned());
            let error = verify_boon_ir_file_inventory(&invalid).unwrap_err();
            assert!(error.contains(old), "{error}");
        }

        let mut split_mapping = valid;
        split_mapping.insert("crates/boon_ir/src/semantic_mapping/helpers.rs".to_owned());
        assert!(
            verify_boon_ir_file_inventory(&split_mapping)
                .unwrap_err()
                .contains("exactly")
        );
    }

    #[test]
    fn semantic_mapping_contract_requires_totality_and_bijection_evidence() {
        let mapping = include_str!("../../boon_ir/src/semantic_mapping.rs");
        verify_semantic_mapping_contract(mapping).unwrap();

        let missing_totality = mapping.replacen(
            "self.id_map.states.len()",
            "self.executable.states.len()",
            1,
        );
        let error = verify_semantic_mapping_contract(&missing_totality).unwrap_err();
        assert!(error.contains("states"), "{error}");

        let missing_bijection = mapping.replacen(
            "let unique_expressions = expressions.iter().copied().collect::<BTreeSet<_>>();",
            "let unique_expressions = expressions.iter().copied().collect::<Vec<_>>();",
            1,
        );
        let error = verify_semantic_mapping_contract(&missing_bijection).unwrap_err();
        assert!(error.contains("bijection"), "{error}");

        let missing_resource_identity = mapping.replacen("    row_scopes: Vec<ScopeId>,\n", "", 1);
        let error = verify_semantic_mapping_contract(&missing_resource_identity).unwrap_err();
        assert!(error.contains("row_scopes"), "{error}");

        let missing_explicit_erasure =
            mapping.replacen("    value_list_authorities: Vec<()>,\n", "", 1);
        let error = verify_semantic_mapping_contract(&missing_explicit_erasure).unwrap_err();
        assert!(error.contains("value_list_authorities"), "{error}");
    }
}
