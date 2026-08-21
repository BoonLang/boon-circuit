use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::report_v2::{ExpectedIdentity, ToolResult};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProducerSetIdentity {
    pub(crate) product: ProducerIdentity,
    pub(crate) evidence: ProducerIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProducerIdentity {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) metadata: ProducerMetadata,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProducerMetadata {
    pub(crate) product_kind: String,
    pub(crate) binary_path: String,
    pub(crate) binary_sha256: String,
    pub(crate) build_source_head: String,
    pub(crate) build_source_workspace_sha256: String,
    pub(crate) build_source_dirty: bool,
    pub(crate) build_input_scope: String,
    pub(crate) rustc_release: String,
    pub(crate) rustc_commit: String,
    pub(crate) rustc_commit_date: String,
    pub(crate) llvm_version: String,
    pub(crate) target_triple: String,
    pub(crate) target_cpu: String,
    pub(crate) cargo_profile: String,
    pub(crate) profile_options: String,
    pub(crate) rustflags: String,
    pub(crate) allocator: AllocatorMetadata,
    pub(crate) allocation_instrumentation: String,
    pub(crate) allocation_counter_scope: String,
    pub(crate) ld_preload: Option<String>,
    pub(crate) mimalloc_environment: Vec<EnvironmentValue>,
    pub(crate) transparent_hugepage_enabled: String,
    pub(crate) transparent_hugepage_defrag: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AllocatorMetadata {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) upstream_tag: Option<String>,
    pub(crate) upstream_commit: Option<String>,
    pub(crate) source_sha256: Option<String>,
    pub(crate) static_archive_sha256: Option<String>,
    pub(crate) linkage: String,
    pub(crate) build_options: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvironmentValue {
    pub(crate) name: String,
    pub(crate) value: String,
}

pub(crate) struct ExpectedProducer<'a> {
    pub(crate) path: &'a str,
    pub(crate) sha256: &'a str,
    pub(crate) product_kind: &'a str,
    pub(crate) allocator_id: &'a str,
    pub(crate) allocation_instrumentation: &'a str,
    pub(crate) allocation_counter_scope: &'a str,
    pub(crate) cargo_profile: &'a str,
    pub(crate) target_cpu: &'a str,
    pub(crate) profile_options: &'a str,
    pub(crate) rustflags: &'a str,
}

pub(crate) fn validate_producer_metadata(
    workspace: &Path,
    identity: &ExpectedIdentity,
    metadata: &ProducerMetadata,
    expected: ExpectedProducer<'_>,
) -> ToolResult<()> {
    let expected_binary = workspace.join(expected.path).canonicalize()?;
    if Path::new(&metadata.binary_path).canonicalize()? != expected_binary {
        return Err(format!(
            "producer reports binary path {}; expected {}",
            metadata.binary_path,
            expected_binary.display()
        )
        .into());
    }
    if metadata.binary_sha256 != expected.sha256 {
        return Err("producer self-reported binary hash differs from collector hash".into());
    }
    let build_input = current_producer_input_identity(workspace)?;
    if metadata.build_source_head != identity.source.head.as_str()
        || metadata.build_source_head != build_input.head
        || metadata.build_source_workspace_sha256 != build_input.sha256
        || metadata.build_source_dirty != build_input.dirty
        || metadata.build_input_scope
            != "workspace-cargo-toolchain-and-crates-excluding-xtask-plus-mimalloc-v1"
    {
        return Err(
            "producer build-time input identity differs from the live compiler input closure"
                .into(),
        );
    }
    if metadata.product_kind != expected.product_kind
        || metadata.allocator.id != expected.allocator_id
        || metadata.allocation_instrumentation != expected.allocation_instrumentation
        || metadata.allocation_counter_scope != expected.allocation_counter_scope
        || metadata.cargo_profile != expected.cargo_profile
        || metadata.target_cpu != expected.target_cpu
        || metadata.profile_options != expected.profile_options
        || metadata.rustflags != expected.rustflags
    {
        return Err(format!(
            "producer execution/build lane differs from manifest: kind={:?}, allocator={:?}, instrumentation={:?}/{:?}, profile={:?}, target_cpu={:?}, profile_options={:?}, rustflags={:?}",
            metadata.product_kind,
            metadata.allocator.id,
            metadata.allocation_instrumentation,
            metadata.allocation_counter_scope,
            metadata.cargo_profile,
            metadata.target_cpu,
            metadata.profile_options,
            metadata.rustflags,
        )
        .into());
    }
    validate_sha256(&metadata.binary_sha256, "producer binary")?;
    validate_sha256(
        &metadata.build_source_workspace_sha256,
        "producer build source",
    )?;
    validate_git_commit(&metadata.build_source_head, "producer build HEAD")?;
    validate_git_commit(&metadata.rustc_commit, "producer rustc commit")?;
    if metadata.rustc_release.is_empty()
        || metadata.rustc_commit_date.is_empty()
        || metadata.llvm_version.is_empty()
        || metadata.target_triple.is_empty()
        || metadata.transparent_hugepage_enabled.is_empty()
        || metadata.transparent_hugepage_defrag.is_empty()
    {
        return Err("producer omitted toolchain, target, or THP provenance".into());
    }
    if metadata
        .ld_preload
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().contains("mimalloc"))
    {
        return Err(
            "producer must not combine an integrated allocator with mimalloc preload".into(),
        );
    }
    if !metadata
        .mimalloc_environment
        .windows(2)
        .all(|pair| pair[0].name < pair[1].name)
        || metadata
            .mimalloc_environment
            .iter()
            .any(|entry| !entry.name.starts_with("MIMALLOC_"))
    {
        return Err("producer mimalloc environment is not canonical".into());
    }
    match metadata.allocator.id.as_str() {
        "system" => {
            if metadata.allocator.version != "platform-libc"
                || metadata.allocator.upstream_tag.is_some()
                || metadata.allocator.upstream_commit.is_some()
                || metadata.allocator.source_sha256.is_some()
                || metadata.allocator.static_archive_sha256.is_some()
                || metadata.allocator.linkage != "rust-system-global-allocator"
            {
                return Err("System allocator provenance is inconsistent".into());
            }
        }
        "microsoft-mimalloc" => {
            if metadata.allocator.version != "3.5.0"
                || metadata.allocator.upstream_tag.as_deref() != Some("v3.5.0")
                || metadata.allocator.upstream_commit.as_deref()
                    != Some("18b08671c9302247bfb682286e6bf3cc1773f801")
                || metadata.allocator.linkage != "static-rust-global-allocator-no-libc-override"
            {
                return Err("mimalloc provenance is not exact Microsoft 3.5.0".into());
            }
            validate_sha256(
                metadata
                    .allocator
                    .source_sha256
                    .as_deref()
                    .ok_or("mimalloc source hash is missing")?,
                "mimalloc source tree",
            )?;
            validate_sha256(
                metadata
                    .allocator
                    .static_archive_sha256
                    .as_deref()
                    .ok_or("mimalloc archive hash is missing")?,
                "mimalloc static archive",
            )?;
        }
        other => return Err(format!("unknown producer allocator `{other}`").into()),
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ProducerInputIdentity {
    workspace: PathBuf,
    head: String,
    sha256: String,
    dirty: bool,
}

fn current_producer_input_identity(workspace: &Path) -> ToolResult<&'static ProducerInputIdentity> {
    static IDENTITY: OnceLock<Result<ProducerInputIdentity, String>> = OnceLock::new();
    let identity = IDENTITY.get_or_init(|| {
        compute_producer_input_identity(workspace).map_err(|error| error.to_string())
    });
    let identity = identity
        .as_ref()
        .map_err(|error| -> Box<dyn std::error::Error> { error.clone().into() })?;
    if identity.workspace != workspace.canonicalize()? {
        return Err("one xtask process cannot validate producers from multiple workspaces".into());
    }
    Ok(identity)
}

fn compute_producer_input_identity(workspace: &Path) -> ToolResult<ProducerInputIdentity> {
    let workspace = workspace.canonicalize()?;
    let head = git_stdout(&workspace, &["rev-parse", "HEAD"])?;
    let head = String::from_utf8(head)?.trim().to_owned();
    let input_paths = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo",
        "crates",
        "third_party/mimalloc-3.5.0",
        ":(exclude)crates/xtask",
    ];
    let mut diff_args = vec!["diff", "--binary", "HEAD", "--"];
    diff_args.extend(input_paths);
    let diff = git_stdout(&workspace, &diff_args)?;
    let mut untracked_args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    untracked_args.extend(input_paths);
    let untracked = git_stdout(&workspace, &untracked_args)?;

    let mut hasher = Sha256::new();
    hasher.update(b"boon-producer-input-identity-v1\0");
    hasher.update(head.as_bytes());
    hasher.update([0]);
    hasher.update(&diff);
    for raw_path in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        hasher.update([0]);
        hasher.update(raw_path);
        hasher.update([0]);
        let relative = std::str::from_utf8(raw_path)?;
        let path = workspace.join(relative);
        if path.is_file() {
            hasher.update(fs::read(path)?);
        }
    }
    Ok(ProducerInputIdentity {
        workspace,
        head,
        sha256: hex_digest(hasher.finalize()),
        dirty: !diff.is_empty() || !untracked.is_empty(),
    })
}

fn git_stdout(workspace: &Path, args: &[&str]) -> ToolResult<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?} failed while computing producer input identity: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_sha256(value: &str, label: &str) -> ToolResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} SHA-256 is malformed").into());
    }
    Ok(())
}

fn validate_git_commit(value: &str, label: &str) -> ToolResult<()> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} is malformed").into());
    }
    Ok(())
}
