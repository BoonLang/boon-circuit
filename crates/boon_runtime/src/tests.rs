use super::*;

const CORE_COMPILED_ARTIFACT_EXAMPLES: [&str; 3] = ["counter", "todomvc", "cells"];

struct TestTempRoot {
    path: PathBuf,
}

impl TestTempRoot {
    fn new(label: &str) -> Self {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed");
        let path =
            std::env::temp_dir().join(format!("boon-{label}-{}-{thread_name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path.join(path)
    }
}

impl Drop for TestTempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn copy_dir_for_test(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir_for_test(&source, &target);
        } else {
            std::fs::copy(&source, &target).unwrap();
        }
    }
}

struct CompiledArtifactFixture {
    artifact: CompiledArtifact,
    compiled: CompiledProgram,
}

fn compiled_artifact_fixture(temp_root: &TestTempRoot, example: &str) -> CompiledArtifactFixture {
    let (source, _, _) = example_paths(example).unwrap();
    let artifact_path = temp_root.join(format!("{example}.boonc"));
    emit_compiled_artifact(&source, &artifact_path, None).unwrap();
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let parsed = parse_source_path_or_manifest_project(&source).unwrap();
    let ir = lower(&parsed).unwrap();
    let compiled = CompiledProgram::from_ir(&ir).unwrap();
    CompiledArtifactFixture { artifact, compiled }
}

fn for_core_compiled_artifacts(
    temp_root: &TestTempRoot,
    mut check: impl FnMut(&str, &CompiledArtifactFixture),
) {
    for example in CORE_COMPILED_ARTIFACT_EXAMPLES {
        let fixture = compiled_artifact_fixture(temp_root, example);
        check(example, &fixture);
    }
}

fn for_core_compiled_programs(mut check: impl FnMut(&str, &TypedProgram, &CompiledProgram)) {
    for example in CORE_COMPILED_ARTIFACT_EXAMPLES {
        let (source, _, _) = example_paths(example).unwrap();
        let parsed = parse_source_path_or_manifest_project(&source).unwrap();
        let ir = lower(&parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        check(example, &ir, &compiled);
    }
}

// Runtime test shards are grouped by behavior area while staying in this module for private invariant access.
include!("tests/cache_and_compiler_facade.rs");
include!("tests/cells.rs");
include!("tests/compiled_artifacts.rs");
include!("tests/counter.rs");
include!("tests/generic_runtime_core.rs");
include!("tests/physical_todomvc.rs");
include!("tests/reports_and_schema.rs");
include!("tests/todomvc.rs");
