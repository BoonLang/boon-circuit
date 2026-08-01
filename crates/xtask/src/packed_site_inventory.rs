use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub(crate) const MANIFEST_PATH: &str = "docs/architecture/phase0/packed_site_inventory.toml";
pub(crate) const LEDGER_PATH: &str = "docs/architecture/phase0/packed_site_occurrences.tsv";
const LEDGER_PROTOCOL: &str = "boon-phase0-packed-site-occurrences-v1";
const LEDGER_HEADER: &str = "# path\tline\tbyte_column\tordinal\tprobe\tcategory\texecution_class\towner_phase\towner_plan\treplacement_phase\treason\tcontext_sha256";
const OWNER_PHASE: &str = "goal-phase-5-packed";
const OWNER_PLAN: &str = "docs/plans/BOON_PACKED_DATA_AND_DENSE_INTERNALS_PLAN.md";
const BASELINE_SOURCE_HEAD: &str = "4a820727d339038826a9d589c207ef5f973dad83";
const MAX_SOURCE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LEDGER_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) const CATEGORY_IDS: &[&str] = &[
    "recursive-value-carrier",
    "runtime-string-lookup",
    "recursive-clone",
    "allocation-or-growth",
    "whole-snapshot-or-comparison",
    "boundary-or-runtime-materialization",
];

pub(crate) const EXECUTION_CLASS_IDS: &[&str] = &["hot", "cold", "boundary"];

const SCAN_ROOTS: &[&str] = &[
    "crates/boon_compiler",
    "crates/boon_data",
    "crates/boon_distributed_runtime",
    "crates/boon_document/src/runtime.rs",
    "crates/boon_document_model",
    "crates/boon_host_runtime/src/effects.rs",
    "crates/boon_host_runtime/src/migration_scenario.rs",
    "crates/boon_host_runtime/src/persistent.rs",
    "crates/boon_host_runtime/src/persistent_program_session.rs",
    "crates/boon_ir",
    "crates/boon_list_access",
    "crates/boon_persistence",
    "crates/boon_plan",
    "crates/boon_plan_executor",
    "crates/boon_program_runtime",
    "crates/boon_runtime",
    "crates/boon_semantic/src/core_lowering.rs",
    "crates/boon_semantic/src/program_core.rs",
    "crates/boon_server_runtime",
    "crates/boon_typecheck",
    "crates/boon_web_host/src/web_persistent.rs",
    "crates/boon_wire",
];

const RUNTIME_ROOTS: &[&str] = &[
    "crates/boon_distributed_runtime",
    "crates/boon_document/src/runtime.rs",
    "crates/boon_host_runtime/src/effects.rs",
    "crates/boon_host_runtime/src/migration_scenario.rs",
    "crates/boon_host_runtime/src/persistent.rs",
    "crates/boon_host_runtime/src/persistent_program_session.rs",
    "crates/boon_list_access",
    "crates/boon_plan_executor",
    "crates/boon_program_runtime",
    "crates/boon_runtime",
    "crates/boon_server_runtime",
    "crates/boon_web_host/src/web_persistent.rs",
];

const HOT_ROOTS: &[&str] = &[
    "crates/boon_distributed_runtime/src",
    "crates/boon_document/src/runtime.rs",
    "crates/boon_host_runtime/src/effects.rs",
    "crates/boon_host_runtime/src/migration_scenario.rs",
    "crates/boon_host_runtime/src/persistent.rs",
    "crates/boon_host_runtime/src/persistent_program_session.rs",
    "crates/boon_list_access/src",
    "crates/boon_plan_executor/src",
    "crates/boon_program_runtime/src",
    "crates/boon_runtime/src",
    "crates/boon_server_runtime/src",
    "crates/boon_web_host/src/web_persistent.rs",
];

const BOUNDARY_ROOTS: &[&str] = &[
    "crates/boon_data",
    "crates/boon_document_model",
    "crates/boon_persistence",
    "crates/boon_wire",
];

const BOUNDARY_FILE_SUFFIXES: &[&str] = &[
    "/persistent.rs",
    "/web_persistent.rs",
    "/boon_distributed_runtime/src/message.rs",
    "/content_ref.rs",
];

const BOUNDARY_CONTEXT_NEEDLES: &[&str] = &[
    "archive",
    "boundary",
    "decode",
    "encode",
    "export",
    "import",
    "migration",
    "persist",
    "restore",
    "serialize",
    "snapshot",
    "wire",
];

const TEST_ATTRIBUTES: &[&[u8]] = &[
    b"#[cfg(test)]",
    b"#[test]",
    b"#[tokio::test]",
    b"#[wasm_bindgen_test]",
];

#[derive(Clone, Copy)]
enum ProbeScope {
    All,
    Runtime,
}

#[derive(Clone, Copy)]
struct Probe {
    id: &'static str,
    needle: &'static str,
    category: &'static str,
    scope: ProbeScope,
}

const PROBES: &[Probe] = &[
    Probe {
        id: "vec-value",
        needle: "Vec<Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-eval-value",
        needle: "Vec<EvalValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-stored-value",
        needle: "Vec<StoredValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-string-value",
        needle: "BTreeMap<String, Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-field-value",
        needle: "BTreeMap<FieldId, Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-string-eval-value",
        needle: "BTreeMap<String, EvalValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-field-eval-value",
        needle: "BTreeMap<FieldId, EvalValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "box-value",
        needle: "Box<Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "box-eval-value",
        needle: "Box<EvalValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "optional-eval-list",
        needle: "Option<Vec<EvalValue>>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "optional-value",
        needle: "Option<Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "optional-eval-value",
        needle: "Option<EvalValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "optional-stored-value",
        needle: "Option<StoredValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-string-stored-value",
        needle: "BTreeMap<String, StoredValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-distributed-argument-value",
        needle: "BTreeMap<DistributedArgumentId, Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-state-value",
        needle: "BTreeMap<StateId, Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-import-value",
        needle: "BTreeMap<ImportId, Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-value-target-value",
        needle: "BTreeMap<ValueTarget, Value>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-memory-leaf-stored-value",
        needle: "BTreeMap<MemoryLeafId, StoredValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "map-migration-input-stored-value",
        needle: "BTreeMap<MigrationInputId, StoredValue>",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "value-list-variant",
        needle: "Value::List",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "value-record-variant",
        needle: "Value::Record",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "eval-list-variant",
        needle: "EvalValue::List",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "eval-record-variant",
        needle: "EvalValue::Record",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "eval-mapped-row-variant",
        needle: "EvalValue::MappedRow",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "eval-tagged-variant",
        needle: "EvalValue::Tagged",
        category: "recursive-value-carrier",
        scope: ProbeScope::All,
    },
    Probe {
        id: "fields-get",
        needle: "fields.get(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "fields-remove",
        needle: "fields.remove(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "fields-get-mut",
        needle: "fields.get_mut(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "fields-contains-key",
        needle: "fields.contains_key(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "fields-entry",
        needle: "fields.entry(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "fields-get-key-value",
        needle: "fields.get_key_value(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "fields-remove-entry",
        needle: "fields.remove_entry(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-get",
        needle: "record.get(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-remove",
        needle: "record.remove(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-get-mut",
        needle: "record.get_mut(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-contains-key",
        needle: "record.contains_key(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-entry",
        needle: "record.entry(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-get-key-value",
        needle: "record.get_key_value(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "record-remove-entry",
        needle: "record.remove_entry(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "runtime-string-map-get",
        needle: "runtime_string_map_get(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "runtime-string-map-remove",
        needle: "runtime_string_map_remove(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "runtime-string-map-contains-key",
        needle: "runtime_string_map_contains_key(",
        category: "runtime-string-lookup",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "clone-call",
        needle: ".clone()",
        category: "recursive-clone",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "cloned-call",
        needle: ".cloned()",
        category: "recursive-clone",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "clone-from-call",
        needle: ".clone_from(",
        category: "recursive-clone",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "clone-tree-call",
        needle: "clone_tree(",
        category: "recursive-clone",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "clone-whole-list-snapshot-call",
        needle: "clone_whole_list_snapshot(",
        category: "recursive-clone",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "vec-new",
        needle: "Vec::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-with-capacity",
        needle: "Vec::with_capacity(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-deque-new",
        needle: "VecDeque::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-deque-with-capacity",
        needle: "VecDeque::with_capacity(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-from",
        needle: "Vec::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "string-new",
        needle: "String::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "string-with-capacity",
        needle: "String::with_capacity(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "string-from",
        needle: "String::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "string-from-utf8",
        needle: "String::from_utf8(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "string-from-utf8-lossy",
        needle: "String::from_utf8_lossy(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "box-new",
        needle: "Box::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "box-pin",
        needle: "Box::pin(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "rc-new",
        needle: "Rc::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "arc-new",
        needle: "Arc::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "arc-from",
        needle: "Arc::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "arc-make-mut",
        needle: "Arc::make_mut(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "btree-map-new",
        needle: "BTreeMap::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "btree-map-from",
        needle: "BTreeMap::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "btree-set-new",
        needle: "BTreeSet::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "btree-set-from",
        needle: "BTreeSet::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "hash-map-new",
        needle: "HashMap::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "hash-map-with-capacity",
        needle: "HashMap::with_capacity(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "hash-map-from",
        needle: "HashMap::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "hash-set-new",
        needle: "HashSet::new(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "hash-set-with-capacity",
        needle: "HashSet::with_capacity(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "hash-set-from",
        needle: "HashSet::from(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "vec-macro",
        needle: "vec![",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "push-call",
        needle: ".push(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "push-back-call",
        needle: ".push_back(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "insert-call",
        needle: ".insert(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "extend-call",
        needle: ".extend(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "extend-from-slice-call",
        needle: ".extend_from_slice(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "append-call",
        needle: ".append(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "splice-call",
        needle: ".splice(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "reserve-call",
        needle: ".reserve(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "reserve-exact-call",
        needle: ".reserve_exact(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "try-reserve-call",
        needle: ".try_reserve(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "resize-call",
        needle: ".resize(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "resize-with-call",
        needle: ".resize_with(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "to-owned-call",
        needle: ".to_owned(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "to-vec-call",
        needle: ".to_vec(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "to-string-call",
        needle: ".to_string(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "into-owned-call",
        needle: ".into_owned(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "repeat-call",
        needle: ".repeat(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "concat-call",
        needle: ".concat(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "join-call",
        needle: ".join(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "into-boxed-slice",
        needle: ".into_boxed_slice(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "collect-vec",
        needle: "collect::<Vec",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "collect-btree-map",
        needle: "collect::<BTreeMap",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "collect-btree-set",
        needle: "collect::<BTreeSet",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "collect-inferred",
        needle: ".collect(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "format-macro",
        needle: "format!(",
        category: "allocation-or-growth",
        scope: ProbeScope::All,
    },
    Probe {
        id: "snapshot-identifier",
        needle: "snapshot",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::All,
    },
    Probe {
        id: "old-items-comparison",
        needle: "old.as_ref() != Some(&items)",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "whole-list-snapshot-slot",
        needle: "items: Option<Vec<EvalValue>>",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "whole-list-snapshot-changed",
        needle: "whole_list_snapshot_changed(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "semantic-value-image",
        needle: "semantic_value_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "stored-value-image",
        needle: "stored_value_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "durable-restore-image",
        needle: "durable_restore_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "load-durable-image",
        needle: "load_durable_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "recovery-image",
        needle: "recovery_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "canonical-image",
        needle: "canonical_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "restore-image-method",
        needle: ".restore_image(",
        category: "whole-snapshot-or-comparison",
        scope: ProbeScope::Runtime,
    },
    Probe {
        id: "materialization-stem",
        needle: "materializ",
        category: "boundary-or-runtime-materialization",
        scope: ProbeScope::All,
    },
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryManifest {
    format_version: u16,
    baseline_source_head: String,
    baseline_scope: String,
    scan_roots: Vec<String>,
    runtime_roots: Vec<String>,
    hot_roots: Vec<String>,
    boundary_roots: Vec<String>,
    boundary_file_suffixes: Vec<String>,
    boundary_context_needles: Vec<String>,
    test_attributes: Vec<String>,
    extension: String,
    lexical_scope: String,
    workspace_file_policy: String,
    candidate_policy: String,
    execution_class_policy: String,
    occurrence_ledger_path: String,
    occurrence_ledger_protocol: String,
    occurrence_ledger_generator: String,
    occurrence_ledger_validator: String,
    expected_probe_set_sha256: String,
    expected_file_set_sha256: String,
    expected_scanned_files: usize,
    expected_occurrence_files: usize,
    expected_occurrence_ledger_sha256: String,
    expected_occurrence_rows: usize,
    categories: Vec<CategoryExpectation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CategoryExpectation {
    order: usize,
    id: String,
    owner_phase: String,
    owner_plan: String,
    expected_rows: usize,
    expected_hot: usize,
    expected_cold: usize,
    expected_boundary: usize,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Occurrence {
    path: String,
    line: usize,
    byte_column: usize,
    ordinal: usize,
    probe: String,
    category: String,
    execution_class: String,
    owner_phase: String,
    owner_plan: String,
    replacement_phase: String,
    reason: String,
    context_sha256: String,
}

#[derive(Clone, Debug)]
struct SourceFile {
    path: String,
    text: String,
    structural: Vec<u8>,
    line_starts: Vec<usize>,
    test_only_intervals: Vec<(usize, usize)>,
}

#[derive(Debug)]
pub struct InventorySummary {
    pub rows: usize,
    pub occurrence_files: usize,
    pub scanned_files: usize,
    pub ledger_sha256: String,
    pub file_set_sha256: String,
    pub probe_set_sha256: String,
    pub category_counts: BTreeMap<String, [usize; 3]>,
}

pub fn run_cli(workspace: &Path, args: &[String]) -> Result<(), String> {
    match args {
        [flag] if flag == "--generate" => {
            let summary = generate(workspace)?;
            print_summary(&summary);
            println!(
                "generation wrote {LEDGER_PATH}; review and update {MANIFEST_PATH}, then run --check"
            );
            Ok(())
        }
        [flag] if flag == "--check" => {
            let summary = validate(workspace)?;
            println!(
                "checked {LEDGER_PATH}: {} rows across {} occurrence files",
                summary.rows, summary.occurrence_files
            );
            Ok(())
        }
        _ => Err(
            "usage: cargo run -p xtask --example packed_site_inventory -- [--generate|--check]"
                .to_owned(),
        ),
    }
}

pub fn generate(workspace: &Path) -> Result<InventorySummary, String> {
    let (files, file_set_sha256) = load_source_files(workspace)?;
    let occurrences = scan(&files)?;
    let output = render_ledger(&occurrences);
    let ledger_path = workspace.join(LEDGER_PATH);
    let parent = ledger_path
        .parent()
        .ok_or_else(|| format!("{LEDGER_PATH} has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    let temporary =
        ledger_path.with_extension(format!("tsv.packed-site-generator-{}", std::process::id()));
    fs::write(&temporary, output.as_bytes())
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, &ledger_path)
        .map_err(|error| format!("{}: {error}", ledger_path.display()))?;
    Ok(summarize(
        &occurrences,
        files.len(),
        file_set_sha256,
        sha256_bytes(output.as_bytes()),
    ))
}

pub fn validate(workspace: &Path) -> Result<InventorySummary, String> {
    let manifest_path = workspace.join(MANIFEST_PATH);
    let manifest_bytes = read_bounded(&manifest_path, 512 * 1024)?;
    let manifest: InventoryManifest =
        toml::from_slice(&manifest_bytes).map_err(|error| format!("{MANIFEST_PATH}: {error}"))?;
    validate_manifest(workspace, &manifest)?;

    let (files, file_set_sha256) = load_source_files(workspace)?;
    if files.len() != manifest.expected_scanned_files {
        return Err(format!(
            "packed site inventory scans {} Rust files; manifest expects {}",
            files.len(),
            manifest.expected_scanned_files
        ));
    }
    if file_set_sha256 != manifest.expected_file_set_sha256 {
        return Err(format!(
            "packed site Rust file-set digest is {file_set_sha256}; expected {}",
            manifest.expected_file_set_sha256
        ));
    }
    let expected = scan(&files)?;
    let ledger_path = workspace.join(safe_relative(&manifest.occurrence_ledger_path)?);
    let ledger_bytes = read_bounded(&ledger_path, MAX_LEDGER_BYTES)?;
    let ledger_digest = sha256_bytes(&ledger_bytes);
    if ledger_digest != manifest.expected_occurrence_ledger_sha256 {
        return Err(format!(
            "packed site occurrence ledger digest is {ledger_digest}; expected {}",
            manifest.expected_occurrence_ledger_sha256
        ));
    }
    let actual = parse_ledger(&ledger_bytes)?;
    if actual.len() != manifest.expected_occurrence_rows {
        return Err(format!(
            "packed site occurrence ledger has {} rows; expected {}",
            actual.len(),
            manifest.expected_occurrence_rows
        ));
    }
    if actual != expected {
        let first = actual
            .iter()
            .zip(expected.iter())
            .position(|(actual, expected)| actual != expected)
            .unwrap_or(actual.len().min(expected.len()));
        return Err(format!(
            "packed site occurrence ledger is stale at sorted row {}; regenerate with `{}`",
            first + 1,
            manifest.occurrence_ledger_generator
        ));
    }

    let summary = summarize(&actual, files.len(), file_set_sha256, ledger_digest);
    validate_summary(&manifest, &summary)?;
    Ok(summary)
}

fn validate_manifest(workspace: &Path, manifest: &InventoryManifest) -> Result<(), String> {
    if manifest.format_version != 1 {
        return Err(format!(
            "{MANIFEST_PATH} format_version is {}; expected 1",
            manifest.format_version
        ));
    }
    require_exact(
        &manifest.baseline_source_head,
        BASELINE_SOURCE_HEAD,
        "baseline_source_head",
    )?;
    require_exact(
        &manifest.baseline_scope,
        "current-worktree",
        "baseline_scope",
    )?;
    let roots = SCAN_ROOTS
        .iter()
        .map(|root| (*root).to_owned())
        .collect::<Vec<_>>();
    if manifest.scan_roots != roots {
        return Err(format!(
            "packed site scan roots differ: actual={:?}, expected={roots:?}",
            manifest.scan_roots
        ));
    }
    require_string_list(&manifest.runtime_roots, RUNTIME_ROOTS, "runtime_roots")?;
    require_string_list(&manifest.hot_roots, HOT_ROOTS, "hot_roots")?;
    require_string_list(&manifest.boundary_roots, BOUNDARY_ROOTS, "boundary_roots")?;
    require_string_list(
        &manifest.boundary_file_suffixes,
        BOUNDARY_FILE_SUFFIXES,
        "boundary_file_suffixes",
    )?;
    require_string_list(
        &manifest.boundary_context_needles,
        BOUNDARY_CONTEXT_NEEDLES,
        "boundary_context_needles",
    )?;
    let test_attributes = TEST_ATTRIBUTES
        .iter()
        .map(|attribute| std::str::from_utf8(attribute).expect("test attribute is UTF-8"))
        .collect::<Vec<_>>();
    require_string_list(
        &manifest.test_attributes,
        &test_attributes,
        "test_attributes",
    )?;
    require_exact(&manifest.extension, "rs", "extension")?;
    require_exact(
        &manifest.lexical_scope,
        "rust-code-excluding-comments-and-literals",
        "lexical_scope",
    )?;
    require_exact(
        &manifest.workspace_file_policy,
        "git-cached-and-untracked-nonignored",
        "workspace_file_policy",
    )?;
    require_exact(
        &manifest.candidate_policy,
        "deterministic-conservative-syntactic-sites",
        "candidate_policy",
    )?;
    require_exact(
        &manifest.execution_class_policy,
        "test-scopes-cold-boundary-roots-or-context-boundary-runtime-roots-hot-otherwise-cold",
        "execution_class_policy",
    )?;
    require_exact(
        &manifest.occurrence_ledger_path,
        LEDGER_PATH,
        "occurrence_ledger_path",
    )?;
    require_exact(
        &manifest.occurrence_ledger_protocol,
        LEDGER_PROTOCOL,
        "occurrence_ledger_protocol",
    )?;
    require_exact(
        &manifest.occurrence_ledger_generator,
        "cargo run -p xtask --example packed_site_inventory -- --generate",
        "occurrence_ledger_generator",
    )?;
    require_exact(
        &manifest.occurrence_ledger_validator,
        "cargo run -p xtask --example packed_site_inventory -- --check",
        "occurrence_ledger_validator",
    )?;
    require_lower_hex(
        &manifest.expected_probe_set_sha256,
        64,
        "expected_probe_set_sha256",
    )?;
    require_lower_hex(
        &manifest.expected_file_set_sha256,
        64,
        "expected_file_set_sha256",
    )?;
    require_lower_hex(
        &manifest.expected_occurrence_ledger_sha256,
        64,
        "expected_occurrence_ledger_sha256",
    )?;
    let probe_digest = probe_set_sha256();
    if manifest.expected_probe_set_sha256 != probe_digest {
        return Err(format!(
            "packed site probe-set digest is {probe_digest}; expected {}",
            manifest.expected_probe_set_sha256
        ));
    }
    if manifest.expected_scanned_files == 0
        || manifest.expected_occurrence_files == 0
        || manifest.expected_occurrence_rows == 0
    {
        return Err("packed site manifest contains a zero inventory cardinality".to_owned());
    }
    let ids = manifest
        .categories
        .iter()
        .map(|category| category.id.as_str())
        .collect::<Vec<_>>();
    if ids != CATEGORY_IDS {
        return Err(format!(
            "packed site category order differs: actual={ids:?}, expected={CATEGORY_IDS:?}"
        ));
    }
    for (order, category) in manifest.categories.iter().enumerate() {
        if category.order != order {
            return Err(format!(
                "packed site category {} order is {}; expected {order}",
                category.id, category.order
            ));
        }
        require_exact(
            &category.owner_phase,
            OWNER_PHASE,
            &format!("category {} owner_phase", category.id),
        )?;
        require_exact(
            &category.owner_plan,
            OWNER_PLAN,
            &format!("category {} owner_plan", category.id),
        )?;
        if !workspace.join(&category.owner_plan).is_file() {
            return Err(format!(
                "category {} owner plan {} is not a workspace file",
                category.id, category.owner_plan
            ));
        }
        if category.expected_rows
            != category
                .expected_hot
                .saturating_add(category.expected_cold)
                .saturating_add(category.expected_boundary)
        {
            return Err(format!(
                "category {} row total does not equal hot+cold+boundary",
                category.id
            ));
        }
    }
    Ok(())
}

fn validate_summary(
    manifest: &InventoryManifest,
    summary: &InventorySummary,
) -> Result<(), String> {
    if summary.occurrence_files != manifest.expected_occurrence_files {
        return Err(format!(
            "packed site inventory has {} occurrence files; expected {}",
            summary.occurrence_files, manifest.expected_occurrence_files
        ));
    }
    for category in &manifest.categories {
        let counts = summary
            .category_counts
            .get(&category.id)
            .copied()
            .unwrap_or_default();
        let total = counts.iter().sum::<usize>();
        if total != category.expected_rows
            || counts[0] != category.expected_hot
            || counts[1] != category.expected_cold
            || counts[2] != category.expected_boundary
        {
            return Err(format!(
                "category {} counts are rows={total}, hot={}, cold={}, boundary={}; expected rows={}, hot={}, cold={}, boundary={}",
                category.id,
                counts[0],
                counts[1],
                counts[2],
                category.expected_rows,
                category.expected_hot,
                category.expected_cold,
                category.expected_boundary
            ));
        }
    }
    Ok(())
}

fn load_source_files(workspace: &Path) -> Result<(Vec<SourceFile>, String), String> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "crates",
        ])
        .output()
        .map_err(|error| format!("git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_owned)
                .map_err(|error| format!("git path is not UTF-8: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        workspace.join(path).is_file()
            && path.ends_with(".rs")
            && SCAN_ROOTS
                .iter()
                .any(|root| path == root || path.starts_with(&format!("{root}/")))
    });
    paths.sort();
    paths.dedup();
    let file_set = paths.join("\n") + "\n";
    let file_set_sha256 = sha256_bytes(file_set.as_bytes());
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = read_bounded(&workspace.join(&path), MAX_SOURCE_BYTES)?;
        let text = String::from_utf8(bytes).map_err(|error| format!("{path}: {error}"))?;
        let structural = rust_structural_bytes(&text);
        let line_starts = source_line_starts(&text);
        let test_only_intervals = test_only_intervals(&structural);
        files.push(SourceFile {
            path,
            text,
            structural,
            line_starts,
            test_only_intervals,
        });
    }
    Ok((files, file_set_sha256))
}

fn scan(files: &[SourceFile]) -> Result<Vec<Occurrence>, String> {
    let mut occurrences = Vec::new();
    for file in files {
        let mut ordinal_by_probe = BTreeMap::<&str, usize>::new();
        for probe in PROBES {
            if matches!(probe.scope, ProbeScope::Runtime)
                && !RUNTIME_ROOTS
                    .iter()
                    .any(|root| path_matches(&file.path, root))
            {
                continue;
            }
            let needle = probe.needle.as_bytes();
            if needle.is_empty() {
                return Err(format!("probe {} has an empty needle", probe.id));
            }
            let mut offset = 0;
            while offset + needle.len() <= file.structural.len() {
                let Some(relative) = file.structural[offset..]
                    .windows(needle.len())
                    .position(|window| window == needle)
                else {
                    break;
                };
                let source_offset = offset + relative;
                let ordinal = ordinal_by_probe.entry(probe.id).or_default();
                *ordinal += 1;
                let (line, byte_column) = line_and_column(&file.line_starts, source_offset)?;
                let execution_class = classify_execution(file, source_offset, line, probe.category);
                let replacement_phase = replacement_phase(probe.category, execution_class);
                let reason = reason(probe.category, execution_class);
                let raw_line = source_line(&file.text, &file.line_starts, line)?;
                occurrences.push(Occurrence {
                    path: file.path.clone(),
                    line,
                    byte_column,
                    ordinal: *ordinal,
                    probe: probe.id.to_owned(),
                    category: probe.category.to_owned(),
                    execution_class: execution_class.to_owned(),
                    owner_phase: OWNER_PHASE.to_owned(),
                    owner_plan: OWNER_PLAN.to_owned(),
                    replacement_phase: replacement_phase.to_owned(),
                    reason: reason.to_owned(),
                    context_sha256: sha256_bytes(raw_line.as_bytes()),
                });
                offset = source_offset + needle.len();
            }
        }
    }
    occurrences.sort();
    let mut identities = BTreeSet::new();
    for occurrence in &occurrences {
        let identity = (
            occurrence.path.as_str(),
            occurrence.line,
            occurrence.byte_column,
            occurrence.probe.as_str(),
        );
        if !identities.insert(identity) {
            return Err(format!(
                "duplicate packed site occurrence {}:{}:{} {}",
                occurrence.path, occurrence.line, occurrence.byte_column, occurrence.probe
            ));
        }
    }
    for category in CATEGORY_IDS {
        if !occurrences
            .iter()
            .any(|occurrence| occurrence.category == *category)
        {
            return Err(format!(
                "packed site category {category} has no occurrences"
            ));
        }
    }
    Ok(occurrences)
}

fn classify_execution(
    file: &SourceFile,
    source_offset: usize,
    line: usize,
    category: &str,
) -> &'static str {
    if is_test_only(file, source_offset) {
        return "cold";
    }
    if BOUNDARY_ROOTS
        .iter()
        .any(|root| path_matches(&file.path, root))
        || BOUNDARY_FILE_SUFFIXES
            .iter()
            .any(|suffix| file.path.ends_with(suffix))
    {
        return "boundary";
    }
    let context = context_lines(file, line).to_ascii_lowercase();
    if category == "boundary-or-runtime-materialization"
        && contains_any(&context, BOUNDARY_CONTEXT_NEEDLES)
    {
        return "boundary";
    }
    if HOT_ROOTS.iter().any(|root| path_matches(&file.path, root)) {
        "hot"
    } else {
        "cold"
    }
}

fn replacement_phase(category: &str, execution_class: &str) -> &'static str {
    if execution_class == "cold" {
        return "packed-phase-9-flag-day-audit";
    }
    if execution_class == "boundary" {
        return "packed-phase-8-boundaries-and-targets";
    }
    match category {
        "recursive-value-carrier" | "runtime-string-lookup" | "recursive-clone" => {
            "packed-phase-1-dense-semantic-artifacts"
        }
        "allocation-or-growth" | "whole-snapshot-or-comparison" => {
            "packed-phase-3-dense-scalar-runtime"
        }
        "boundary-or-runtime-materialization" => "packed-phase-8-boundaries-and-targets",
        _ => "packed-phase-9-flag-day-audit",
    }
}

fn reason(category: &str, execution_class: &str) -> &'static str {
    match (category, execution_class) {
        ("recursive-value-carrier", "hot") => "recursive-value-in-hot-execution-radius",
        ("recursive-value-carrier", "cold") => "recursive-value-in-cold-construction-or-test",
        ("recursive-value-carrier", "boundary") => "recursive-value-at-canonical-boundary",
        ("runtime-string-lookup", "hot") => "candidate-string-field-lookup-in-hot-runtime",
        ("runtime-string-lookup", "cold") => "candidate-string-field-lookup-in-runtime-test",
        ("runtime-string-lookup", "boundary") => "candidate-string-field-lookup-at-boundary",
        ("recursive-clone", "hot") => "candidate-recursive-clone-in-hot-runtime",
        ("recursive-clone", "cold") => "candidate-recursive-clone-in-runtime-test",
        ("recursive-clone", "boundary") => "candidate-recursive-clone-at-runtime-boundary",
        ("allocation-or-growth", "hot") => "candidate-allocation-or-growth-in-hot-execution",
        ("allocation-or-growth", "cold") => "candidate-allocation-or-growth-in-cold-or-test",
        ("allocation-or-growth", "boundary") => "candidate-allocation-or-growth-at-boundary",
        ("whole-snapshot-or-comparison", "hot") => {
            "candidate-whole-snapshot-or-comparison-in-hot-runtime"
        }
        ("whole-snapshot-or-comparison", "cold") => {
            "candidate-whole-snapshot-or-comparison-in-cold-or-test"
        }
        ("whole-snapshot-or-comparison", "boundary") => {
            "candidate-whole-snapshot-or-comparison-at-boundary"
        }
        ("boundary-or-runtime-materialization", "hot") => {
            "materialization-in-hot-runtime-execution-radius"
        }
        ("boundary-or-runtime-materialization", "cold") => {
            "materialization-in-cold-construction-or-test"
        }
        ("boundary-or-runtime-materialization", "boundary") => {
            "materialization-at-canonical-boundary"
        }
        _ => "unclassified-packed-site",
    }
}

fn render_ledger(occurrences: &[Occurrence]) -> String {
    let mut output = format!("# {LEDGER_PROTOCOL}\n{LEDGER_HEADER}\n");
    for occurrence in occurrences {
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            occurrence.path,
            occurrence.line,
            occurrence.byte_column,
            occurrence.ordinal,
            occurrence.probe,
            occurrence.category,
            occurrence.execution_class,
            occurrence.owner_phase,
            occurrence.owner_plan,
            occurrence.replacement_phase,
            occurrence.reason,
            occurrence.context_sha256
        ));
    }
    output
}

fn parse_ledger(bytes: &[u8]) -> Result<Vec<Occurrence>, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| format!("{LEDGER_PATH}: {error}"))?;
    let mut lines = text.lines();
    let protocol = format!("# {LEDGER_PROTOCOL}");
    if lines.next() != Some(protocol.as_str()) {
        return Err("packed site ledger has the wrong protocol header".to_owned());
    }
    if lines.next() != Some(LEDGER_HEADER) {
        return Err("packed site ledger has the wrong column header".to_owned());
    }
    let probe_ids = PROBES.iter().map(|probe| probe.id).collect::<BTreeSet<_>>();
    let mut occurrences = Vec::new();
    let mut identities = BTreeSet::new();
    for (index, row) in lines.enumerate() {
        if row.is_empty() {
            return Err(format!("packed site ledger row {} is blank", index + 3));
        }
        let columns = row.split('\t').collect::<Vec<_>>();
        if columns.len() != 12 {
            return Err(format!(
                "packed site ledger row {} has {} columns; expected 12",
                index + 3,
                columns.len()
            ));
        }
        safe_relative(columns[0])?;
        if !columns[0].ends_with(".rs")
            || !SCAN_ROOTS.iter().any(|root| path_matches(columns[0], root))
        {
            return Err(format!(
                "packed site ledger row {} path {} is outside the Rust scan roots",
                index + 3,
                columns[0]
            ));
        }
        let line = parse_positive(columns[1], "line")?;
        let byte_column = parse_positive(columns[2], "byte_column")?;
        let ordinal = parse_positive(columns[3], "ordinal")?;
        if !probe_ids.contains(columns[4]) {
            return Err(format!(
                "packed site ledger row {} has unknown probe {}",
                index + 3,
                columns[4]
            ));
        }
        if !CATEGORY_IDS.contains(&columns[5]) {
            return Err(format!(
                "packed site ledger row {} has unknown category {}",
                index + 3,
                columns[5]
            ));
        }
        if !EXECUTION_CLASS_IDS.contains(&columns[6]) {
            return Err(format!(
                "packed site ledger row {} has unknown execution class {}",
                index + 3,
                columns[6]
            ));
        }
        require_exact(columns[7], OWNER_PHASE, "ledger owner_phase")?;
        require_exact(columns[8], OWNER_PLAN, "ledger owner_plan")?;
        require_bounded_token(columns[9], 96, "replacement_phase")?;
        require_bounded_token(columns[10], 128, "reason")?;
        require_lower_hex(columns[11], 64, "context_sha256")?;
        let occurrence = Occurrence {
            path: columns[0].to_owned(),
            line,
            byte_column,
            ordinal,
            probe: columns[4].to_owned(),
            category: columns[5].to_owned(),
            execution_class: columns[6].to_owned(),
            owner_phase: columns[7].to_owned(),
            owner_plan: columns[8].to_owned(),
            replacement_phase: columns[9].to_owned(),
            reason: columns[10].to_owned(),
            context_sha256: columns[11].to_owned(),
        };
        let identity = (
            occurrence.path.clone(),
            occurrence.line,
            occurrence.byte_column,
            occurrence.probe.clone(),
        );
        if !identities.insert(identity) {
            return Err(format!(
                "packed site ledger duplicates {}:{}:{} {}",
                occurrence.path, occurrence.line, occurrence.byte_column, occurrence.probe
            ));
        }
        occurrences.push(occurrence);
    }
    let mut sorted = occurrences.clone();
    sorted.sort();
    if sorted != occurrences {
        return Err("packed site ledger rows are not in canonical order".to_owned());
    }
    Ok(occurrences)
}

fn summarize(
    occurrences: &[Occurrence],
    scanned_files: usize,
    file_set_sha256: String,
    ledger_sha256: String,
) -> InventorySummary {
    let occurrence_files = occurrences
        .iter()
        .map(|occurrence| occurrence.path.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let mut category_counts = CATEGORY_IDS
        .iter()
        .map(|category| ((*category).to_owned(), [0; 3]))
        .collect::<BTreeMap<_, _>>();
    for occurrence in occurrences {
        let index = match occurrence.execution_class.as_str() {
            "hot" => 0,
            "cold" => 1,
            "boundary" => 2,
            _ => continue,
        };
        category_counts
            .entry(occurrence.category.clone())
            .or_default()[index] += 1;
    }
    InventorySummary {
        rows: occurrences.len(),
        occurrence_files,
        scanned_files,
        ledger_sha256,
        file_set_sha256,
        probe_set_sha256: probe_set_sha256(),
        category_counts,
    }
}

fn print_summary(summary: &InventorySummary) {
    println!("expected_probe_set_sha256 = {:?}", summary.probe_set_sha256);
    println!("expected_file_set_sha256 = {:?}", summary.file_set_sha256);
    println!("expected_scanned_files = {}", summary.scanned_files);
    println!("expected_occurrence_files = {}", summary.occurrence_files);
    println!(
        "expected_occurrence_ledger_sha256 = {:?}",
        summary.ledger_sha256
    );
    println!("expected_occurrence_rows = {}", summary.rows);
    for (order, category) in CATEGORY_IDS.iter().enumerate() {
        let counts = summary
            .category_counts
            .get(*category)
            .copied()
            .unwrap_or_default();
        println!();
        println!("[[categories]]");
        println!("order = {order}");
        println!("id = {category:?}");
        println!("owner_phase = {OWNER_PHASE:?}");
        println!("owner_plan = {OWNER_PLAN:?}");
        println!("expected_rows = {}", counts.iter().sum::<usize>());
        println!("expected_hot = {}", counts[0]);
        println!("expected_cold = {}", counts[1]);
        println!("expected_boundary = {}", counts[2]);
    }
}

fn probe_set_sha256() -> String {
    let mut canonical = String::new();
    for root in SCAN_ROOTS {
        canonical.push_str("scan-root\t");
        canonical.push_str(root);
        canonical.push('\n');
    }
    for root in RUNTIME_ROOTS {
        canonical.push_str("runtime-root\t");
        canonical.push_str(root);
        canonical.push('\n');
    }
    for root in HOT_ROOTS {
        canonical.push_str("hot-root\t");
        canonical.push_str(root);
        canonical.push('\n');
    }
    for root in BOUNDARY_ROOTS {
        canonical.push_str("boundary-root\t");
        canonical.push_str(root);
        canonical.push('\n');
    }
    for suffix in BOUNDARY_FILE_SUFFIXES {
        canonical.push_str("boundary-file-suffix\t");
        canonical.push_str(suffix);
        canonical.push('\n');
    }
    for needle in BOUNDARY_CONTEXT_NEEDLES {
        canonical.push_str("boundary-context-needle\t");
        canonical.push_str(needle);
        canonical.push('\n');
    }
    for attribute in TEST_ATTRIBUTES {
        canonical.push_str("test-attribute\t");
        canonical.push_str(std::str::from_utf8(attribute).expect("test attribute is UTF-8"));
        canonical.push('\n');
    }
    for probe in PROBES {
        canonical.push_str(probe.id);
        canonical.push('\t');
        canonical.push_str(probe.needle);
        canonical.push('\t');
        canonical.push_str(probe.category);
        canonical.push('\t');
        canonical.push_str(match probe.scope {
            ProbeScope::All => "all",
            ProbeScope::Runtime => "runtime",
        });
        canonical.push('\n');
    }
    sha256_bytes(canonical.as_bytes())
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(format!(
            "{} must be a regular file of 1..={maximum} bytes",
            path.display()
        ));
    }
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn safe_relative(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(format!("path {value:?} is not a safe relative path"))
    } else {
        Ok(path.to_path_buf())
    }
}

fn path_matches(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
}

fn line_and_column(line_starts: &[usize], offset: usize) -> Result<(usize, usize), String> {
    let line_index = match line_starts.binary_search(&offset) {
        Ok(index) => index,
        Err(0) => return Err(format!("source offset {offset} precedes line zero")),
        Err(index) => index - 1,
    };
    Ok((line_index + 1, offset - line_starts[line_index] + 1))
}

fn source_line<'a>(text: &'a str, line_starts: &[usize], line: usize) -> Result<&'a str, String> {
    let start = *line_starts
        .get(line - 1)
        .ok_or_else(|| format!("source line {line} does not exist"))?;
    let end = line_starts.get(line).copied().unwrap_or(text.len());
    let line = &text[start..end];
    let line = line.strip_suffix('\n').unwrap_or(line);
    Ok(line.strip_suffix('\r').unwrap_or(line))
}

fn source_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' && index + 1 < text.len() {
            starts.push(index + 1);
        }
    }
    starts
}

fn context_lines(file: &SourceFile, line: usize) -> String {
    let start_line = line.saturating_sub(2).max(1);
    let end_line = (line + 2).min(file.line_starts.len());
    let mut output = String::new();
    for current in start_line..=end_line {
        if let Ok(source) = source_line(&file.text, &file.line_starts, current) {
            output.push_str(source);
            output.push(' ');
        }
    }
    output
}

fn is_test_only(file: &SourceFile, offset: usize) -> bool {
    file.path.contains("/tests/")
        || file.path.ends_with("/tests.rs")
        || file
            .test_only_intervals
            .iter()
            .any(|(start, end)| offset >= *start && offset < *end)
}

fn test_only_intervals(structural: &[u8]) -> Vec<(usize, usize)> {
    let mut intervals = Vec::new();
    for attribute in TEST_ATTRIBUTES {
        let mut offset = 0;
        while offset + attribute.len() <= structural.len() {
            let Some(relative) = structural[offset..]
                .windows(attribute.len())
                .position(|window| window == *attribute)
            else {
                break;
            };
            let start = offset + relative;
            intervals.push((
                start,
                test_only_item_end(structural, start + attribute.len()),
            ));
            offset = start + attribute.len();
        }
    }
    intervals.sort();
    merge_intervals(intervals)
}

fn merge_intervals(intervals: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for interval in intervals {
        if let Some(previous) = merged.last_mut()
            && interval.0 <= previous.1
        {
            previous.1 = previous.1.max(interval.1);
            continue;
        }
        merged.push(interval);
    }
    merged
}

fn test_only_item_end(structural: &[u8], start: usize) -> usize {
    let mut parentheses = 0_usize;
    let mut brackets = 0_usize;
    let mut angles = 0_usize;
    let mut index = start;
    while index < structural.len() {
        match structural[index] {
            b'(' => parentheses += 1,
            b')' => parentheses = parentheses.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'<' if parentheses == 0 && brackets == 0 => angles += 1,
            b'>' if parentheses == 0 && brackets == 0 => angles = angles.saturating_sub(1),
            b'{' if parentheses == 0 && brackets == 0 && angles == 0 => {
                return matching_brace(structural, index).map_or(structural.len(), |end| end + 1);
            }
            b';' | b',' if parentheses == 0 && brackets == 0 && angles == 0 => {
                return index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    structural.len()
}

fn matching_brace(structural: &[u8], opening: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (relative, byte) in structural[opening..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(opening + relative);
                }
            }
            _ => {}
        }
    }
    None
}

fn rust_structural_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut structural = bytes.to_vec();
    let mut index = 0_usize;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            let end = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |relative| index + relative);
            blank_non_newlines(&mut structural[index..end]);
            index = end;
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            let start = index;
            index += 2;
            let mut depth = 1_usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            blank_non_newlines(&mut structural[start..index]);
            continue;
        }
        if bytes[index] == b'r' {
            let mut quote = index + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if quote < bytes.len() && bytes[quote] == b'"' {
                let hashes = quote - index - 1;
                let start = index;
                index = quote + 1;
                while index < bytes.len() {
                    if bytes[index] == b'"'
                        && index + 1 + hashes <= bytes.len()
                        && bytes[index + 1..index + 1 + hashes]
                            .iter()
                            .all(|byte| *byte == b'#')
                    {
                        index += 1 + hashes;
                        break;
                    }
                    index += 1;
                }
                blank_non_newlines(&mut structural[start..index]);
                continue;
            }
        }
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            blank_non_newlines(&mut structural[start..index]);
            continue;
        }
        if bytes[index] == b'\'' {
            let simple_ascii = index + 2 < bytes.len() && bytes[index + 2] == b'\'';
            let escaped_or_unicode = index + 1 < bytes.len()
                && (bytes[index + 1] == b'\\' || !bytes[index + 1].is_ascii());
            let end = (simple_ascii || escaped_or_unicode)
                .then(|| {
                    bytes[index + 1..]
                        .iter()
                        .position(|byte| *byte == b'\'')
                        .map(|relative| index + relative + 2)
                })
                .flatten();
            if let Some(end) = end.filter(|end| *end - index <= 16) {
                blank_non_newlines(&mut structural[index..end]);
                index = end;
                continue;
            }
        }
        index += 1;
    }
    structural
}

fn blank_non_newlines(bytes: &mut [u8]) {
    for byte in bytes {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse_positive(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("{label} {value:?}: {error}"))?;
    if parsed == 0 {
        Err(format!("{label} must be positive"))
    } else {
        Ok(parsed)
    }
}

fn require_exact(actual: &str, expected: &str, label: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label} is {actual:?}; expected {expected:?}"))
    }
}

fn require_string_list(actual: &[String], expected: &[&str], label: &str) -> Result<(), String> {
    if actual
        .iter()
        .map(String::as_str)
        .eq(expected.iter().copied())
    {
        Ok(())
    } else {
        Err(format!("{label} is {actual:?}; expected {expected:?}"))
    }
}

fn require_lower_hex(value: &str, length: usize, label: &str) -> Result<(), String> {
    if value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(format!(
            "{label} must be {length} lowercase hexadecimal characters"
        ))
    }
}

fn require_bounded_token(value: &str, maximum: usize, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > maximum
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    {
        Err(format!(
            "{label} is not a bounded lowercase token: {value:?}"
        ))
    } else {
        Ok(())
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_scan_excludes_comments_and_literals() {
        let source = r##"
            let live = Vec::new();
            // let commented = Vec::new();
            let literal = "Vec::new()";
            let raw = r#"Vec::new()"#;
        "##;
        let structural = rust_structural_bytes(source);
        assert_eq!(
            structural
                .windows(b"Vec::new(".len())
                .filter(|window| *window == b"Vec::new(")
                .count(),
            1
        );
    }

    #[test]
    fn probe_registry_has_exact_categories_and_unique_ids() {
        let ids = PROBES.iter().map(|probe| probe.id).collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), PROBES.len());
        let categories = PROBES
            .iter()
            .map(|probe| probe.category)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            categories,
            CATEGORY_IDS.iter().copied().collect::<BTreeSet<_>>()
        );
    }

    #[test]
    fn ledger_parser_rejects_unknown_classification() {
        let occurrence = Occurrence {
            path: "crates/boon_runtime/src/lib.rs".to_owned(),
            line: 1,
            byte_column: 1,
            ordinal: 1,
            probe: "clone-call".to_owned(),
            category: "recursive-clone".to_owned(),
            execution_class: "hot".to_owned(),
            owner_phase: OWNER_PHASE.to_owned(),
            owner_plan: OWNER_PLAN.to_owned(),
            replacement_phase: "packed-phase-1-dense-semantic-artifacts".to_owned(),
            reason: "candidate-recursive-clone-in-hot-runtime".to_owned(),
            context_sha256: "00".repeat(32),
        };
        let valid = render_ledger(&[occurrence]);
        parse_ledger(valid.as_bytes()).unwrap();
        let invalid = valid.replace("\trecursive-clone\t", "\tinvented-category\t");
        assert!(parse_ledger(invalid.as_bytes()).is_err());
    }

    #[test]
    fn checked_in_inventory_is_current() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        validate(&workspace).unwrap();
    }
}
