use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod build_git;

const MIMALLOC_UPSTREAM_COMMIT: &str = "18b08671c9302247bfb682286e6bf3cc1773f801";

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("boon_cli must live at <workspace>/crates/boon_cli");

    emit_workspace_build_identity(workspace);
    emit_toolchain_identity();
    emit_profile_identity();

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        build_mimalloc(workspace);
    }
}

fn build_mimalloc(workspace: &Path) {
    let root = workspace.join("third_party/mimalloc-3.5.0");
    let source = root.join("src/static.c");
    let include = root.join("include");
    let source_include = root.join("src");
    assert!(source.is_file(), "missing vendored mimalloc static.c");

    let source_sha256 = tree_sha256(&root, &["LICENSE", "include", "src"]);
    println!("cargo:rustc-env=BOON_MIMALLOC_SOURCE_SHA256={source_sha256}");
    println!("cargo:rustc-env=BOON_MIMALLOC_UPSTREAM_COMMIT={MIMALLOC_UPSTREAM_COMMIT}");

    cc::Build::new()
        .file(source)
        .include(include)
        .include(source_include)
        .define("MI_STATIC_LIB", "1")
        .define("MI_BUILD_RELEASE", "1")
        .define("MI_CMAKE_BUILD_TYPE", "release")
        .flag_if_supported("-ftls-model=initial-exec")
        .pic(true)
        .opt_level(3)
        .warnings(false)
        .compile("boon_mimalloc_350");

    let archive =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("libboon_mimalloc_350.a");
    let archive_sha256 = file_sha256(&archive);
    println!("cargo:rustc-env=BOON_MIMALLOC_ARCHIVE_SHA256={archive_sha256}");
}

fn emit_toolchain_identity() {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| OsStr::new("rustc").to_owned());
    let output = Command::new(rustc)
        .arg("-Vv")
        .output()
        .expect("run rustc -Vv");
    assert!(output.status.success(), "rustc -Vv failed");
    let verbose = String::from_utf8(output.stdout).expect("rustc -Vv UTF-8");
    let field = |name: &str| {
        verbose
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
            .unwrap_or("unknown")
    };
    println!("cargo:rustc-env=BOON_RUSTC_RELEASE={}", field("release:"));
    println!(
        "cargo:rustc-env=BOON_RUSTC_COMMIT={}",
        field("commit-hash:")
    );
    println!(
        "cargo:rustc-env=BOON_RUSTC_COMMIT_DATE={}",
        field("commit-date:")
    );
    println!(
        "cargo:rustc-env=BOON_LLVM_VERSION={}",
        field("LLVM version:")
    );
}

fn emit_profile_identity() {
    let rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let target_cpu = target_cpu_from_rustflags(&rustflags).unwrap_or("generic");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    println!(
        "cargo:rustc-env=BOON_BUILD_PROFILE={}",
        profile
    );
    println!(
        "cargo:rustc-env=BOON_BUILD_OPT_LEVEL={}",
        env::var("OPT_LEVEL").unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "cargo:rustc-env=BOON_BUILD_DEBUG={}",
        env::var("DEBUG").unwrap_or_else(|_| "unknown".to_owned())
    );
    println!(
        "cargo:rustc-env=BOON_BUILD_TARGET={}",
        env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned())
    );
    println!("cargo:rustc-env=BOON_BUILD_TARGET_CPU={target_cpu}");
    println!("cargo:rustc-env=BOON_BUILD_RUSTFLAGS={rustflags}");
    // This is the repository's frozen release measurement lane, not a general
    // effective Cargo profile resolver. Do not attest release-only settings for
    // occasional, explicitly unscored debug observations.
    let settings = if profile == "release" {
        "lto=false;codegen-units=16;debug-assertions=off;overflow-checks=off"
    } else {
        "lto=unknown;codegen-units=unknown;debug-assertions=unknown;overflow-checks=unknown"
    };
    println!(
        "cargo:rustc-env=BOON_BUILD_PROFILE_OPTIONS=opt-level={};{settings}",
        env::var("OPT_LEVEL").unwrap_or_else(|_| "unknown".to_owned())
    );
}

fn target_cpu_from_rustflags(flags: &str) -> Option<&str> {
    let fields = flags.split('\u{1f}').collect::<Vec<_>>();
    for (index, field) in fields.iter().enumerate() {
        if let Some(cpu) = field.strip_prefix("-Ctarget-cpu=") {
            return Some(cpu);
        }
        if *field == "-C" {
            if let Some(cpu) = fields
                .get(index + 1)
                .and_then(|next| next.strip_prefix("target-cpu="))
            {
                return Some(cpu);
            }
        }
    }
    None
}

fn emit_workspace_build_identity(workspace: &Path) {
    // The v1 identity includes HEAD and staging state, not only source bytes.
    // A commit must refresh provenance even when it changes documentation only.
    for path in build_git::identity_watch_paths(workspace) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let head = git_stdout(workspace, &["rev-parse", "HEAD"]);
    let head = String::from_utf8(head).expect("git HEAD UTF-8");
    let head = head.trim();
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
    let diff = git_stdout(workspace, &diff_args);
    let mut untracked_args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    untracked_args.extend(input_paths);
    let untracked = git_stdout(workspace, &untracked_args);

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
        let relative = std::str::from_utf8(raw_path).expect("untracked path UTF-8");
        let path = workspace.join(relative);
        if path.is_file() {
            hasher.update(fs::read(path).expect("read untracked build input"));
        }
    }

    println!("cargo:rustc-env=BOON_BUILD_SOURCE_HEAD={head}");
    println!(
        "cargo:rustc-env=BOON_BUILD_SOURCE_WORKSPACE_SHA256={}",
        hex_digest(hasher.finalize())
    );
    println!(
        "cargo:rustc-env=BOON_BUILD_SOURCE_DIRTY={}",
        !diff.is_empty() || !untracked.is_empty()
    );

    let mut tracked_args = vec!["ls-files", "-z", "--"];
    tracked_args.extend(input_paths);
    let tracked = git_stdout(workspace, &tracked_args);
    for raw_path in tracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = std::str::from_utf8(raw_path).expect("tracked path UTF-8");
        println!(
            "cargo:rerun-if-changed={}",
            workspace.join(relative).display()
        );
    }
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join("crates").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join("third_party/mimalloc-3.5.0").display()
    );
}

fn git_stdout(workspace: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("run git for compiler build identity");
    assert!(output.status.success(), "git command {args:?} failed");
    output.stdout
}

fn tree_sha256(root: &Path, entries: &[&str]) -> String {
    let mut files = Vec::new();
    for entry in entries {
        collect_files(&root.join(entry), &mut files);
    }
    files.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"boon-vendored-tree-v1\0");
    for path in files {
        let relative = path.strip_prefix(root).expect("vendored relative path");
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        let bytes = fs::read(&path).expect("read vendored source");
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    hex_digest(hasher.finalize())
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("read vendored directory") {
            collect_files(&entry.expect("vendored directory entry").path(), files);
        }
    } else if path.is_file() {
        files.push(path.to_path_buf());
    }
}

fn file_sha256(path: &Path) -> String {
    hex_digest(Sha256::digest(
        fs::read(path).expect("read built mimalloc archive"),
    ))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(bytes.as_ref().len() * 2);
    for byte in bytes.as_ref() {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("write SHA-256 hex");
    }
    output
}
