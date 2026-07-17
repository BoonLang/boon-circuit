use crate::report_v2::{CheckOutcome, GateEvidence, check, empty_evidence};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        if matches!(package, "boon_cli" | "xtask") {
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
        if manifest_has_dependency(&manifest, "serde_json") {
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

fn manifest_has_dependency(value: &toml::Value, dependency: &str) -> bool {
    let Some(table) = value.as_table() else {
        return false;
    };
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if table
            .get(key)
            .and_then(toml::Value::as_table)
            .is_some_and(|dependencies| dependencies.contains_key(dependency))
        {
            return true;
        }
    }
    table
        .values()
        .any(|child| manifest_has_dependency(child, dependency))
}

fn single_execution_path(workspace: &Path) -> Result<String, String> {
    let files = workspace_files(workspace)?;
    let mut machine_plan_definitions = Vec::new();
    let mut duplicate_runtime_types = Vec::new();
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
        for _ in text.match_indices("pub struct MachinePlan") {
            machine_plan_definitions.push(relative.clone());
        }
        if !relative.starts_with("crates/boon_plan_executor/")
            && [
                "struct PlanExecutorLiveSession",
                "struct PlanExecutorRuntimeState",
                "struct PlanExecutorOutputEvaluator",
            ]
            .iter()
            .any(|marker| text.contains(marker))
        {
            duplicate_runtime_types.push(relative.clone());
        }
    }
    let executor_source = read_text(&workspace.join("crates/boon_plan_executor/src/lib.rs"))?;
    let executor_session = read_text(&workspace.join("crates/boon_plan_executor/src/session.rs"))?;
    let runtime_source = read_text(&workspace.join("crates/boon_runtime/src/lib.rs"))?;
    let session_owned_by_executor =
        executor_source.contains("Session") && executor_session.contains("pub struct Session");
    let runtime_uses_session = runtime_source.contains("boon_plan_executor::Session")
        || (runtime_source.contains("use boon_plan_executor")
            && runtime_source.contains("Session"));

    let mut direct_executor_dependents = Vec::new();
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
        if package != "boon_plan_executor"
            && manifest
                .get("dependencies")
                .and_then(toml::Value::as_table)
                .is_some_and(|dependencies| dependencies.contains_key("boon_plan_executor"))
        {
            direct_executor_dependents.push(package.to_owned());
        }
    }
    direct_executor_dependents.sort();

    let valid = machine_plan_definitions == vec!["crates/boon_plan/src/lib.rs".to_owned()]
        && duplicate_runtime_types.is_empty()
        && session_owned_by_executor
        && runtime_uses_session
        && direct_executor_dependents == vec!["boon_runtime".to_owned()];
    if valid {
        Ok("one MachinePlan definition and one boon_plan_executor::Session path".to_owned())
    } else {
        Err(format!(
            "MachinePlan defs={}; duplicate runtime executors={}; executor Session={session_owned_by_executor}; runtime uses Session={runtime_uses_session}; direct dependents={}",
            bounded_list(&machine_plan_definitions),
            bounded_list(&duplicate_runtime_types),
            bounded_list(&direct_executor_dependents)
        ))
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
        "crates/boon_ir/",
        "crates/boon_plan/",
        "crates/boon_plan_executor/",
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
        let bytes = fs::read(workspace.join(&relative)).map_err(|error| error.to_string())?;
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
