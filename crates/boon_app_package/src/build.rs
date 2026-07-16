use crate::{
    APP_MANIFEST_FORMAT, AppManifest, ArtifactDescriptor, BUNDLE_FORMAT, BUNDLE_MANIFEST_FILE,
    BundleFileDescriptor, BundleFileKind, BundleManifest, MAX_PACKAGE_FILE_BYTES, NamespaceProfile,
    PackageError, PackageFileManifest, ProgramManifest, RunMode, StaticCachePolicy, sha256_bytes,
};
use boon_plan::{ApplicationIdentity, ProgramRole};
use boon_runtime::{
    ProgramArtifact, ProgramCompileRequest, RuntimeSourceUnit,
    compile_trusted_package_program_artifact,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use wasm_bindgen_cli_support::Bindgen;

const PROGRAM_ARTIFACT_REVISION: u64 = 1;

pub struct BuildRequest<'a> {
    pub manifest_path: &'a Path,
    pub output_dir: &'a Path,
    pub run_mode: RunMode,
    pub namespace_profile: NamespaceProfile,
    pub browser_wasm: &'a Path,
    pub source_revision: &'a str,
    pub force: bool,
}

pub struct BuildResult {
    pub output_dir: PathBuf,
    pub manifest: BundleManifest,
}

pub fn build_app_package(request: BuildRequest<'_>) -> Result<BuildResult, PackageError> {
    validate_build_request(&request)?;
    let manifest = AppManifest::from_path(request.manifest_path)?;
    if !manifest.supports_mode(request.run_mode) {
        return Err(PackageError::new(format!(
            "package does not support {} mode",
            request.run_mode.as_str()
        )));
    }
    if request.namespace_profile == NamespaceProfile::Production
        && request.run_mode != RunMode::Live
    {
        return Err(PackageError::new(
            "production namespace packages must be built in live mode",
        ));
    }
    if request.namespace_profile == NamespaceProfile::Deterministic
        && request.run_mode != RunMode::Deterministic
    {
        return Err(PackageError::new(
            "deterministic namespace packages must be built in deterministic mode",
        ));
    }

    let manifest_path = fs::canonicalize(request.manifest_path)
        .map_err(|error| PackageError::context("canonicalize app manifest", error))?;
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| PackageError::new("app manifest has no parent directory"))?;
    let workspace_root = find_workspace_root(manifest_dir)?;
    let output = absolute_output_path(request.output_dir)?;
    let parent = output
        .parent()
        .ok_or_else(|| PackageError::new("package output has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.boon-build-{}",
        output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("package"),
        std::process::id()
    ));
    if temp.exists() {
        fs::remove_dir_all(&temp)
            .map_err(|error| PackageError::context("remove stale package temp directory", error))?;
    }
    fs::create_dir(&temp)?;

    let result = build_into(
        &manifest,
        &manifest_path,
        manifest_dir,
        &workspace_root,
        &temp,
        request.run_mode,
        request.namespace_profile,
        request.browser_wasm,
        request.source_revision,
    );
    let bundle_manifest = match result {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_dir_all(&temp);
            return Err(error);
        }
    };
    if output.exists() {
        if !request.force {
            let _ = fs::remove_dir_all(&temp);
            return Err(PackageError::new(format!(
                "package output `{}` already exists; pass --force to replace it",
                output.display()
            )));
        }
        let metadata = fs::symlink_metadata(&output)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            let _ = fs::remove_dir_all(&temp);
            return Err(PackageError::new(
                "existing package output is not a regular directory",
            ));
        }
        fs::remove_dir_all(&output)?;
    }
    fs::rename(&temp, &output)
        .map_err(|error| PackageError::context("activate built package", error))?;
    Ok(BuildResult {
        output_dir: output,
        manifest: bundle_manifest,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_into(
    app: &AppManifest,
    manifest_path: &Path,
    manifest_dir: &Path,
    workspace_root: &Path,
    output: &Path,
    run_mode: RunMode,
    namespace_profile: NamespaceProfile,
    browser_wasm: &Path,
    source_revision: &str,
) -> Result<BundleManifest, PackageError> {
    let mut files = Vec::new();
    let mut targets = BTreeSet::new();
    let document = compile_program(
        app,
        &app.programs.document,
        manifest_dir,
        workspace_root,
        namespace_profile,
    )?;
    let server = compile_program(
        app,
        &app.programs.server,
        manifest_dir,
        workspace_root,
        namespace_profile,
    )?;
    let document_descriptor = write_artifact(
        output,
        &app.programs.document,
        document,
        app.package.protocol_version,
        namespace_profile,
        &mut files,
        &mut targets,
    )?;
    let server_descriptor = write_artifact(
        output,
        &app.programs.server,
        server,
        app.package.protocol_version,
        namespace_profile,
        &mut files,
        &mut targets,
    )?;

    generate_browser_assets(
        app,
        browser_wasm,
        output,
        &document_descriptor,
        &mut files,
        &mut targets,
    )?;
    copy_declared_files(
        manifest_dir,
        workspace_root,
        output,
        &app.assets,
        BundleFileKind::Asset,
        true,
        &mut files,
        &mut targets,
    )?;
    copy_declared_files(
        manifest_dir,
        workspace_root,
        output,
        &app.fixtures,
        BundleFileKind::Fixture,
        false,
        &mut files,
        &mut targets,
    )?;
    copy_declared_files(
        manifest_dir,
        workspace_root,
        output,
        &app.migrations,
        BundleFileKind::Migration,
        false,
        &mut files,
        &mut targets,
    )?;
    copy_declared_files(
        manifest_dir,
        workspace_root,
        output,
        &app.scenarios,
        BundleFileKind::Scenario,
        false,
        &mut files,
        &mut targets,
    )?;
    copy_declared_files(
        manifest_dir,
        workspace_root,
        output,
        &app.budgets,
        BundleFileKind::Budget,
        false,
        &mut files,
        &mut targets,
    )?;
    copy_one_file(
        manifest_path,
        output,
        "package/app.toml",
        BundleFileKind::PackageManifest,
        false,
        StaticCachePolicy::Revalidate,
        &mut files,
        &mut targets,
    )?;

    files.sort_by(|left, right| left.path.cmp(&right.path));
    let bundle = BundleManifest {
        format: BUNDLE_FORMAT,
        package_id: app.package.id.clone(),
        package_version: app.package.version.clone(),
        deployment_domain: app.package.deployment_domain.clone(),
        source_revision: source_revision.to_owned(),
        run_mode,
        namespace_profile,
        protocol_version: app.package.protocol_version,
        artifacts: vec![document_descriptor, server_descriptor],
        files,
        browser: app.browser.clone(),
        http: app.http.clone(),
        environment: app.environment.clone(),
    };
    bundle.validate()?;
    let bundle_bytes = serde_json::to_vec_pretty(&bundle)?;
    fs::write(output.join(BUNDLE_MANIFEST_FILE), bundle_bytes)?;
    Ok(bundle)
}

struct CompiledProgram {
    artifact: ProgramArtifact,
    source_bundle_sha256: String,
}

fn compile_program(
    app: &AppManifest,
    program: &ProgramManifest,
    manifest_dir: &Path,
    workspace_root: &Path,
    namespace_profile: NamespaceProfile,
) -> Result<CompiledProgram, PackageError> {
    let mut source_by_declared_path = BTreeMap::new();
    let mut units = Vec::new();
    for declared in &program.sources {
        let source_path = resolve_workspace_file(manifest_dir, workspace_root, declared)?;
        let label = source_path
            .strip_prefix(workspace_root)
            .map_err(|_| PackageError::new("source path escaped workspace"))?
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&source_path)
            .map_err(|error| PackageError::context(&format!("read source `{declared}`"), error))?;
        source_by_declared_path.insert(declared.as_str(), label.clone());
        units.push(RuntimeSourceUnit {
            path: label,
            source,
        });
    }
    units.sort_by(|left, right| left.path.cmp(&right.path));
    let entry_path = source_by_declared_path
        .get(program.entry.as_str())
        .ok_or_else(|| PackageError::new("program entry was not resolved from sources"))?
        .clone();
    let source_bundle_sha256 = source_bundle_digest(&units);
    let request = ProgramCompileRequest {
        revision: PROGRAM_ARTIFACT_REVISION,
        entry_path,
        units,
        application: ApplicationIdentity::new(
            &app.package.id,
            program.namespace(namespace_profile),
            &app.package.deployment_domain,
        ),
        capability_profile: program.capability_profile,
    };
    let artifact = compile_trusted_package_program_artifact(&request)
        .map_err(|error| PackageError::context("compile trusted program artifact", error))?;
    if artifact.role() != program.role
        || artifact.capability_profile() != program.capability_profile
        || artifact.plan().target_profile != program.target_profile
    {
        return Err(PackageError::new(format!(
            "compiled {} artifact does not match the declared role/profile contract",
            program.role.as_str()
        )));
    }
    Ok(CompiledProgram {
        artifact,
        source_bundle_sha256,
    })
}

fn write_artifact(
    output: &Path,
    program: &ProgramManifest,
    compiled: CompiledProgram,
    protocol_version: u32,
    namespace_profile: NamespaceProfile,
    files: &mut Vec<BundleFileDescriptor>,
    targets: &mut BTreeSet<String>,
) -> Result<ArtifactDescriptor, PackageError> {
    validate_output_target(&program.artifact)?;
    if !targets.insert(program.artifact.clone()) {
        return Err(PackageError::new(format!(
            "package target `{}` is duplicated",
            program.artifact
        )));
    }
    let content = compiled.artifact.to_content_artifact();
    if content.bytes.is_empty() || content.bytes.len() > crate::MAX_ARTIFACT_BYTES {
        return Err(PackageError::new(format!(
            "compiled {} artifact exceeds its byte budget",
            program.role.as_str()
        )));
    }
    write_bytes(output, &program.artifact, &content.bytes)?;
    let bytes_sha256 = sha256_bytes(&content.bytes);
    files.push(BundleFileDescriptor {
        path: program.artifact.clone(),
        kind: BundleFileKind::ProgramArtifact,
        bytes_sha256: bytes_sha256.clone(),
        bytes_len: content.bytes.len(),
        public: program.role == ProgramRole::Document,
        cache: StaticCachePolicy::Immutable,
    });
    Ok(ArtifactDescriptor {
        role: program.role,
        path: program.artifact.clone(),
        revision: compiled.artifact.revision(),
        content_artifact_id: compiled.artifact.id_text(),
        content_media_type: content.media_type,
        bytes_sha256,
        bytes_len: content.bytes.len(),
        source_bundle_sha256: compiled.source_bundle_sha256,
        source_digest: compiled.artifact.source_digest().to_owned(),
        plan_digest: compiled.artifact.plan_digest().to_owned(),
        compiler_id: compiled.artifact.compiler_id().to_owned(),
        target_profile: compiled.artifact.plan().target_profile,
        capability_profile: compiled.artifact.capability_profile(),
        capability_profile_id: program.capability_profile_id.clone(),
        state_namespace: program.namespace(namespace_profile).to_owned(),
        protocol_version,
    })
}

fn generate_browser_assets(
    app: &AppManifest,
    browser_wasm: &Path,
    output: &Path,
    document: &ArtifactDescriptor,
    files: &mut Vec<BundleFileDescriptor>,
    targets: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    let browser_wasm = fs::canonicalize(browser_wasm)
        .map_err(|error| PackageError::context("canonicalize browser Wasm", error))?;
    let metadata = fs::symlink_metadata(&browser_wasm)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(PackageError::new(
            "browser Wasm must be a regular non-symlink file",
        ));
    }
    let bytes = fs::read(&browser_wasm)?;
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        return Err(PackageError::new(
            "browser Wasm input does not contain a WebAssembly module",
        ));
    }
    let mut bindgen = Bindgen::new();
    bindgen
        .input_bytes(&app.browser.wasm_output_name, bytes)
        .out_name(&app.browser.wasm_output_name)
        .typescript(false);
    bindgen
        .web(true)
        .map_err(|error| PackageError::context("configure browser Wasm bindings", error))?;
    bindgen
        .generate(output)
        .map_err(|error| PackageError::context("compile browser Wasm bindings", error))?;

    let app_config = BrowserAppConfig {
        format: APP_MANIFEST_FORMAT,
        package_id: &app.package.id,
        protocol_version: app.package.protocol_version,
        client_artifact_path: format!("/{}", document.path),
        client_artifact_id: &document.content_artifact_id,
        client_artifact_sha256: &document.bytes_sha256,
        canvas_id: &app.browser.canvas_id,
    };
    let app_config_bytes = serde_json::to_vec_pretty(&app_config)?;
    write_generated_public_file(
        output,
        "boon-app.json",
        &app_config_bytes,
        StaticCachePolicy::Revalidate,
        files,
        targets,
    )?;
    let index = browser_index(&app.browser.title, &app.browser.canvas_id);
    write_generated_public_file(
        output,
        "index.html",
        index.as_bytes(),
        StaticCachePolicy::Revalidate,
        files,
        targets,
    )?;
    write_generated_public_file(
        output,
        "boon-app-loader.js",
        browser_loader(&app.browser.wasm_output_name).as_bytes(),
        StaticCachePolicy::Revalidate,
        files,
        targets,
    )?;
    for generated in [
        format!("{}.js", app.browser.wasm_output_name),
        format!("{}_bg.wasm", app.browser.wasm_output_name),
    ] {
        let generated_path = output.join(&generated);
        let generated_bytes = fs::read(&generated_path).map_err(|error| {
            PackageError::context(
                &format!("read generated browser asset `{generated}`"),
                error,
            )
        })?;
        register_existing_public_file(
            &generated,
            &generated_bytes,
            StaticCachePolicy::Immutable,
            files,
            targets,
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct BrowserAppConfig<'a> {
    format: u32,
    package_id: &'a str,
    protocol_version: u32,
    client_artifact_path: String,
    client_artifact_id: &'a str,
    client_artifact_sha256: &'a str,
    canvas_id: &'a str,
}

#[allow(clippy::too_many_arguments)]
fn copy_declared_files(
    manifest_dir: &Path,
    workspace_root: &Path,
    output: &Path,
    declared: &[PackageFileManifest],
    kind: BundleFileKind,
    public: bool,
    files: &mut Vec<BundleFileDescriptor>,
    targets: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    for file in declared {
        let source = resolve_workspace_file(manifest_dir, workspace_root, &file.source)?;
        copy_one_file(
            &source,
            output,
            &file.target,
            kind,
            public,
            file.cache,
            files,
            targets,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn copy_one_file(
    source: &Path,
    output: &Path,
    target: &str,
    kind: BundleFileKind,
    public: bool,
    cache: StaticCachePolicy,
    files: &mut Vec<BundleFileDescriptor>,
    targets: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    validate_output_target(target)?;
    if !targets.insert(target.to_owned()) {
        return Err(PackageError::new(format!(
            "package target `{target}` is duplicated"
        )));
    }
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(PackageError::new(format!(
            "package source `{}` is not a regular non-symlink file",
            source.display()
        )));
    }
    if metadata.len() as usize > MAX_PACKAGE_FILE_BYTES {
        return Err(PackageError::new(format!(
            "package source `{}` exceeds {MAX_PACKAGE_FILE_BYTES} bytes",
            source.display()
        )));
    }
    let bytes = fs::read(source)?;
    write_bytes(output, target, &bytes)?;
    files.push(BundleFileDescriptor {
        path: target.to_owned(),
        kind,
        bytes_sha256: sha256_bytes(&bytes),
        bytes_len: bytes.len(),
        public,
        cache,
    });
    Ok(())
}

fn write_generated_public_file(
    output: &Path,
    target: &str,
    bytes: &[u8],
    cache: StaticCachePolicy,
    files: &mut Vec<BundleFileDescriptor>,
    targets: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    if !targets.insert(target.to_owned()) {
        return Err(PackageError::new(format!(
            "package target `{target}` is duplicated"
        )));
    }
    write_bytes(output, target, bytes)?;
    files.push(BundleFileDescriptor {
        path: target.to_owned(),
        kind: BundleFileKind::BrowserHost,
        bytes_sha256: sha256_bytes(bytes),
        bytes_len: bytes.len(),
        public: true,
        cache,
    });
    Ok(())
}

fn register_existing_public_file(
    target: &str,
    bytes: &[u8],
    cache: StaticCachePolicy,
    files: &mut Vec<BundleFileDescriptor>,
    targets: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    validate_output_target(target)?;
    if !targets.insert(target.to_owned()) {
        return Err(PackageError::new(format!(
            "package target `{target}` is duplicated"
        )));
    }
    files.push(BundleFileDescriptor {
        path: target.to_owned(),
        kind: BundleFileKind::BrowserHost,
        bytes_sha256: sha256_bytes(bytes),
        bytes_len: bytes.len(),
        public: true,
        cache,
    });
    Ok(())
}

fn resolve_workspace_file(
    manifest_dir: &Path,
    workspace_root: &Path,
    declared: &str,
) -> Result<PathBuf, PackageError> {
    let joined = manifest_dir.join(declared);
    let metadata = fs::symlink_metadata(&joined).map_err(|error| {
        PackageError::context(&format!("read package input `{declared}` metadata"), error)
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(PackageError::new(format!(
            "package input `{declared}` is not a regular non-symlink file"
        )));
    }
    let canonical = fs::canonicalize(&joined).map_err(|error| {
        PackageError::context(&format!("canonicalize package input `{declared}`"), error)
    })?;
    if !canonical.starts_with(workspace_root) {
        return Err(PackageError::new(format!(
            "package input `{declared}` escapes the Cargo workspace"
        )));
    }
    Ok(canonical)
}

fn find_workspace_root(start: &Path) -> Result<PathBuf, PackageError> {
    for candidate in start.ancestors() {
        let cargo = candidate.join("Cargo.toml");
        if let Ok(text) = fs::read_to_string(&cargo)
            && text.lines().any(|line| line.trim() == "[workspace]")
        {
            return fs::canonicalize(candidate).map_err(Into::into);
        }
    }
    Err(PackageError::new(
        "app manifest is not inside a Cargo workspace",
    ))
}

fn validate_build_request(request: &BuildRequest<'_>) -> Result<(), PackageError> {
    if request.source_revision.is_empty()
        || request.source_revision.trim() != request.source_revision
        || request.source_revision.len() > 256
        || !request
            .source_revision
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        return Err(PackageError::new(
            "source revision must be a bounded canonical build identity",
        ));
    }
    Ok(())
}

fn absolute_output_path(path: &Path) -> Result<PathBuf, PackageError> {
    if path.as_os_str().is_empty() {
        return Err(PackageError::new("package output path is empty"));
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn validate_output_target(target: &str) -> Result<(), PackageError> {
    if target.is_empty()
        || target.trim() != target
        || Path::new(target).components().any(|component| {
            !matches!(component, Component::Normal(_))
                || component.as_os_str().to_string_lossy().starts_with('.')
        })
    {
        return Err(PackageError::new(format!(
            "package target `{target}` is not a canonical relative output path"
        )));
    }
    Ok(())
}

fn write_bytes(output: &Path, target: &str, bytes: &[u8]) -> Result<(), PackageError> {
    validate_output_target(target)?;
    let path = output.join(target);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn source_bundle_digest(units: &[RuntimeSourceUnit]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"boon.source-bundle.v1");
    for unit in units {
        hasher.update((unit.path.len() as u64).to_be_bytes());
        hasher.update(unit.path.as_bytes());
        hasher.update((unit.source.len() as u64).to_be_bytes());
        hasher.update(unit.source.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn browser_index(title: &str, canvas_id: &str) -> String {
    let title = escape_html(title);
    let canvas_id = escape_html(canvas_id);
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1,viewport-fit=cover\">\
<meta name=\"color-scheme\" content=\"light dark\"><title>{title}</title>\
<style>html,body,main,canvas{{width:100%;height:100%;margin:0}}body{{overflow:hidden;background:#f4f6f5}}\
canvas{{display:block}}#boon-unsupported{{position:fixed;inset:0;display:none;place-items:center;padding:2rem;\
font:16px system-ui;background:#f4f6f5;color:#17221d}}</style></head>\
<body><main id=\"boon-root\"><canvas id=\"{canvas_id}\"></canvas>\
<div id=\"boon-unsupported\" role=\"alert\"></div></main>\
<script type=\"module\" src=\"/boon-app-loader.js\"></script></body></html>\n"
    )
}

fn browser_loader(wasm_output_name: &str) -> String {
    format!(
        "import init, * as host from '/{wasm_output_name}.js';\n\
const unsupported = document.getElementById('boon-unsupported');\n\
try {{\n  const app = await fetch('/boon-app.json', {{cache:'no-store'}}).then(r => {{\n\
    if (!r.ok) throw new Error(`package metadata ${{r.status}}`); return r.json(); }});\n\
  await init('/{wasm_output_name}_bg.wasm');\n\
  if (typeof host.start_boon_app !== 'function') throw new Error('browser host startup export is unavailable');\n\
  await host.start_boon_app(app);\n\
}} catch (error) {{ unsupported.textContent = `This Boon application cannot start: ${{error}}`; unsupported.style.display='grid'; }}\n"
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
