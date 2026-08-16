use crate::report_v2::{CheckOutcome, GateEvidence, check, empty_evidence};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syn::visit::Visit;

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
        "legacy-owner-solver-test-only",
        legacy_owner_solver_test_only(workspace),
    );
    push_check(
        &mut checks,
        "pinned-production-rust-toolchain",
        pinned_production_rust_toolchain(workspace),
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
            checks.push(check(
                "tracked-rust-loc-inventory",
                CheckOutcome::Pass,
                format!(
                    "tracked Rust: {} lines; repository-wide count is telemetry, not a deletion gate",
                    counts.total
                ),
            ));
            checks.push(check(
                "test-rust-loc-inventory",
                CheckOutcome::Pass,
                format!(
                    "test Rust: {} lines; repository-wide count is telemetry, not a deletion gate",
                    counts.tests
                ),
            ));
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

fn pinned_production_rust_toolchain(workspace: &Path) -> Result<String, String> {
    let manifest = parse_toml(&workspace.join("Cargo.toml"))?;
    let resolver = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("resolver"))
        .and_then(toml::Value::as_str)
        .ok_or("workspace resolver is missing")?;
    if resolver != "3" {
        return Err(format!(
            "Edition 2024 workspace must use rust-version-aware resolver 3; found {resolver}"
        ));
    }
    let minimum = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("rust-version"))
        .and_then(toml::Value::as_str)
        .ok_or("workspace.package.rust-version is missing")?;
    let toolchain = parse_toml(&workspace.join("rust-toolchain.toml"))?;
    let channel = toolchain
        .get("toolchain")
        .and_then(|toolchain| toolchain.get("channel"))
        .and_then(toml::Value::as_str)
        .ok_or("rust-toolchain.toml toolchain.channel is missing")?;
    if channel.split('.').count() != 3
        || !channel
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!(
            "production Rust channel must be an exact stable x.y.z release; found {channel}"
        ));
    }
    let components = toolchain
        .get("toolchain")
        .and_then(|toolchain| toolchain.get("components"))
        .and_then(toml::Value::as_array)
        .ok_or("rust-toolchain.toml toolchain.components is missing")?;
    for required in ["clippy", "rustfmt"] {
        if !components
            .iter()
            .any(|component| component.as_str() == Some(required))
        {
            return Err(format!(
                "production Rust toolchain omits required component {required}"
            ));
        }
    }
    let dockerfile = read_text(&workspace.join("deploy/fjordpulse/Dockerfile"))?;
    let expected_builder = format!("FROM rust:{channel}-bookworm AS builder");
    if !dockerfile.lines().any(|line| line == expected_builder) {
        return Err(format!(
            "FjordPulse builder must use production Rust {channel} exactly"
        ));
    }
    Ok(format!(
        "production Rust {channel}; compatibility floor {minimum}; resolver {resolver}"
    ))
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

fn legacy_owner_solver_test_only(workspace: &Path) -> Result<String, String> {
    const FEATURE: &str = "legacy-owner-oracle";
    const MODULES: &[&str] = &[
        "owner_body",
        "owner_checked",
        "owner_compat",
        "owner_constraints",
        "owner_diagnostics",
        "owner_interface",
        "owner_shard_builder",
        "owner_signature_lexical",
        "owner_syntax",
    ];

    let typecheck_manifest = parse_toml(&workspace.join("crates/boon_typecheck/Cargo.toml"))?;
    let compiler_manifest = parse_toml(&workspace.join("crates/boon_compiler/Cargo.toml"))?;
    let typecheck_source = read_text(&workspace.join("crates/boon_typecheck/src/lib.rs"))?;

    if typecheck_manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .and_then(|features| features.get(FEATURE))
        .is_none()
    {
        return Err(format!(
            "boon_typecheck does not declare the explicit `{FEATURE}` oracle feature"
        ));
    }

    let gated_prefix = format!("#[cfg(any(test, feature = \"{FEATURE}\"))]");
    for module in MODULES {
        let declaration = format!("{gated_prefix}\nmod {module};");
        let export = format!("{gated_prefix}\npub use {module}::*;");
        if !typecheck_source.contains(&declaration) || !typecheck_source.contains(&export) {
            return Err(format!(
                "legacy owner module `{module}` is not gated at both declaration and export"
            ));
        }
    }

    let production_dependency = compiler_manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|dependencies| dependencies.get("boon_typecheck"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "boon_compiler lacks its explicit boon_typecheck dependency".to_owned())?;
    if production_dependency
        .get("features")
        .and_then(toml::Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some(FEATURE))
        })
    {
        return Err("boon_compiler production enables the legacy owner solver".to_owned());
    }

    let test_feature = compiler_manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .and_then(|features| features.get("test-kernel-oracle"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| "boon_compiler lacks its bounded kernel oracle feature".to_owned())?;
    if !test_feature
        .iter()
        .any(|feature| feature.as_str() == Some("boon_typecheck/legacy-owner-oracle"))
    {
        return Err(
            "the bounded kernel oracle does not explicitly enable the legacy owner solver"
                .to_owned(),
        );
    }

    let development_dependency = compiler_manifest
        .get("dev-dependencies")
        .and_then(toml::Value::as_table)
        .and_then(|dependencies| dependencies.get("boon_typecheck"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            "boon_compiler tests lack their explicit owner oracle dependency".to_owned()
        })?;
    if !development_dependency
        .get("features")
        .and_then(toml::Value::as_array)
        .is_some_and(|features| {
            features
                .iter()
                .any(|feature| feature.as_str() == Some(FEATURE))
        })
    {
        return Err("boon_compiler tests do not explicitly enable the owner oracle".to_owned());
    }

    Ok(format!(
        "{} legacy owner modules are absent from production and available only to explicit differential tests",
        MODULES.len()
    ))
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
    if !semantic_dependencies.contains("boon_checked")
        || !semantic_dependencies.contains("boon_contract")
        || semantic_dependencies.contains("boon_typecheck")
    {
        return Err(
            "boon_semantic must consume boon_checked/boon_contract without a production typechecker dependency"
                .to_owned(),
        );
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
    let program_core = read_text(&workspace.join("crates/boon_semantic/src/program_core.rs"))?;
    let verify = read_text(&workspace.join("crates/boon_verify/src/lib.rs"))?;
    let ir = read_text(&workspace.join("crates/boon_ir/src/lib.rs"))?;
    let parser = read_text(&workspace.join("crates/boon_parser/src/lib.rs"))?;
    let syntax = read_text(&workspace.join("crates/boon_syntax/src/lib.rs"))?;
    let checked = read_text(&workspace.join("crates/boon_checked/src/lib.rs"))?;
    let typecheck = read_text(&workspace.join("crates/boon_typecheck/src/lib.rs"))?;
    let compiler = read_text(&workspace.join("crates/boon_compiler/src/lib.rs"))?;
    let distributed =
        read_text(&workspace.join("crates/boon_compiler/src/distributed_compiler.rs"))?;

    verify_semantic_core_ownership_boundary(workspace)?;
    verify_opaque_artifact(&parser, "ParsedProgram", true, None)?;
    verify_opaque_artifact(
        &checked,
        "CheckedProgram",
        true,
        Some("from_typechecker_parts_unchecked"),
    )?;
    verify_opaque_artifact(&semantic, "SemanticProgram", false, None)?;
    verify_opaque_artifact(&verify, "ContractVerifiedProgram", false, None)?;
    verify_opaque_artifact(&ir, "ErasedProgram", true, None)?;
    if typecheck
        .matches("CheckedProgram::from_typechecker_parts_unchecked(")
        .count()
        != 2
        || !typecheck
            .contains("pub fn seal_project_checked_program_construction_with_call_occurrences(")
        || !typecheck.contains("fn seal_checked_program_fields(")
    {
        return Err(
            "typechecker must cross the unsafe CheckedProgram seal through exactly one legacy and one compact-kernel entrypoint"
                .to_owned(),
        );
    }
    verify_required_direct_field(
        &syntax,
        "ParsedProgramFields",
        "source_bundle_digest_v1",
        "SourceBundleDigestV1",
    )?;
    verify_required_direct_field(
        &checked,
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
        &semantic,
        "SemanticProgram",
        "canonical_core",
        "CanonicalProgramCoreV2",
    )?;
    verify_required_direct_field(
        &semantic,
        "SemanticProgram",
        "semantic_image",
        "SealedSemanticImageV3",
    )?;
    verify_required_direct_field(&ir, "ErasedProgram", "fields", "CanonicalProgramCoreV2")?;
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
    if exact_struct_definition_lines(&program_core, "CanonicalProgramCoreV2").count() != 1 {
        return Err("boon_semantic must own exactly one public CanonicalProgramCoreV2".to_owned());
    }
    for forbidden in [
        "pub struct ErasedProgramFields",
        "pub type ErasedProgramFields",
        "pub use boon_semantic::program_core",
    ] {
        if ir.contains(forbidden) {
            return Err(format!(
                "boon_ir retains compatibility ownership for the semantic core via `{forbidden}`"
            ));
        }
    }
    for required in [
        "pub struct SemanticProgram",
        "pub fn elaborate(",
        "CallableDependencyManifestV7",
        "#[cfg(test)]\n    checked_program: CheckedProgramFields",
        "#[cfg(test)]\n    execution_graph: SemanticExecutionImageColumnsV1",
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
    for required in [
        "pub struct CompileRequest<'a>",
        "pub fn compile_machine_plan(",
        "pub fn compile_erased_program(",
    ] {
        if !compiler.contains(required) {
            return Err(format!("compiler omits canonical entrypoint `{required}`"));
        }
    }
    for forbidden in [
        "pub fn compile_typed_program",
        "pub fn compile_source_path_to_machine_plan",
        "pub fn compile_source_text_to_machine_plan",
        "pub fn compile_source_units_to_machine_plan",
        "pub fn compile_runtime_source_text_to_machine_plan",
        "pub fn compile_runtime_source_units_to_machine_plan",
        "pub fn compile_parsed_program_to_machine_plan",
    ] {
        if compiler.contains(forbidden) {
            return Err(format!(
                "compiler retains superseded public entrypoint `{forbidden}`"
            ));
        }
    }

    Ok(
        "one explicit CompileRequest follows CheckedProgram -> SemanticProgram -> ContractVerifiedProgram -> opaque ErasedProgram"
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
    allowed_public_unsafe_constructor: Option<&str>,
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
    let has_manual_serialize_impl = syntax.items.iter().any(|item| {
        let syn::Item::Impl(implementation) = item else {
            return false;
        };
        let syn::Type::Path(self_type) = implementation.self_ty.as_ref() else {
            return false;
        };
        let Some((_, trait_path, _)) = &implementation.trait_ else {
            return false;
        };
        self_type
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == artifact)
            && trait_path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Serialize")
    });
    if must_serialize && !derives.contains("Serialize") && !has_manual_serialize_impl {
        return Err(format!("opaque artifact `{artifact}` is not serializable"));
    }
    for forbidden in ["Deserialize", "Default"] {
        if derives.contains(forbidden) {
            return Err(format!(
                "opaque artifact `{artifact}` derives forgeable `{forbidden}`"
            ));
        }
    }

    let mut allowed_public_unsafe_constructor_seen = false;
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
                    if allowed_public_unsafe_constructor.is_some_and(|allowed| {
                        function.sig.ident == allowed && function.sig.unsafety.is_some()
                    }) {
                        allowed_public_unsafe_constructor_seen = true;
                        continue;
                    }
                    return Err(format!(
                        "opaque artifact `{artifact}` exposes public associated constructor `{}`",
                        function.sig.ident
                    ));
                }
            }
        }
    }
    if let Some(constructor) = allowed_public_unsafe_constructor
        && !allowed_public_unsafe_constructor_seen
    {
        return Err(format!(
            "opaque artifact `{artifact}` omits allowed unsafe constructor `{constructor}`"
        ));
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

struct ProductionIdentifierReferenceCollector<'a> {
    identifier: &'a str,
    references: Vec<&'static str>,
}

impl<'a> ProductionIdentifierReferenceCollector<'a> {
    fn new(identifier: &'a str) -> Self {
        Self {
            identifier,
            references: Vec::new(),
        }
    }
}

impl<'ast> Visit<'ast> for ProductionIdentifierReferenceCollector<'_> {
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

    fn visit_expr_block(&mut self, expression: &'ast syn::ExprBlock) {
        if cfg_is_test_only(&expression.attrs) {
            return;
        }
        syn::visit::visit_expr_block(self, expression);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if expression
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident.to_string() == self.identifier)
        {
            self.references.push("expression");
        }
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        if rust_token_text_contains_ident(&item.tokens.to_string(), self.identifier) {
            self.references.push("macro");
        }
        syn::visit::visit_macro(self, item);
    }
}

fn verify_semantic_core_ownership_boundary(workspace: &Path) -> Result<(), String> {
    let ir_path = workspace.join("crates/boon_ir/src/lib.rs");
    let semantic_path = workspace.join("crates/boon_semantic/src/lib.rs");
    let lowering_path = workspace.join("crates/boon_semantic/src/core_lowering.rs");
    let semantic_image_path = workspace.join("crates/boon_semantic/src/semantic_image.rs");
    let reactive_path = workspace.join("crates/boon_semantic/src/reactive.rs");
    let dependency_manifest_path =
        workspace.join("crates/boon_semantic/src/dependency_manifest.rs");
    for obsolete in [
        workspace.join("crates/boon_ir/src/semantic_mapping.rs"),
        workspace.join("crates/boon_ir/src/semantic_mapping"),
        workspace.join("crates/boon_ir/src/contextual_expansion.rs"),
        workspace.join("crates/boon_ir/src/semantic_migration.rs"),
    ] {
        if obsolete.exists() {
            return Err(format!(
                "obsolete post-verification lowering owner remains at `{}`",
                obsolete.display()
            ));
        }
    }

    let ir = read_text(&ir_path)?;
    let semantic = read_text(&semantic_path)?;
    let lowering = read_text(&lowering_path)?;
    let semantic_image = read_text(&semantic_image_path)?;
    let reactive = read_text(&reactive_path)?;
    let dependency_manifest = read_text(&dependency_manifest_path)?;
    syn::parse_file(&ir)
        .map_err(|error| format!("cannot parse `{}`: {error}", ir_path.display()))?;
    syn::parse_file(&lowering)
        .map_err(|error| format!("cannot parse `{}`: {error}", lowering_path.display()))?;
    syn::parse_file(&semantic_image)
        .map_err(|error| format!("cannot parse `{}`: {error}", semantic_image_path.display()))?;
    let reactive_syntax = syn::parse_file(&reactive)
        .map_err(|error| format!("cannot parse `{}`: {error}", reactive_path.display()))?;
    let dependency_manifest_syntax = syn::parse_file(&dependency_manifest).map_err(|error| {
        format!(
            "cannot parse `{}`: {error}",
            dependency_manifest_path.display()
        )
    })?;

    for forbidden in [
        "mod semantic_mapping",
        "SemanticExecutionGraphV1",
        "SemanticExecutionImageColumnsV1",
        "SemanticResourceGraphV2",
        "SemanticReactiveGraphV1",
        "map_semantic_execution",
        "map_semantic_resources",
        "lower_verified_semantic_execution",
        "build_canonical_program_core",
        "CanonicalProgramCoreV2 {",
    ] {
        if ir.contains(forbidden) {
            return Err(format!(
                "boon_ir retains semantic-core construction authority via `{forbidden}`"
            ));
        }
    }
    for required in [
        "bind_verified_pulse_fusion(&mut fields, &pulse_fusion_decisions)?;",
        "PulseFusionEligibility::PendingVerification",
        "semantic.into_lowering_parts()",
    ] {
        if !ir.contains(required) {
            return Err(format!(
                "boon_ir omits verification-gated core handoff `{required}`"
            ));
        }
    }

    for required in [
        "mod core_lowering;",
        "canonical_core: program_core::CanonicalProgramCoreV2",
        "canonical_core_digest: [u8; 32]",
        "validate_canonical_core_handoff(self)?;",
        "core_lowering::build_canonical_program_core(",
    ] {
        if !semantic.contains(required) {
            return Err(format!(
                "SemanticProgram omits canonical-core ownership join `{required}`"
            ));
        }
    }
    if semantic
        .matches("core_lowering::build_canonical_program_core(")
        .count()
        != 1
    {
        return Err(
            "semantic elaboration must construct exactly one retained canonical core".to_owned(),
        );
    }

    if lowering.contains("boon_verify::") || lowering.contains("#[cfg(test)]\nmod tests") {
        return Err(
            "semantic core construction depends on verification or retains private mapping tests"
                .to_owned(),
        );
    }
    for required in [
        "pub(crate) fn build_canonical_program_core(",
        "struct CanonicalProgramCoreBuildV2",
        "execution_handoff: crate::semantic_image::ExecutionImageHandoffV3",
        "manifest_prefix: crate::dependency_manifest::ManifestCheckedExecutionPrefixV7",
        "ExecutionReceiptPublisherV3<'_>",
        "struct SemanticToExecutableMap",
        "fn validate_allocation_bijections(",
        "struct SemanticReactiveToMappedMap",
        "struct SemanticStorageToErasedMap",
        "fusion: program_core::PulseFusionEligibility::PendingVerification",
    ] {
        if !lowering.contains(required) {
            return Err(format!(
                "semantic core construction omits ownership proof `{required}`"
            ));
        }
    }
    if lowering
        .matches("program_core::CanonicalProgramCoreV2 {")
        .count()
        != 1
    {
        return Err(
            "semantic core construction must emit exactly one CanonicalProgramCoreV2".to_owned(),
        );
    }
    if lowering.matches("receipts.publish_").count() != 11 {
        return Err(
            "canonical lowering must publish all eleven executable receipt domains in its construction transaction"
                .to_owned(),
        );
    }
    for forbidden in [
        "execution_payload_seals_v3",
        "payload_seals_v3: crate::semantic_image::ExecutionRowPayloadSealsV3",
    ] {
        if lowering.contains(forbidden) {
            return Err(format!(
                "canonical lowering retains the deleted post-hoc receipt side table `{forbidden}`"
            ));
        }
    }
    if semantic.contains("finalize_executable_receipts") {
        return Err(
            "semantic elaboration retains the deleted post-hoc executable receipt phase".to_owned(),
        );
    }
    for required in [
        "pub(crate) struct ExecutionReceiptPublisherV3",
        "construction-published execution V3 handoff differs from the post-hoc oracle",
    ] {
        if !semantic_image.contains(required) {
            return Err(format!(
                "execution image omits direct construction receipt proof `{required}`"
            ));
        }
    }
    for required in [
        "pub(crate) struct ManifestCheckedExecutionPrefixBuilderV7",
        "pub(crate) struct ManifestCheckedExecutionPrefixV7",
        "construction-published checked/execution prefix differs from the replay oracle",
        "\"consume_checked_execution_prefix\"",
        "#[cfg(test)]\nfn build_dense_projection_index_v7",
    ] {
        if !dependency_manifest.contains(required) {
            return Err(format!(
                "dependency manifest omits construction-published prefix proof `{required}`"
            ));
        }
    }
    let mut replay_references =
        ProductionIdentifierReferenceCollector::new("build_dense_projection_index_v7");
    replay_references.visit_file(&dependency_manifest_syntax);
    if !replay_references.references.is_empty() {
        return Err(format!(
            "production dependency-manifest code replays checked/execution projections at {:?}",
            replay_references.references,
        ));
    }
    verify_required_direct_field(
        &reactive,
        "SemanticReactiveGraphBuildV1",
        "publication",
        "ReactiveDependencyPublicationV1",
    )?;
    for required in [
        "build_semantic_reactive_graph_with_dependency_publication_from_validated_inputs",
        "publish_reactive_dependencies_v1",
    ] {
        if !reactive.contains(required) {
            return Err(format!(
                "reactive construction omits dependency publication seam `{required}`"
            ));
        }
    }
    for required in [
        "ConstructionDependencyDomainV1::Reactive",
        "\"reactive_publication_oracle\"",
        "validate_reactive_dependency_publication_against_oracle",
        "construction-published reactive component digest differs from the replay oracle",
        "construction-published reactive row {ordinal} projection differs from the replay oracle",
        "construction-published reactive row {ordinal} references differ from the replay oracle",
        "#[cfg(test)]\nfn inventory_reactive",
        "\"consume_reactive_rows\"",
        "reference_arena: Vec<PendingDependencyReference>",
        "enum PendingDependencySegmentV7",
        "fn consume_construction_rows(",
        "fn projection_receipts_digest(",
    ] {
        if !dependency_manifest.contains(required) {
            return Err(format!(
                "dependency manifest omits construction-published reactive proof `{required}`"
            ));
        }
    }
    let mut reactive_replay_references =
        ProductionIdentifierReferenceCollector::new("inventory_reactive");
    reactive_replay_references.visit_file(&dependency_manifest_syntax);
    if !reactive_replay_references.references.is_empty() {
        return Err(format!(
            "production dependency-manifest code replays the reactive graph at {:?}",
            reactive_replay_references.references,
        ));
    }
    let mut reactive_publication_references =
        ProductionIdentifierReferenceCollector::new("publish_reactive_dependencies_v1");
    reactive_publication_references.visit_file(&reactive_syntax);
    if reactive_publication_references.references != ["macro"] {
        return Err(format!(
            "reactive construction must publish dependency rows exactly once; found {:?}",
            reactive_publication_references.references,
        ));
    }
    let mut manifest_reactive_publication_references =
        ProductionIdentifierReferenceCollector::new("publish_reactive_dependencies_v1");
    manifest_reactive_publication_references.visit_file(&dependency_manifest_syntax);
    if !manifest_reactive_publication_references
        .references
        .is_empty()
    {
        return Err(format!(
            "production dependency-manifest code reconstructs reactive publication at {:?}",
            manifest_reactive_publication_references.references,
        ));
    }
    for forbidden in [
        "CompactDependencyRowOriginV7::Construction",
        "fn ingest_construction_rows(",
        "\"ingest_reactive_rows\"",
        "fn receipt_members(",
    ] {
        if dependency_manifest.contains(forbidden) {
            return Err(format!(
                "dependency manifest retains deleted construction-row replay `{forbidden}`"
            ));
        }
    }
    for forbidden in [
        "fn validate_totality(",
        "fn validate_callable_and_call_inventory(",
        "fn validate_erased_resource_metadata(",
        "fn validate_reactive_call_and_host_schedules(",
        "fn validate_call_expression(",
        "fn runtime_type_matches_scheme(",
        "MappedSemanticNamedValue",
        "MappedSemanticNamedValueProjection",
        "MappedSemanticNamedValueTarget",
        "MappedSemanticStorageFixedBytesRefinement",
        "MappedSemanticStorageRepresentation",
        "MappedSemanticStorageTypePathSegment",
    ] {
        if lowering.contains(forbidden) {
            return Err(format!(
                "semantic core construction restores proof shadow `{forbidden}`"
            ));
        }
    }
    Ok(())
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
            path: "boon_semantic::elaborate".to_owned(),
            kind: "expression",
        },
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
            path: "boon_verify::verify_explicit_contracts".to_owned(),
            kind: "expression",
        },
        BoundaryReference {
            file: "crates/boon_compiler/src/lib.rs".to_owned(),
            path: "boon_ir::erase_and_lower".to_owned(),
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
            let canonical_core_glob = prefix.as_slice() == ["boon_semantic", "program_core"];
            if !canonical_core_glob
                && prefix.first().is_some_and(|root| {
                    matches!(root.as_str(), "boon_semantic" | "boon_verify" | "boon_ir")
                })
            {
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
        "crates/boon_syntax/",
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
        "crates/boon_host_runtime/src/effects.rs",
        "crates/boon_host_runtime/src/migration_scenario.rs",
        "crates/boon_host_runtime/src/persistent.rs",
        "crates/boon_host_runtime/src/persistent_program_session.rs",
        "crates/boon_program_runtime/",
        "crates/boon_web_host/src/web_persistent.rs",
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
        verify_opaque_artifact(valid, "Artifact", true, None).unwrap();

        let deserializable = valid.replace("Serialize", "Serialize, Deserialize");
        assert!(
            verify_opaque_artifact(&deserializable, "Artifact", true, None)
                .unwrap_err()
                .contains("Deserialize")
        );

        let constructible = valid.replace(
            "pub fn fields(&self)",
            "pub fn forge(fields: Fields) -> Self { Self { fields } }\n    pub fn fields(&self)",
        );
        assert!(
            verify_opaque_artifact(&constructible, "Artifact", true, None)
                .unwrap_err()
                .contains("public associated constructor")
        );

        let unsafe_seal = valid.replace(
            "pub fn fields(&self)",
            "pub unsafe fn seal(fields: Fields) -> Self { Self { fields } }\n    pub fn fields(&self)",
        );
        verify_opaque_artifact(&unsafe_seal, "Artifact", true, Some("seal")).unwrap();

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
}
