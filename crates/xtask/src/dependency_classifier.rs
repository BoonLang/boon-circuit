use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use syn::parse::Parser;

pub(crate) const REGISTRY_RELATIVE_PATH: &str =
    "docs/architecture/phase1/dependency_classifier_schema_v1.toml";
const REGISTRY_SCHEMA_V1: &str = "boon.dependency-classifier-schema.v1";
const REGISTRY_DIGEST_DOMAIN: &[u8] = b"boon.dependency-classifier-schema.v1\0";
const SEMANTIC_DIGEST_SOURCE: &str = "crates/boon_semantic/src/lib.rs";
const SEMANTIC_DIGEST_CONST: &str = "DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1";
const REMOVED_OPTIONAL_PARAMETERS_TOKEN: &str = "optional_parameters";
const PARAMETER_SEMANTICS_SOURCE_ROOTS: &[&str] = &[
    "crates/boon_ir/src",
    "crates/boon_semantic/src",
    "crates/boon_typecheck/src",
];

pub(crate) fn verify(workspace: &Path) -> Result<String, String> {
    let registry_path = workspace.join(REGISTRY_RELATIVE_PATH);
    let registry_text = fs::read_to_string(&registry_path)
        .map_err(|error| format!("{}: {error}", registry_path.display()))?;
    let validated = validate_registry(&registry_text, |relative| {
        let path = workspace.join(relative);
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
    })?;
    let semantic_path = workspace.join(SEMANTIC_DIGEST_SOURCE);
    let semantic_text = fs::read_to_string(&semantic_path)
        .map_err(|error| format!("{}: {error}", semantic_path.display()))?;
    let checked_in_digest = extract_semantic_digest_constant(&semantic_text)?;
    let checked_in_digest = hex(&checked_in_digest);
    if checked_in_digest != validated.digest {
        return Err(format!(
            "dependency classifier canonical digest `{}` does not match `{SEMANTIC_DIGEST_CONST}` `{checked_in_digest}` in `{SEMANTIC_DIGEST_SOURCE}`",
            validated.digest
        ));
    }
    Ok(format!(
        "{} records and {} explicit field/variant dispositions; schema digest {}",
        validated.record_count, validated.disposition_count, validated.digest
    ))
}

pub(crate) fn verify_parameter_semantics_deletion(workspace: &Path) -> Result<String, String> {
    let mut source_files = Vec::new();
    for root in PARAMETER_SEMANTICS_SOURCE_ROOTS {
        collect_rust_sources(&workspace.join(root), &mut source_files)?;
    }
    source_files.sort();
    for path in &source_files {
        let text =
            fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        reject_removed_optional_parameters(path, &text)?;
    }
    Ok(format!(
        "removed hidden `{REMOVED_OPTIONAL_PARAMETERS_TOKEN}` side table is absent from {} compiler source files",
        source_files.len()
    ))
}

fn collect_rust_sources(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    entries.sort_by_key(std::fs::DirEntry::path);
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_rust_sources(&path, files)?;
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn reject_removed_optional_parameters(path: &Path, text: &str) -> Result<(), String> {
    if let Some((line_index, _)) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(REMOVED_OPTIONAL_PARAMETERS_TOKEN))
    {
        return Err(format!(
            "{}:{} contains forbidden legacy `{REMOVED_OPTIONAL_PARAMETERS_TOKEN}` side-table token; required/default semantics must live on CheckedParameter",
            path.display(),
            line_index + 1
        ));
    }
    Ok(())
}

fn extract_semantic_digest_constant(source: &str) -> Result<[u8; 32], String> {
    let syntax = syn::parse_file(source)
        .map_err(|error| format!("cannot parse `{SEMANTIC_DIGEST_SOURCE}`: {error}"))?;
    let mut matches = syntax.items.iter().filter_map(|item| match item {
        syn::Item::Const(item) if item.ident == SEMANTIC_DIGEST_CONST => Some(item),
        _ => None,
    });
    let item = matches.next().ok_or_else(|| {
        format!(
            "`{SEMANTIC_DIGEST_SOURCE}` is missing exact public `{SEMANTIC_DIGEST_CONST}: [u8; 32]`"
        )
    })?;
    if matches.next().is_some() {
        return Err(format!(
            "`{SEMANTIC_DIGEST_SOURCE}` defines `{SEMANTIC_DIGEST_CONST}` more than once"
        ));
    }
    if !matches!(item.vis, syn::Visibility::Public(_)) {
        return Err(format!(
            "`{SEMANTIC_DIGEST_SOURCE}` constant `{SEMANTIC_DIGEST_CONST}` must be public"
        ));
    }
    let syn::Type::Array(array_type) = item.ty.as_ref() else {
        return Err(format!(
            "`{SEMANTIC_DIGEST_CONST}` must have exact type `[u8; 32]`"
        ));
    };
    let element_is_u8 = matches!(
        array_type.elem.as_ref(),
        syn::Type::Path(path)
            if path.qself.is_none() && path.path.is_ident("u8")
    );
    let length_is_32 = matches!(
        &array_type.len,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(length),
            ..
        }) if length.base10_parse::<usize>().is_ok_and(|length| length == 32)
    );
    if !element_is_u8 || !length_is_32 {
        return Err(format!(
            "`{SEMANTIC_DIGEST_CONST}` must have exact type `[u8; 32]`"
        ));
    }
    let syn::Expr::Array(array) = item.expr.as_ref() else {
        return Err(format!(
            "`{SEMANTIC_DIGEST_CONST}` must list exactly 32 literal bytes"
        ));
    };
    if array.elems.len() != 32 {
        return Err(format!(
            "`{SEMANTIC_DIGEST_CONST}` lists {} bytes; expected exactly 32",
            array.elems.len()
        ));
    }
    let mut bytes = [0_u8; 32];
    for (index, expression) in array.elems.iter().enumerate() {
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(value),
            ..
        }) = expression
        else {
            return Err(format!(
                "`{SEMANTIC_DIGEST_CONST}` byte {index} is not an integer literal"
            ));
        };
        bytes[index] = value.base10_parse::<u8>().map_err(|error| {
            format!("`{SEMANTIC_DIGEST_CONST}` byte {index} is not a valid u8 literal: {error}")
        })?;
    }
    Ok(bytes)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryV1 {
    schema: String,
    sources: Vec<SourceSpecV1>,
    dispositions: Vec<DispositionSpecV1>,
    records: Vec<RecordSpecV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceSpecV1 {
    path: String,
    selection: SourceSelectionV1,
    prefixes: Vec<String>,
    explicit_types: Vec<String>,
    additional_types: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceSelectionV1 {
    AllPublic,
    Prefixes,
    Explicit,
}

fn required_source_specs() -> Vec<SourceSpecV1> {
    vec![
        SourceSpecV1 {
            path: "crates/boon_document_model/src/lib.rs".to_owned(),
            selection: SourceSelectionV1::Explicit,
            prefixes: Vec::new(),
            explicit_types: vec!["ProgramRole".to_owned()],
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_ir/src/lib.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/program_core.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_ir/src/semantic_mapping.rs".to_owned(),
            selection: SourceSelectionV1::Explicit,
            prefixes: Vec::new(),
            explicit_types: [
                "ExecutableLexicalScopeId",
                "MappedReactiveBindingId",
                "MappedReactiveFieldId",
                "MappedReactiveReadId",
                "MappedReactiveTriggerId",
                "MappedSemanticBinding",
                "MappedSemanticBindingTarget",
                "MappedSemanticCallInvocationSchedule",
                "MappedSemanticDependencyTarget",
                "MappedSemanticDependencyUse",
                "MappedSemanticDerivedValue",
                "MappedSemanticExecution",
                "MappedSemanticExternalReference",
                "MappedSemanticExternalReferenceKind",
                "MappedSemanticField",
                "MappedSemanticHostEffectSchedule",
                "MappedSemanticListMutation",
                "MappedSemanticProducerInstance",
                "MappedSemanticReactive",
                "MappedSemanticRead",
                "MappedSemanticReadTarget",
                "MappedSemanticResources",
                "MappedSemanticSource",
                "MappedSemanticStateUpdateArm",
                "MappedSemanticStorage",
                "MappedSemanticStorageDependencyTarget",
                "MappedSemanticStorageDependencyUse",
                "MappedSemanticStorageRead",
                "MappedSemanticStorageReadTarget",
                "MappedSemanticTriggerArm",
                "SemanticReactiveToMappedMap",
                "SemanticStorageToErasedMap",
                "SemanticToExecutableMap",
            ]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/dependency_manifest.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/execution.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/lib.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/lowering_contract.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/memory_contract.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/out_net.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/reactive.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/resource.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/storage_contract.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_semantic/src/view_contract.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_typecheck/src/lib.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: Vec::new(),
        },
        SourceSpecV1 {
            path: "crates/boon_verify/src/lib.rs".to_owned(),
            selection: SourceSelectionV1::AllPublic,
            prefixes: Vec::new(),
            explicit_types: Vec::new(),
            additional_types: vec![
                "RequiredObligationDigestPayloadV1".to_owned(),
                "VerificationManifestDigestPayloadV1".to_owned(),
            ],
        },
    ]
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RecordSpecV1 {
    Struct {
        source: String,
        name: String,
        fields: Vec<FieldDispositionV1>,
    },
    Enum {
        source: String,
        name: String,
        variants: Vec<VariantDispositionV1>,
    },
}

impl RecordSpecV1 {
    fn source(&self) -> &str {
        match self {
            Self::Struct { source, .. } | Self::Enum { source, .. } => source,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Struct { name, .. } | Self::Enum { name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VariantDispositionV1 {
    name: String,
    disposition: String,
    reviewed: bool,
    fields: Vec<FieldDispositionV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FieldDispositionV1 {
    name: String,
    disposition: String,
    reviewed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct DispositionSpecV1 {
    id: String,
    traversal: TraversalV1,
    roles: Vec<DependencyRoleV1>,
    visibility: DependencyVisibilityV1,
    hash_targets: Vec<HashTargetV1>,
    erasure: ErasurePolicyV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum TraversalV1 {
    Recurse,
    SemanticAtom,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum DependencyRoleV1 {
    FormulaBinder,
    ResourceOrProvider,
    CoverageOrRouting,
    AssuranceOrActivation,
    DiagnosticOrSource,
    IntentionallyNonsemantic,
    ForbiddenInVerifiedSlice,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum DependencyVisibilityV1 {
    Public,
    Private,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum HashTargetV1 {
    PublicStatement,
    SemanticProgram,
    ImplementationDependency,
    ProofContext,
    Obligation,
    EvidenceCache,
    SourceOnly,
    None,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ErasurePolicyV1 {
    PreserveSemantic,
    NormalizeThenErase,
    EraseAfterVerification,
    Reject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RustRecordSchema {
    Struct { fields: Vec<String> },
    Enum { variants: Vec<RustVariantSchema> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RustRecordDefinition {
    is_public: bool,
    schema: RustRecordSchema,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RustVariantSchema {
    name: String,
    fields: Vec<String>,
}

#[derive(Debug)]
struct ValidatedRegistry {
    digest: String,
    record_count: usize,
    disposition_count: usize,
}

fn validate_registry(
    registry_text: &str,
    read_source: impl FnMut(&str) -> Result<String, String>,
) -> Result<ValidatedRegistry, String> {
    let required_sources = required_source_specs();
    validate_registry_with_source_contract(registry_text, read_source, Some(&required_sources))
}

fn validate_registry_with_source_contract(
    registry_text: &str,
    mut read_source: impl FnMut(&str) -> Result<String, String>,
    required_sources: Option<&[SourceSpecV1]>,
) -> Result<ValidatedRegistry, String> {
    let mut registry = toml::from_str::<RegistryV1>(registry_text)
        .map_err(|error| format!("dependency classifier registry is invalid TOML: {error}"))?;
    if registry.schema != REGISTRY_SCHEMA_V1 {
        return Err(format!(
            "dependency classifier registry schema `{}` is unsupported; expected `{REGISTRY_SCHEMA_V1}`",
            registry.schema
        ));
    }
    validate_source_specs(&registry.sources)?;
    if required_sources.is_some_and(|required| registry.sources != required) {
        let required_sources = required_sources.expect("checked as some");
        let configured = registry
            .sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>();
        let required = required_sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>();
        return Err(format!(
            "dependency classifier tracked-source contract drifted; configured [{}], required [{}]",
            configured.join(", "),
            required.join(", ")
        ));
    }
    let dispositions = validate_disposition_specs(&registry.dispositions)?;

    let mut source_schemas = BTreeMap::<String, BTreeMap<String, RustRecordSchema>>::new();
    for source in &registry.sources {
        let text = read_source(&source.path)?;
        let syntax = syn::parse_file(&text).map_err(|error| {
            format!(
                "cannot parse tracked Rust source `{}`: {error}",
                source.path
            )
        })?;
        let all_records = extract_top_level_records(&syntax, &source.path)?;
        let selected = select_records(source, &all_records)?;
        source_schemas.insert(source.path.clone(), selected);
    }

    let used_dispositions =
        validate_record_specs(&registry.records, &source_schemas, &dispositions)?;
    let defined_dispositions = dispositions.keys().copied().collect::<BTreeSet<_>>();
    let used_disposition_ids = used_dispositions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if used_disposition_ids != defined_dispositions {
        let unused = defined_dispositions
            .difference(&used_disposition_ids)
            .copied()
            .collect::<Vec<_>>();
        return Err(format!(
            "dependency classifier defines unused dispositions [{}]",
            unused.join(", ")
        ));
    }
    let disposition_count = registry.records.iter().map(record_disposition_count).sum();
    registry
        .sources
        .sort_by(|left, right| left.path.cmp(&right.path));
    registry
        .dispositions
        .sort_by(|left, right| left.id.cmp(&right.id));
    registry
        .records
        .sort_by(|left, right| (left.source(), left.name()).cmp(&(right.source(), right.name())));
    let canonical = serde_json::to_vec(&registry)
        .map_err(|error| format!("cannot encode classifier registry canonically: {error}"))?;
    let digest = domain_hash(REGISTRY_DIGEST_DOMAIN, &canonical);
    Ok(ValidatedRegistry {
        digest: hex(&digest),
        record_count: registry.records.len(),
        disposition_count,
    })
}

fn validate_source_specs(sources: &[SourceSpecV1]) -> Result<(), String> {
    if sources.is_empty() {
        return Err("dependency classifier registry has no tracked Rust sources".to_owned());
    }
    let mut seen = BTreeSet::new();
    for source in sources {
        validate_exact_name("source path", &source.path)?;
        if !source.path.ends_with(".rs") {
            return Err(format!(
                "tracked dependency classifier source `{}` is not a Rust file",
                source.path
            ));
        }
        if !seen.insert(source.path.as_str()) {
            return Err(format!(
                "tracked dependency classifier source `{}` is duplicated",
                source.path
            ));
        }
        require_sorted_unique("source prefix", &source.prefixes)?;
        require_sorted_unique("source explicit type", &source.explicit_types)?;
        require_sorted_unique("source additional type", &source.additional_types)?;
        match source.selection {
            SourceSelectionV1::AllPublic => {
                if !source.prefixes.is_empty() || !source.explicit_types.is_empty() {
                    return Err(format!(
                        "all_public source `{}` must use empty prefixes and explicit_types",
                        source.path
                    ));
                }
            }
            SourceSelectionV1::Prefixes => {
                if source.prefixes.is_empty() || !source.explicit_types.is_empty() {
                    return Err(format!(
                        "prefix source `{}` requires prefixes and empty explicit_types",
                        source.path
                    ));
                }
            }
            SourceSelectionV1::Explicit => {
                if source.explicit_types.is_empty() || !source.prefixes.is_empty() {
                    return Err(format!(
                        "explicit source `{}` requires explicit_types and empty prefixes",
                        source.path
                    ));
                }
            }
        }
        let selected = source
            .explicit_types
            .iter()
            .chain(&source.additional_types)
            .collect::<BTreeSet<_>>();
        if selected.len() != source.explicit_types.len() + source.additional_types.len() {
            return Err(format!(
                "source `{}` repeats a type between explicit_types and additional_types",
                source.path
            ));
        }
    }
    Ok(())
}

fn validate_disposition_specs(
    dispositions: &[DispositionSpecV1],
) -> Result<BTreeMap<&str, &DispositionSpecV1>, String> {
    if dispositions.is_empty() {
        return Err("dependency classifier registry has no dispositions".to_owned());
    }
    let mut by_id = BTreeMap::new();
    for disposition in dispositions {
        validate_exact_name("disposition ID", &disposition.id)?;
        if by_id.insert(disposition.id.as_str(), disposition).is_some() {
            return Err(format!(
                "dependency classifier disposition `{}` is duplicated",
                disposition.id
            ));
        }
        validate_disposition(
            &format!("policy {}", disposition.id),
            disposition.traversal,
            &disposition.roles,
            disposition.visibility,
            &disposition.hash_targets,
            disposition.erasure,
        )?;
    }
    Ok(by_id)
}

fn extract_top_level_records(
    syntax: &syn::File,
    source: &str,
) -> Result<BTreeMap<String, RustRecordDefinition>, String> {
    let mut records = BTreeMap::new();
    for item in &syntax.items {
        match item {
            syn::Item::Struct(item) => {
                insert_record(
                    &mut records,
                    source,
                    item.ident.to_string(),
                    RustRecordDefinition {
                        is_public: matches!(item.vis, syn::Visibility::Public(_)),
                        schema: RustRecordSchema::Struct {
                            fields: field_names(&item.fields),
                        },
                    },
                )?;
            }
            syn::Item::Enum(item) => {
                let mut variants = Vec::with_capacity(item.variants.len());
                for variant in &item.variants {
                    if variant.discriminant.is_some() {
                        return Err(format!(
                            "tracked enum `{}::{}` in `{source}` has an explicit discriminant; classifier support must be defined before it can be admitted",
                            item.ident, variant.ident
                        ));
                    }
                    variants.push(RustVariantSchema {
                        name: variant.ident.to_string(),
                        fields: field_names(&variant.fields),
                    });
                }
                insert_record(
                    &mut records,
                    source,
                    item.ident.to_string(),
                    RustRecordDefinition {
                        is_public: matches!(item.vis, syn::Visibility::Public(_)),
                        schema: RustRecordSchema::Enum { variants },
                    },
                )?;
            }
            syn::Item::Macro(item) if tracked_record_macro(item) => {
                let names =
                    syn::punctuated::Punctuated::<syn::Ident, syn::Token![,]>::parse_terminated
                        .parse2(item.mac.tokens.clone())
                        .map_err(|error| {
                            format!(
                                "cannot parse tracked record macro `{}` in `{source}`: {error}",
                                item.mac
                                    .path
                                    .segments
                                    .last()
                                    .expect("macro path is nonempty")
                                    .ident
                            )
                        })?;
                for name in names {
                    insert_record(
                        &mut records,
                        source,
                        name.to_string(),
                        RustRecordDefinition {
                            is_public: true,
                            schema: RustRecordSchema::Struct {
                                fields: vec!["0".to_owned()],
                            },
                        },
                    )?;
                }
            }
            syn::Item::Macro(item) if item.ident.is_none() && !reviewed_nonrecord_macro(item) => {
                return Err(format!(
                    "tracked Rust source `{source}` contains unreviewed top-level macro invocation `{}`; classify its generated record surface or explicitly teach the extractor that it emits no records",
                    item.mac
                        .path
                        .segments
                        .last()
                        .expect("macro path is nonempty")
                        .ident
                ));
            }
            _ => {}
        }
    }
    Ok(records)
}

fn tracked_record_macro(item: &syn::ItemMacro) -> bool {
    item.mac.path.segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "dependency_id"
                | "digest_type"
                | "id_type"
                | "memory_id"
                | "route_usize_ids"
                | "storage_id"
                | "string_ids"
                | "typed_lowering_id"
                | "typed_out_id"
                | "typed_semantic_id"
                | "typed_usize_ids"
                | "view_id"
        )
    })
}

fn reviewed_nonrecord_macro(item: &syn::ItemMacro) -> bool {
    item.mac
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "thread_local")
}

fn insert_record(
    records: &mut BTreeMap<String, RustRecordDefinition>,
    source: &str,
    name: String,
    definition: RustRecordDefinition,
) -> Result<(), String> {
    if records.insert(name.clone(), definition).is_some() {
        return Err(format!(
            "tracked Rust source `{source}` defines record `{name}` more than once"
        ));
    }
    Ok(())
}

fn field_names(fields: &syn::Fields) -> Vec<String> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            field
                .ident
                .as_ref()
                .map_or_else(|| index.to_string(), ToString::to_string)
        })
        .collect()
}

fn select_records(
    source: &SourceSpecV1,
    all_records: &BTreeMap<String, RustRecordDefinition>,
) -> Result<BTreeMap<String, RustRecordSchema>, String> {
    let mut selected = BTreeMap::new();
    match source.selection {
        SourceSelectionV1::AllPublic => {
            selected.extend(
                all_records
                    .iter()
                    .filter(|(_, definition)| definition.is_public)
                    .map(|(name, definition)| (name.clone(), definition.schema.clone())),
            );
        }
        SourceSelectionV1::Prefixes => {
            for (name, definition) in all_records {
                if source
                    .prefixes
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                {
                    selected.insert(name.clone(), definition.schema.clone());
                }
            }
        }
        SourceSelectionV1::Explicit => {
            for name in &source.explicit_types {
                let definition = all_records.get(name).ok_or_else(|| {
                    format!(
                        "explicit classifier type `{name}` is missing from `{}`",
                        source.path
                    )
                })?;
                selected.insert(name.clone(), definition.schema.clone());
            }
        }
    }
    for name in &source.additional_types {
        let definition = all_records.get(name).ok_or_else(|| {
            format!(
                "additional classifier type `{name}` is missing from `{}`",
                source.path
            )
        })?;
        selected.insert(name.clone(), definition.schema.clone());
    }
    Ok(selected)
}

fn validate_record_specs(
    records: &[RecordSpecV1],
    source_schemas: &BTreeMap<String, BTreeMap<String, RustRecordSchema>>,
    dispositions: &BTreeMap<&str, &DispositionSpecV1>,
) -> Result<BTreeSet<String>, String> {
    let mut registry_keys = BTreeSet::new();
    let mut used_dispositions = BTreeSet::new();
    for record in records {
        validate_exact_name("record source", record.source())?;
        validate_exact_name("record name", record.name())?;
        let key = (record.source(), record.name());
        if !registry_keys.insert(key) {
            return Err(format!(
                "dependency classifier record `{}` in `{}` is duplicated",
                record.name(),
                record.source()
            ));
        }
        let schema = source_schemas
            .get(record.source())
            .ok_or_else(|| {
                format!(
                    "dependency classifier record `{}` references untracked source `{}`",
                    record.name(),
                    record.source()
                )
            })?
            .get(record.name())
            .ok_or_else(|| {
                format!(
                    "dependency classifier registry has stale or unselected record `{}` in `{}`",
                    record.name(),
                    record.source()
                )
            })?;
        match (record, schema) {
            (
                RecordSpecV1::Struct { fields, .. },
                RustRecordSchema::Struct {
                    fields: source_fields,
                },
            ) => {
                validate_fields(
                    record.name(),
                    fields,
                    source_fields,
                    dispositions,
                    &mut used_dispositions,
                )?;
            }
            (
                RecordSpecV1::Enum { variants, .. },
                RustRecordSchema::Enum {
                    variants: source_variants,
                },
            ) => {
                validate_variants(
                    record.name(),
                    variants,
                    source_variants,
                    dispositions,
                    &mut used_dispositions,
                )?;
            }
            (RecordSpecV1::Struct { .. }, RustRecordSchema::Enum { .. })
            | (RecordSpecV1::Enum { .. }, RustRecordSchema::Struct { .. }) => {
                return Err(format!(
                    "dependency classifier record `{}` changed between struct and enum",
                    record.name()
                ));
            }
        }
    }

    let source_keys = source_schemas
        .iter()
        .flat_map(|(source, records)| {
            records
                .keys()
                .map(move |name| (source.as_str(), name.as_str()))
        })
        .collect::<BTreeSet<_>>();
    if registry_keys != source_keys {
        let missing = source_keys
            .difference(&registry_keys)
            .map(|(source, name)| format!("{source}:{name}"))
            .collect::<Vec<_>>();
        let extra = registry_keys
            .difference(&source_keys)
            .map(|(source, name)| format!("{source}:{name}"))
            .collect::<Vec<_>>();
        return Err(format!(
            "dependency classifier record inventory drifted; missing [{}]; extra [{}]",
            missing.join(", "),
            extra.join(", ")
        ));
    }
    Ok(used_dispositions)
}

fn validate_fields(
    context: &str,
    fields: &[FieldDispositionV1],
    source_fields: &[String],
    dispositions: &BTreeMap<&str, &DispositionSpecV1>,
    used_dispositions: &mut BTreeSet<String>,
) -> Result<(), String> {
    let registry_fields = fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    require_no_duplicate_names(&format!("{context} field"), &registry_fields)?;
    for field in fields {
        if !field.reviewed {
            return Err(format!(
                "dependency classifier member `{context}.{}` is not explicitly reviewed",
                field.name
            ));
        }
        validate_disposition_reference(
            &format!("{context}.{}", field.name),
            &field.disposition,
            dispositions,
            used_dispositions,
        )?;
    }
    if registry_fields != source_fields {
        return Err(format!(
            "dependency classifier fields for `{context}` drifted; source order [{}], registry order [{}]",
            source_fields.join(", "),
            registry_fields.join(", ")
        ));
    }
    Ok(())
}

fn validate_variants(
    context: &str,
    variants: &[VariantDispositionV1],
    source_variants: &[RustVariantSchema],
    dispositions: &BTreeMap<&str, &DispositionSpecV1>,
    used_dispositions: &mut BTreeSet<String>,
) -> Result<(), String> {
    let registry_variants = variants
        .iter()
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    let source_names = source_variants
        .iter()
        .map(|variant| variant.name.clone())
        .collect::<Vec<_>>();
    require_no_duplicate_names(&format!("{context} variant"), &registry_variants)?;
    if registry_variants != source_names {
        return Err(format!(
            "dependency classifier variants for `{context}` drifted; source order [{}], registry order [{}]",
            source_names.join(", "),
            registry_variants.join(", ")
        ));
    }
    for (variant, source_variant) in variants.iter().zip(source_variants) {
        if !variant.reviewed {
            return Err(format!(
                "dependency classifier member `{context}::{}` is not explicitly reviewed",
                variant.name
            ));
        }
        validate_disposition_reference(
            &format!("{context}::{}", variant.name),
            &variant.disposition,
            dispositions,
            used_dispositions,
        )?;
        validate_fields(
            &format!("{context}::{}", variant.name),
            &variant.fields,
            &source_variant.fields,
            dispositions,
            used_dispositions,
        )?;
    }
    Ok(())
}

fn validate_disposition_reference(
    context: &str,
    disposition: &str,
    dispositions: &BTreeMap<&str, &DispositionSpecV1>,
    used_dispositions: &mut BTreeSet<String>,
) -> Result<(), String> {
    validate_exact_name("disposition reference", disposition)?;
    if !dispositions.contains_key(disposition) {
        return Err(format!(
            "dependency classifier member `{context}` references undefined disposition `{disposition}`"
        ));
    }
    used_dispositions.insert(disposition.to_owned());
    Ok(())
}

fn validate_disposition(
    context: &str,
    traversal: TraversalV1,
    roles: &[DependencyRoleV1],
    visibility: DependencyVisibilityV1,
    hash_targets: &[HashTargetV1],
    erasure: ErasurePolicyV1,
) -> Result<(), String> {
    if roles.is_empty() {
        return Err(format!(
            "dependency classifier disposition `{context}` has no semantic role"
        ));
    }
    require_sorted_unique_copy("dependency role", roles)?;
    if hash_targets.is_empty() {
        return Err(format!(
            "dependency classifier disposition `{context}` has no hash target"
        ));
    }
    require_sorted_unique_copy("hash target", hash_targets)?;
    if hash_targets.contains(&HashTargetV1::None) && hash_targets.len() != 1 {
        return Err(format!(
            "dependency classifier disposition `{context}` mixes `none` with semantic hash targets"
        ));
    }
    let intentionally_nonsemantic = roles.contains(&DependencyRoleV1::IntentionallyNonsemantic);
    let forbidden = roles.contains(&DependencyRoleV1::ForbiddenInVerifiedSlice);
    if roles.contains(&DependencyRoleV1::IntentionallyNonsemantic)
        && (roles.len() != 1
            || traversal != TraversalV1::None
            || hash_targets != [HashTargetV1::None]
            || visibility == DependencyVisibilityV1::Public
            || erasure == ErasurePolicyV1::PreserveSemantic)
    {
        return Err(format!(
            "intentionally nonsemantic disposition `{context}` must use traversal none, hash target none, and an erasing policy"
        ));
    }
    if hash_targets == [HashTargetV1::None] && !intentionally_nonsemantic {
        return Err(format!(
            "dependency classifier disposition `{context}` may use hash target none only for intentionally nonsemantic data"
        ));
    }
    if forbidden
        && (roles.len() != 1
            || traversal != TraversalV1::None
            || visibility != DependencyVisibilityV1::Private
            || hash_targets != [HashTargetV1::ImplementationDependency]
            || erasure != ErasurePolicyV1::Reject)
    {
        return Err(format!(
            "forbidden disposition `{context}` must be private, non-traversed, implementation-hashed, rejected, and carry no other role"
        ));
    }
    if erasure == ErasurePolicyV1::Reject && !forbidden {
        return Err(format!(
            "reject disposition `{context}` lacks forbidden_in_verified_slice role"
        ));
    }
    if hash_targets.contains(&HashTargetV1::SourceOnly)
        && (hash_targets.len() != 1
            || roles != [DependencyRoleV1::DiagnosticOrSource]
            || visibility != DependencyVisibilityV1::Diagnostic
            || erasure != ErasurePolicyV1::NormalizeThenErase)
    {
        return Err(format!(
            "source-only disposition `{context}` must be diagnostic-only and normalized away"
        ));
    }
    if hash_targets.contains(&HashTargetV1::PublicStatement)
        && (visibility != DependencyVisibilityV1::Public
            || erasure != ErasurePolicyV1::PreserveSemantic
            || !hash_targets.contains(&HashTargetV1::SemanticProgram))
    {
        return Err(format!(
            "public-statement disposition `{context}` must be public, preserved, and semantic-program hashed"
        ));
    }
    if erasure == ErasurePolicyV1::PreserveSemantic
        && (!hash_targets.contains(&HashTargetV1::SemanticProgram)
            || visibility != DependencyVisibilityV1::Public)
    {
        return Err(format!(
            "preserved semantic disposition `{context}` must be public and semantic-program hashed"
        ));
    }
    if hash_targets.contains(&HashTargetV1::Obligation)
        && !hash_targets.contains(&HashTargetV1::ProofContext)
    {
        return Err(format!(
            "obligation disposition `{context}` must also hash into the proof context"
        ));
    }
    if hash_targets.contains(&HashTargetV1::EvidenceCache)
        && (!hash_targets.contains(&HashTargetV1::ProofContext)
            || !hash_targets.contains(&HashTargetV1::Obligation))
    {
        return Err(format!(
            "evidence-cache disposition `{context}` must also hash into proof context and obligation"
        ));
    }
    Ok(())
}

fn validate_exact_name(kind: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.contains('*')
        || value.contains("..")
        || value == "_"
        || value.contains('?')
    {
        return Err(format!(
            "dependency classifier {kind} `{value}` is empty or wildcard-like; exact names are required"
        ));
    }
    Ok(())
}

fn require_sorted_unique(kind: &str, values: &[String]) -> Result<(), String> {
    for value in values {
        validate_exact_name(kind, value)?;
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!(
            "dependency classifier {kind} values must be strictly sorted and unique"
        ));
    }
    Ok(())
}

fn require_no_duplicate_names(kind: &str, values: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_exact_name(kind, value)?;
        if !seen.insert(value) {
            return Err(format!(
                "dependency classifier {kind} `{value}` is duplicated"
            ));
        }
    }
    Ok(())
}

fn require_sorted_unique_copy<T: Copy + Ord>(kind: &str, values: &[T]) -> Result<(), String> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!(
            "dependency classifier {kind} values must be strictly sorted and unique"
        ));
    }
    Ok(())
}

fn record_disposition_count(record: &RecordSpecV1) -> usize {
    match record {
        RecordSpecV1::Struct { fields, .. } => fields.len(),
        RecordSpecV1::Enum { variants, .. } => {
            variants.len()
                + variants
                    .iter()
                    .map(|variant| variant.fields.len())
                    .sum::<usize>()
        }
    }
}

fn domain_hash(domain: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(
        u64::try_from(payload.len())
            .expect("Rust byte slices cannot exceed u64")
            .to_be_bytes(),
    );
    hasher.update(payload);
    hasher.finalize().into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
pub struct Sample {
    pub value: u64,
}

pub enum Mode {
    Ready,
    Data { payload: String },
}
"#;

    const PARAMETER_SOURCE: &str = r#"
pub struct CheckedParameter {
    pub decl_id: u32,
    pub name: String,
    pub kind: u8,
    pub ordinal: usize,
    pub flow_type: u8,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: u8,
    pub start: usize,
    pub end: usize,
}

pub enum CheckedParameterDefault {
    CallableProfile { profile: String },
    Bool { value: bool },
    ExactInteger { value: i64 },
    Text { value: String },
}

pub enum CheckedParameterRequirement {
    Required,
    Optional { default: CheckedParameterDefault },
}
"#;

    fn registry() -> String {
        r#"
schema = "boon.dependency-classifier-schema.v1"

[[sources]]
path = "fixture.rs"
selection = "explicit"
prefixes = []
explicit_types = ["Mode", "Sample"]
additional_types = []

[[dispositions]]
id = "private_coverage"
traversal = "semantic_atom"
roles = ["coverage_or_routing"]
visibility = "private"
hash_targets = ["semantic_program"]
erasure = "normalize_then_erase"

[[dispositions]]
id = "public_formula"
traversal = "recurse"
roles = ["formula_binder"]
visibility = "public"
hash_targets = ["public_statement", "semantic_program"]
erasure = "preserve_semantic"

[[records]]
kind = "struct"
source = "fixture.rs"
name = "Sample"
fields = [
  { name = "value", disposition = "public_formula", reviewed = true },
]

[[records]]
kind = "enum"
source = "fixture.rs"
name = "Mode"

[[records.variants]]
name = "Ready"
disposition = "private_coverage"
reviewed = true
fields = []

[[records.variants]]
name = "Data"
disposition = "public_formula"
reviewed = true
fields = [
  { name = "payload", disposition = "public_formula", reviewed = true },
]
"#
        .to_owned()
    }

    fn validate(text: &str, source: &str) -> Result<ValidatedRegistry, String> {
        validate_registry_with_source_contract(
            text,
            |path| {
                if path == "fixture.rs" {
                    Ok(source.to_owned())
                } else {
                    Err(format!("unexpected fixture path `{path}`"))
                }
            },
            None,
        )
    }

    fn parameter_registry() -> String {
        r#"
schema = "boon.dependency-classifier-schema.v1"

[[sources]]
path = "parameter_fixture.rs"
selection = "explicit"
prefixes = []
explicit_types = ["CheckedParameter", "CheckedParameterDefault", "CheckedParameterRequirement"]
additional_types = []

[[dispositions]]
id = "parameter_semantics"
traversal = "recurse"
roles = ["formula_binder", "assurance_or_activation"]
visibility = "private"
hash_targets = ["semantic_program", "implementation_dependency"]
erasure = "normalize_then_erase"

[[records]]
kind = "struct"
source = "parameter_fixture.rs"
name = "CheckedParameter"
fields = [
  { name = "decl_id", disposition = "parameter_semantics", reviewed = true },
  { name = "name", disposition = "parameter_semantics", reviewed = true },
  { name = "kind", disposition = "parameter_semantics", reviewed = true },
  { name = "ordinal", disposition = "parameter_semantics", reviewed = true },
  { name = "flow_type", disposition = "parameter_semantics", reviewed = true },
  { name = "requirement", disposition = "parameter_semantics", reviewed = true },
  { name = "evaluation_scope", disposition = "parameter_semantics", reviewed = true },
  { name = "start", disposition = "parameter_semantics", reviewed = true },
  { name = "end", disposition = "parameter_semantics", reviewed = true },
]

[[records]]
kind = "enum"
source = "parameter_fixture.rs"
name = "CheckedParameterDefault"

[[records.variants]]
name = "CallableProfile"
disposition = "parameter_semantics"
reviewed = true
fields = [
  { name = "profile", disposition = "parameter_semantics", reviewed = true },
]

[[records.variants]]
name = "Bool"
disposition = "parameter_semantics"
reviewed = true
fields = [
  { name = "value", disposition = "parameter_semantics", reviewed = true },
]

[[records.variants]]
name = "ExactInteger"
disposition = "parameter_semantics"
reviewed = true
fields = [
  { name = "value", disposition = "parameter_semantics", reviewed = true },
]

[[records.variants]]
name = "Text"
disposition = "parameter_semantics"
reviewed = true
fields = [
  { name = "value", disposition = "parameter_semantics", reviewed = true },
]

[[records]]
kind = "enum"
source = "parameter_fixture.rs"
name = "CheckedParameterRequirement"

[[records.variants]]
name = "Required"
disposition = "parameter_semantics"
reviewed = true
fields = []

[[records.variants]]
name = "Optional"
disposition = "parameter_semantics"
reviewed = true
fields = [
  { name = "default", disposition = "parameter_semantics", reviewed = true },
]
"#
        .to_owned()
    }

    fn validate_parameter_source(source: &str) -> Result<ValidatedRegistry, String> {
        validate_registry_with_source_contract(
            &parameter_registry(),
            |path| {
                if path == "parameter_fixture.rs" {
                    Ok(source.to_owned())
                } else {
                    Err(format!("unexpected parameter fixture path `{path}`"))
                }
            },
            None,
        )
    }

    #[test]
    fn exact_registry_is_deterministic() {
        let first = validate(&registry(), SOURCE).unwrap();
        let second = validate(&registry(), SOURCE).unwrap();
        assert_eq!(first.digest, second.digest);
        assert_eq!(first.record_count, 2);
        assert_eq!(first.disposition_count, 4);
    }

    #[test]
    fn source_field_add_remove_and_rename_are_rejected() {
        for changed in [
            SOURCE.replace("pub value: u64,", "pub value: u64,\n    pub added: bool,"),
            SOURCE.replace("    pub value: u64,\n", ""),
            SOURCE.replace("pub value:", "pub renamed:"),
        ] {
            let error = validate(&registry(), &changed).unwrap_err();
            assert!(error.contains("fields for `Sample` drifted"), "{error}");
        }
    }

    #[test]
    fn source_variant_add_remove_and_rename_are_rejected() {
        for changed in [
            SOURCE.replace("    Ready,", "    Ready,\n    Added,"),
            SOURCE.replace("    Ready,\n", ""),
            SOURCE.replace("Ready", "Renamed"),
        ] {
            let error = validate(&registry(), &changed).unwrap_err();
            assert!(error.contains("variants for `Mode` drifted"), "{error}");
        }
    }

    #[test]
    fn duplicate_and_wildcard_registry_entries_are_rejected() {
        let duplicate = registry().replace(
            "fields = [\n  { name = \"value\", disposition = \"public_formula\", reviewed = true },",
            "fields = [\n  { name = \"value\", disposition = \"public_formula\", reviewed = true },\n  { name = \"value\", disposition = \"public_formula\", reviewed = true },",
        );
        let error = validate(&duplicate, SOURCE).unwrap_err();
        assert!(error.contains("duplicated"), "{error}");

        let wildcard = registry().replace("name = \"value\"", "name = \"*\"");
        let error = validate(&wildcard, SOURCE).unwrap_err();
        assert!(error.contains("wildcard-like"), "{error}");
    }

    #[test]
    fn missing_roles_or_policy_dimensions_are_rejected() {
        let missing_roles = registry().replace("roles = [\"formula_binder\"]\n", "");
        let error = validate(&missing_roles, SOURCE).unwrap_err();
        assert!(error.contains("missing field `roles`"), "{error}");

        let missing_erasure = registry().replace("erasure = \"preserve_semantic\"\n", "");
        let error = validate(&missing_erasure, SOURCE).unwrap_err();
        assert!(error.contains("missing field `erasure`"), "{error}");

        let missing_reference = registry().replace(
            "disposition = \"public_formula\"",
            "disposition = \"missing\"",
        );
        let error = validate(&missing_reference, SOURCE).unwrap_err();
        assert!(error.contains("undefined disposition `missing`"), "{error}");

        let missing_review = registry().replace(", reviewed = true", "");
        let error = validate(&missing_review, SOURCE).unwrap_err();
        assert!(error.contains("missing field `reviewed`"), "{error}");

        let unreviewed = registry().replace("reviewed = true", "reviewed = false");
        let error = validate(&unreviewed, SOURCE).unwrap_err();
        assert!(error.contains("not explicitly reviewed"), "{error}");

        let source_only_semantic = registry().replace(
            "hash_targets = [\"semantic_program\"]",
            "hash_targets = [\"source_only\"]",
        );
        let error = validate(&source_only_semantic, SOURCE).unwrap_err();
        assert!(error.contains("source-only disposition"), "{error}");

        let unhashed_semantic = registry().replace(
            "hash_targets = [\"semantic_program\"]",
            "hash_targets = [\"none\"]",
        );
        let error = validate(&unhashed_semantic, SOURCE).unwrap_err();
        assert!(
            error.contains("only for intentionally nonsemantic"),
            "{error}"
        );

        let hidden_public_statement =
            registry().replace("visibility = \"public\"", "visibility = \"private\"");
        let error = validate(&hidden_public_statement, SOURCE).unwrap_err();
        assert!(error.contains("public-statement disposition"), "{error}");

        let malformed_forbidden = registry()
            .replace(
                "roles = [\"coverage_or_routing\"]",
                "roles = [\"forbidden_in_verified_slice\"]",
            )
            .replace("erasure = \"preserve_semantic\"", "erasure = \"reject\"");
        let error = validate(&malformed_forbidden, SOURCE).unwrap_err();
        assert!(error.contains("forbidden disposition"), "{error}");
    }

    #[test]
    fn registry_record_and_variant_field_drift_are_rejected() {
        let stale_record = registry().replace("name = \"Sample\"", "name = \"Stale\"");
        let error = validate(&stale_record, SOURCE).unwrap_err();
        assert!(
            error.contains("stale or unselected record")
                || error.contains("explicit classifier type"),
            "{error}"
        );

        let stale_variant_field = registry().replace("name = \"payload\"", "name = \"stale\"");
        let error = validate(&stale_variant_field, SOURCE).unwrap_err();
        assert!(error.contains("Mode::Data"), "{error}");
    }

    #[test]
    fn removed_optional_parameter_side_table_token_is_rejected() {
        let error = reject_removed_optional_parameters(
            Path::new("fixture.rs"),
            "let optional_parameters = hidden_fallback();",
        )
        .unwrap_err();
        assert!(error.contains("fixture.rs:1"), "{error}");
        assert!(error.contains("must live on CheckedParameter"), "{error}");
        reject_removed_optional_parameters(
            Path::new("fixture.rs"),
            "let parameter_requirement = checked.requirement;",
        )
        .unwrap();
    }

    #[test]
    fn checked_parameter_semantics_have_the_exact_phase1_inventory() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("xtask crate is under the workspace root");
        let path = workspace.join("crates/boon_typecheck/src/lib.rs");
        let text = fs::read_to_string(&path).expect("typecheck source is readable");
        let syntax = syn::parse_file(&text).expect("typecheck source parses");
        let records =
            extract_top_level_records(&syntax, "crates/boon_typecheck/src/lib.rs").unwrap();

        assert_eq!(
            records.get("CheckedParameter").map(|record| &record.schema),
            Some(&RustRecordSchema::Struct {
                fields: [
                    "decl_id",
                    "name",
                    "kind",
                    "ordinal",
                    "flow_type",
                    "requirement",
                    "evaluation_scope",
                    "start",
                    "end",
                ]
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            })
        );
        assert_eq!(
            records
                .get("CheckedParameterRequirement")
                .map(|record| &record.schema),
            Some(&RustRecordSchema::Enum {
                variants: vec![
                    RustVariantSchema {
                        name: "Required".to_owned(),
                        fields: Vec::new(),
                    },
                    RustVariantSchema {
                        name: "Optional".to_owned(),
                        fields: vec!["default".to_owned()],
                    },
                ],
            })
        );
        assert_eq!(
            records
                .get("CheckedParameterDefault")
                .map(|record| &record.schema),
            Some(&RustRecordSchema::Enum {
                variants: vec![
                    RustVariantSchema {
                        name: "CallableProfile".to_owned(),
                        fields: vec!["profile".to_owned()],
                    },
                    RustVariantSchema {
                        name: "Tag".to_owned(),
                        fields: vec!["name".to_owned()],
                    },
                    RustVariantSchema {
                        name: "ExactInteger".to_owned(),
                        fields: vec!["value".to_owned()],
                    },
                    RustVariantSchema {
                        name: "Text".to_owned(),
                        fields: vec!["value".to_owned()],
                    },
                ],
            })
        );
    }

    #[test]
    fn checked_parameter_requirement_field_mutations_are_rejected() {
        for changed in [
            PARAMETER_SOURCE.replace(
                "    pub requirement: CheckedParameterRequirement,",
                "    pub requirement: CheckedParameterRequirement,\n    pub hidden_default: bool,",
            ),
            PARAMETER_SOURCE.replace("    pub requirement: CheckedParameterRequirement,\n", ""),
            PARAMETER_SOURCE.replace(
                "pub requirement: CheckedParameterRequirement",
                "pub renamed_requirement: CheckedParameterRequirement",
            ),
        ] {
            let error = validate_parameter_source(&changed).unwrap_err();
            assert!(
                error.contains("fields for `CheckedParameter` drifted"),
                "{error}"
            );
        }
    }

    #[test]
    fn checked_parameter_requirement_variant_mutations_are_rejected() {
        for changed in [
            PARAMETER_SOURCE.replace("    Required,", "    Required,\n    ConditionallyRequired,"),
            PARAMETER_SOURCE.replace("    Required,\n", ""),
            PARAMETER_SOURCE.replace("    Required,", "    RenamedRequired,"),
        ] {
            let error = validate_parameter_source(&changed).unwrap_err();
            assert!(
                error.contains("variants for `CheckedParameterRequirement` drifted"),
                "{error}"
            );
        }
    }

    #[test]
    fn checked_parameter_default_payload_mutations_are_rejected() {
        for changed in [
            PARAMETER_SOURCE.replace(
                "CallableProfile { profile: String }",
                "CallableProfile { profile: String, revision: u32 }",
            ),
            PARAMETER_SOURCE.replace("CallableProfile { profile: String }", "CallableProfile"),
            PARAMETER_SOURCE.replace(
                "CallableProfile { profile: String }",
                "CallableProfile { renamed_profile: String }",
            ),
        ] {
            let error = validate_parameter_source(&changed).unwrap_err();
            assert!(
                error.contains("CheckedParameterDefault::CallableProfile"),
                "{error}"
            );
        }
    }

    #[test]
    fn production_source_contract_rejects_deletion_reordering_and_selector_downgrade() {
        let required = required_source_specs();
        let base = RegistryV1 {
            schema: REGISTRY_SCHEMA_V1.to_owned(),
            sources: required.clone(),
            dispositions: Vec::new(),
            records: Vec::new(),
        };
        let mut cases = Vec::new();

        let mut deleted = base.clone();
        deleted
            .sources
            .retain(|source| !source.path.ends_with("/dependency_manifest.rs"));
        cases.push(deleted);

        let mut reordered = base.clone();
        reordered.sources.swap(0, 1);
        cases.push(reordered);

        let mut downgraded = base;
        let ir = downgraded
            .sources
            .iter_mut()
            .find(|source| source.path == "crates/boon_semantic/src/program_core.rs")
            .expect("required canonical-core source exists");
        ir.selection = SourceSelectionV1::Prefixes;
        ir.prefixes = vec!["Erased".to_owned()];
        cases.push(downgraded);

        for changed in cases {
            let text = toml::to_string(&changed).expect("test registry serializes");
            let error = validate_registry_with_source_contract(
                &text,
                |_| panic!("must fail closed before reading sources"),
                Some(&required),
            )
            .unwrap_err();
            assert!(error.contains("tracked-source contract drifted"), "{error}");
        }
    }

    #[test]
    fn semantic_digest_constant_requires_exact_public_literal_shape() {
        let literals = (0_u8..32)
            .map(|byte| format!("0x{byte:02x}"))
            .collect::<Vec<_>>()
            .join(", ");
        let valid = format!("pub const {SEMANTIC_DIGEST_CONST}: [u8; 32] = [{literals}];");
        assert_eq!(
            extract_semantic_digest_constant(&valid).unwrap(),
            std::array::from_fn(|index| index as u8)
        );

        for changed in [
            valid.replacen("pub const", "const", 1),
            valid.replacen("[u8; 32]", "[u16; 32]", 1),
            valid.replacen("[u8; 32]", "[u8; 31]", 1),
            format!("pub const {SEMANTIC_DIGEST_CONST}: [u8; 32] = [0; 32];"),
            format!(
                "pub const {SEMANTIC_DIGEST_CONST}: [u8; 32] = [{literals}];\n\
                 pub const {SEMANTIC_DIGEST_CONST}: [u8; 32] = [{literals}];"
            ),
        ] {
            extract_semantic_digest_constant(&changed).unwrap_err();
        }
    }

    #[test]
    fn macro_generated_record_addition_is_rejected() {
        let registry = r#"
schema = "boon.dependency-classifier-schema.v1"

[[sources]]
path = "macro_fixture.rs"
selection = "all_public"
prefixes = []
explicit_types = []
additional_types = []

[[dispositions]]
id = "semantic_id"
traversal = "semantic_atom"
roles = ["coverage_or_routing"]
visibility = "private"
hash_targets = ["semantic_program"]
erasure = "normalize_then_erase"

[[records]]
kind = "struct"
source = "macro_fixture.rs"
name = "SemanticOne"
fields = [
  { name = "0", disposition = "semantic_id", reviewed = true },
]
"#;
        let validate_macro = |source: &str| {
            validate_registry_with_source_contract(
                registry,
                |path| {
                    if path == "macro_fixture.rs" {
                        Ok(source.to_owned())
                    } else {
                        Err(format!("unexpected macro fixture path `{path}`"))
                    }
                },
                None,
            )
        };
        validate_macro("typed_semantic_id!(SemanticOne);").unwrap();
        let error = validate_macro("typed_semantic_id!(SemanticOne, SemanticTwo);").unwrap_err();
        assert!(error.contains("SemanticTwo"), "{error}");
        assert!(error.contains("record inventory drifted"), "{error}");
    }

    #[test]
    fn unreviewed_top_level_macro_invocation_fails_closed() {
        let syntax = syn::parse_file(
            "macro_rules! maybe_records { ($name:ident) => {} }\n\
             maybe_records!(HiddenDependency);",
        )
        .unwrap();
        let error = extract_top_level_records(&syntax, "fixture.rs").unwrap_err();
        assert!(error.contains("unreviewed top-level macro"), "{error}");
        assert!(error.contains("maybe_records"), "{error}");
    }

    #[test]
    fn identifier_words_do_not_match_substrings_and_do_match_plurals() {
        assert!(!contains_identifier_word("SemanticStatement", &["state"]));
        assert!(!contains_identifier_word("RenderSlot", &["end"]));
        assert!(contains_identifier_word("runtime_sources", &["source"]));
        assert!(contains_identifier_word(
            "checked_expression_origins",
            &["origin"]
        ));
        assert!(contains_identifier_word("dependency_policies", &["policy"]));
    }

    #[test]
    fn call_binding_and_parameter_members_are_formula_dependencies() {
        for (source, record, member) in [
            (
                "crates/boon_semantic/src/execution.rs",
                "SemanticCallArgument",
                "formal",
            ),
            (
                "crates/boon_semantic/src/execution.rs",
                "SemanticCallEntry::Input",
                "ordinal",
            ),
            (
                "crates/boon_semantic/src/execution.rs",
                "SemanticFunctionParameter",
                "requirement",
            ),
            (
                "crates/boon_typecheck/src/lib.rs",
                "CheckedCallEntry::Input",
                "from_pipe",
            ),
            (
                "crates/boon_typecheck/src/lib.rs",
                "CheckedParameter",
                "evaluation_scope",
            ),
        ] {
            let disposition = suggest_disposition(source, record, member, None);
            assert!(
                disposition.roles.contains(&DependencyRoleV1::FormulaBinder),
                "{record}.{member} was not classified as formula/binder: {disposition:?}"
            );
        }

        let mapped_binding = suggest_disposition(
            "crates/boon_ir/src/semantic_mapping.rs",
            "SemanticToExecutableMap",
            "local_bindings",
            None,
        );
        assert!(
            mapped_binding
                .roles
                .contains(&DependencyRoleV1::FormulaBinder)
        );
        let mapped_row_scope = suggest_disposition(
            "crates/boon_ir/src/semantic_mapping.rs",
            "SemanticToExecutableMap",
            "row_scopes",
            None,
        );
        assert!(
            mapped_row_scope
                .roles
                .contains(&DependencyRoleV1::ResourceOrProvider)
        );
        let call_result_paths = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "CheckedProgramFields",
            "call_result_paths",
            None,
        );
        assert_eq!(call_result_paths.traversal, TraversalV1::Recurse);
        assert!(
            call_result_paths
                .roles
                .contains(&DependencyRoleV1::FormulaBinder)
        );
        for (source, record, member, variant) in [
            (
                "crates/boon_typecheck/src/lib.rs",
                "CheckedContextualOperation",
                "Map",
                Some(false),
            ),
            (
                "crates/boon_semantic/src/program_core.rs",
                "ExecutableProgram",
                "functions",
                None,
            ),
            (
                "crates/boon_semantic/src/program_core.rs",
                "DerivedValue",
                "kind",
                None,
            ),
            (
                "crates/boon_semantic/src/execution.rs",
                "SemanticStatement",
                "kind",
                None,
            ),
        ] {
            let disposition = suggest_disposition(source, record, member, variant);
            assert!(
                disposition.roles.contains(&DependencyRoleV1::FormulaBinder),
                "{record}.{member} was not classified as formula/binder"
            );
        }
    }

    #[test]
    fn semantic_provenance_effect_and_runtime_origin_policies_are_explicit() {
        let provenance = suggest_disposition(
            "crates/boon_semantic/src/execution.rs",
            "SemanticExpression",
            "provenance",
            None,
        );
        assert!(
            provenance
                .roles
                .contains(&DependencyRoleV1::ResourceOrProvider)
        );

        let effect = suggest_disposition(
            "crates/boon_semantic/src/execution.rs",
            "SemanticExpression",
            "effect",
            None,
        );
        assert!(
            effect
                .roles
                .contains(&DependencyRoleV1::AssuranceOrActivation)
        );
        let effect_member = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "CheckedEffectSummary",
            "reads_state",
            None,
        );
        assert!(
            effect_member
                .roles
                .contains(&DependencyRoleV1::AssuranceOrActivation)
        );

        let order_purity = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "CheckedOrderKey",
            "pure",
            None,
        );
        assert!(
            order_purity
                .roles
                .contains(&DependencyRoleV1::AssuranceOrActivation)
        );

        let debug_fields = suggest_disposition(
            "crates/boon_semantic/src/program_core.rs",
            "CanonicalProgramCoreV1",
            "debug_fields",
            None,
        );
        assert_eq!(debug_fields.traversal, TraversalV1::Recurse);

        let compiler_output = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "CheckOutput",
            "program",
            None,
        );
        assert!(
            !compiler_output
                .roles
                .contains(&DependencyRoleV1::ResourceOrProvider)
        );

        let type_hints = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "TypeCheckReport",
            "type_hint_table",
            None,
        );
        assert_eq!(type_hints.roles, vec![DependencyRoleV1::DiagnosticOrSource]);
        assert_eq!(type_hints.hash_targets, vec![HashTargetV1::SourceOnly]);
        assert_eq!(type_hints.traversal, TraversalV1::Recurse);

        let presence = suggest_disposition(
            "crates/boon_semantic/src/lib.rs",
            "OutPresenceCompatibilityV1",
            "Present",
            Some(false),
        );
        assert!(
            presence
                .roles
                .contains(&DependencyRoleV1::AssuranceOrActivation)
        );
        let contract = suggest_disposition(
            "crates/boon_semantic/src/lib.rs",
            "OutPortContractV1",
            "flow_type",
            None,
        );
        assert!(
            contract
                .roles
                .contains(&DependencyRoleV1::AssuranceOrActivation)
        );
        let external = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "ExternalTypeEnvironment",
            "functions",
            None,
        );
        assert!(
            external
                .roles
                .contains(&DependencyRoleV1::ResourceOrProvider)
        );
        let hold = suggest_disposition(
            "crates/boon_semantic/src/execution.rs",
            "SemanticStatementKind",
            "Hold",
            Some(true),
        );
        assert!(hold.roles.contains(&DependencyRoleV1::ResourceOrProvider));
        let render_slots = suggest_disposition(
            "crates/boon_typecheck/src/lib.rs",
            "CheckedProgramLoweringMetadata",
            "render_slot_table",
            None,
        );
        assert!(
            render_slots
                .roles
                .contains(&DependencyRoleV1::AssuranceOrActivation)
        );

        for (member, has_fields) in [("MissingProducer", true), ("ports", false)] {
            let diagnostic = suggest_disposition(
                "crates/boon_semantic/src/out_net.rs",
                if member == "ports" {
                    "OutNetDiagnostic::MissingProducer"
                } else {
                    "OutNetDiagnostic"
                },
                member,
                (member == "MissingProducer").then_some(has_fields),
            );
            assert_eq!(diagnostic.roles, vec![DependencyRoleV1::DiagnosticOrSource]);
            assert_eq!(diagnostic.hash_targets, vec![HashTargetV1::SourceOnly]);
        }

        let runtime = suggest_disposition(
            "crates/boon_semantic/src/execution.rs",
            "SemanticValueOrigin",
            "Runtime",
            Some(false),
        );
        assert_eq!(runtime.traversal, TraversalV1::SemanticAtom);
        assert_eq!(runtime.roles, vec![DependencyRoleV1::CoverageOrRouting]);
    }

    #[test]
    #[ignore = "development helper; emits the checked-in registry to stdout"]
    fn emit_workspace_registry() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("xtask crate is under the workspace root");
        let previous_text = fs::read_to_string(workspace.join(REGISTRY_RELATIVE_PATH))
            .expect("checked-in registry is readable");
        let previous =
            toml::from_str::<RegistryV1>(&previous_text).expect("checked-in registry parses");
        let previous_records = previous
            .records
            .into_iter()
            .map(|record| {
                (
                    (record.source().to_owned(), record.name().to_owned()),
                    record,
                )
            })
            .collect::<BTreeMap<_, _>>();
        let previous_dispositions = previous
            .dispositions
            .into_iter()
            .map(|disposition| (disposition.id.clone(), disposition))
            .collect::<BTreeMap<_, _>>();
        let sources = required_source_specs();
        let mut records = Vec::new();
        let mut suggested_dispositions = BTreeMap::new();
        for source in &sources {
            let path = workspace.join(&source.path);
            let text = fs::read_to_string(&path).expect("tracked source is readable");
            let syntax = syn::parse_file(&text).expect("tracked source parses");
            let all = extract_top_level_records(&syntax, &source.path)
                .expect("tracked record schema extracts");
            let selected = select_records(source, &all).expect("source selectors are exact");
            for (name, schema) in selected {
                let suggested = suggest_record(
                    &source.path,
                    name.clone(),
                    schema,
                    &mut suggested_dispositions,
                );
                let previous = previous_records
                    .get(&(source.path.clone(), name.clone()))
                    .or_else(|| {
                        (source.path == "crates/boon_semantic/src/program_core.rs").then(|| {
                            let previous_name = if name == "CanonicalProgramCoreV1" {
                                "ErasedProgramFields"
                            } else {
                                name.as_str()
                            };
                            previous_records.get(&(
                                "crates/boon_ir/src/lib.rs".to_owned(),
                                previous_name.to_owned(),
                            ))
                        })?
                    });
                records.push(merge_reviewed_record(previous, suggested));
            }
        }
        let used_dispositions = records
            .iter()
            .flat_map(record_disposition_ids)
            .collect::<BTreeSet<_>>();
        let dispositions = used_dispositions
            .into_iter()
            .map(|id| {
                previous_dispositions
                    .get(id)
                    .or_else(|| suggested_dispositions.get(id))
                    .unwrap_or_else(|| panic!("record references undefined disposition `{id}`"))
                    .clone()
            })
            .collect();
        let registry = RegistryV1 {
            schema: REGISTRY_SCHEMA_V1.to_owned(),
            sources,
            dispositions,
            records,
        };
        println!("---BEGIN-DEPENDENCY-CLASSIFIER---");
        print!("{}", render_compact_registry(&registry));
        println!("---END-DEPENDENCY-CLASSIFIER---");
    }

    #[test]
    #[ignore = "development helper; emits the canonical checked-in digest"]
    fn emit_workspace_registry_digest() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("xtask crate is under the workspace root");
        let registry_path = workspace.join(REGISTRY_RELATIVE_PATH);
        let registry_text =
            fs::read_to_string(&registry_path).expect("checked-in registry is readable");
        let validated = validate_registry(&registry_text, |relative| {
            fs::read_to_string(workspace.join(relative))
                .map_err(|error| format!("{relative}: {error}"))
        })
        .expect("checked-in registry validates");
        println!(
            "canonical dependency classifier digest: {}",
            validated.digest
        );
    }

    fn merge_reviewed_record(
        previous: Option<&RecordSpecV1>,
        suggested: RecordSpecV1,
    ) -> RecordSpecV1 {
        match (previous, suggested) {
            (
                Some(RecordSpecV1::Struct {
                    fields: previous_fields,
                    ..
                }),
                RecordSpecV1::Struct {
                    source,
                    name,
                    fields,
                },
            ) => {
                let previous_fields = previous_fields
                    .iter()
                    .map(|field| (field.name.as_str(), field))
                    .collect::<BTreeMap<_, _>>();
                RecordSpecV1::Struct {
                    source,
                    name,
                    fields: fields
                        .into_iter()
                        .map(|mut field| {
                            if let Some(previous) = previous_fields.get(field.name.as_str()) {
                                field.disposition = previous.disposition.clone();
                            }
                            field.reviewed = true;
                            field
                        })
                        .collect(),
                }
            }
            (
                Some(RecordSpecV1::Enum {
                    variants: previous_variants,
                    ..
                }),
                RecordSpecV1::Enum {
                    source,
                    name,
                    variants,
                },
            ) => {
                let previous_variants = previous_variants
                    .iter()
                    .map(|variant| (variant.name.as_str(), variant))
                    .collect::<BTreeMap<_, _>>();
                RecordSpecV1::Enum {
                    source,
                    name,
                    variants: variants
                        .into_iter()
                        .map(|mut variant| {
                            if let Some(previous) = previous_variants.get(variant.name.as_str()) {
                                variant.disposition = previous.disposition.clone();
                                let previous_fields = previous
                                    .fields
                                    .iter()
                                    .map(|field| (field.name.as_str(), field))
                                    .collect::<BTreeMap<_, _>>();
                                for field in &mut variant.fields {
                                    if let Some(previous) = previous_fields.get(field.name.as_str())
                                    {
                                        field.disposition = previous.disposition.clone();
                                    }
                                }
                            }
                            variant.reviewed = true;
                            for field in &mut variant.fields {
                                field.reviewed = true;
                            }
                            variant
                        })
                        .collect(),
                }
            }
            (_, mut suggested) => {
                mark_record_reviewed(&mut suggested);
                suggested
            }
        }
    }

    fn mark_record_reviewed(record: &mut RecordSpecV1) {
        match record {
            RecordSpecV1::Struct { fields, .. } => {
                for field in fields {
                    field.reviewed = true;
                }
            }
            RecordSpecV1::Enum { variants, .. } => {
                for variant in variants {
                    variant.reviewed = true;
                    for field in &mut variant.fields {
                        field.reviewed = true;
                    }
                }
            }
        }
    }

    fn record_disposition_ids(record: &RecordSpecV1) -> Vec<&str> {
        match record {
            RecordSpecV1::Struct { fields, .. } => fields
                .iter()
                .map(|field| field.disposition.as_str())
                .collect(),
            RecordSpecV1::Enum { variants, .. } => variants
                .iter()
                .flat_map(|variant| {
                    std::iter::once(variant.disposition.as_str()).chain(
                        variant
                            .fields
                            .iter()
                            .map(|field| field.disposition.as_str()),
                    )
                })
                .collect(),
        }
    }

    fn render_compact_registry(registry: &RegistryV1) -> String {
        let mut output = format!("schema = {:?}\n\n", registry.schema);
        for source in &registry.sources {
            output.push_str("[[sources]]\n");
            output.push_str(&format!("path = {:?}\n", source.path));
            output.push_str(&format!(
                "selection = {:?}\n",
                json_label(&source.selection)
            ));
            output.push_str(&format!("prefixes = {}\n", string_array(&source.prefixes)));
            output.push_str(&format!(
                "explicit_types = {}\n",
                string_array(&source.explicit_types)
            ));
            output.push_str(&format!(
                "additional_types = {}\n\n",
                string_array(&source.additional_types)
            ));
        }
        for disposition in &registry.dispositions {
            output.push_str("[[dispositions]]\n");
            output.push_str(&format!("id = {:?}\n", disposition.id));
            output.push_str(&format!(
                "traversal = {:?}\n",
                json_label(&disposition.traversal)
            ));
            output.push_str(&format!("roles = {}\n", enum_array(&disposition.roles)));
            output.push_str(&format!(
                "visibility = {:?}\n",
                json_label(&disposition.visibility)
            ));
            output.push_str(&format!(
                "hash_targets = {}\n",
                enum_array(&disposition.hash_targets)
            ));
            output.push_str(&format!(
                "erasure = {:?}\n\n",
                json_label(&disposition.erasure)
            ));
        }
        for record in &registry.records {
            output.push_str("[[records]]\n");
            output.push_str(&format!(
                "kind = {:?}\n",
                match record {
                    RecordSpecV1::Struct { .. } => "struct",
                    RecordSpecV1::Enum { .. } => "enum",
                }
            ));
            output.push_str(&format!("source = {:?}\n", record.source()));
            output.push_str(&format!("name = {:?}\n", record.name()));
            match record {
                RecordSpecV1::Struct { fields, .. } => {
                    render_fields(&mut output, fields);
                }
                RecordSpecV1::Enum { variants, .. } => {
                    output.push('\n');
                    for variant in variants {
                        output.push_str("[[records.variants]]\n");
                        output.push_str(&format!("name = {:?}\n", variant.name));
                        output.push_str(&format!("disposition = {:?}\n", variant.disposition));
                        output.push_str(&format!("reviewed = {}\n", variant.reviewed));
                        render_fields(&mut output, &variant.fields);
                    }
                }
            }
        }
        output
    }

    fn render_fields(output: &mut String, fields: &[FieldDispositionV1]) {
        output.push_str("fields = [\n");
        for field in fields {
            output.push_str(&format!(
                "  {{ name = {:?}, disposition = {:?}, reviewed = {} }},\n",
                field.name, field.disposition, field.reviewed
            ));
        }
        output.push_str("]\n\n");
    }

    fn string_array(values: &[String]) -> String {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn enum_array<T: Serialize>(values: &[T]) -> String {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| format!("{:?}", json_label(value)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    fn json_label(value: &impl Serialize) -> String {
        serde_json::to_string(value)
            .expect("classifier enum serializes")
            .trim_matches('"')
            .to_owned()
    }

    fn suggest_record(
        source: &str,
        name: String,
        schema: RustRecordSchema,
        dispositions: &mut BTreeMap<String, DispositionSpecV1>,
    ) -> RecordSpecV1 {
        match schema {
            RustRecordSchema::Struct { fields } => RecordSpecV1::Struct {
                source: source.to_owned(),
                name: name.clone(),
                fields: fields
                    .into_iter()
                    .map(|field| suggest_field(source, &name, field, dispositions))
                    .collect(),
            },
            RustRecordSchema::Enum { variants } => RecordSpecV1::Enum {
                source: source.to_owned(),
                name: name.clone(),
                variants: variants
                    .into_iter()
                    .map(|variant| {
                        let has_fields = !variant.fields.is_empty();
                        let variant_disposition = intern_disposition(
                            suggest_disposition(source, &name, &variant.name, Some(has_fields)),
                            dispositions,
                        );
                        let fields = variant
                            .fields
                            .into_iter()
                            .map(|field| {
                                suggest_field(
                                    source,
                                    &format!("{name}::{}", variant.name),
                                    field,
                                    dispositions,
                                )
                            })
                            .collect();
                        VariantDispositionV1 {
                            name: variant.name,
                            disposition: variant_disposition,
                            reviewed: false,
                            fields,
                        }
                    })
                    .collect(),
            },
        }
    }

    fn suggest_field(
        source: &str,
        record: &str,
        name: String,
        dispositions: &mut BTreeMap<String, DispositionSpecV1>,
    ) -> FieldDispositionV1 {
        let disposition = intern_disposition(
            suggest_disposition(source, record, &name, None),
            dispositions,
        );
        FieldDispositionV1 {
            name,
            disposition,
            reviewed: false,
        }
    }

    fn intern_disposition(
        mut disposition: DispositionSpecV1,
        dispositions: &mut BTreeMap<String, DispositionSpecV1>,
    ) -> String {
        disposition.id = disposition_id(&disposition);
        if let Some(previous) = dispositions.insert(disposition.id.clone(), disposition.clone()) {
            assert_eq!(
                previous, disposition,
                "generated disposition IDs are collision-free"
            );
        }
        disposition.id
    }

    fn suggest_disposition(
        source: &str,
        record: &str,
        member: &str,
        variant_has_fields: Option<bool>,
    ) -> DispositionSpecV1 {
        let is_variant = variant_has_fields.is_some();
        let intentionally_nonsemantic = record == "TypeCheckProfile"
            || record == "TypeVarStore"
            || (record.ends_with("Error") && member == "message");
        if intentionally_nonsemantic {
            return DispositionSpecV1 {
                id: String::new(),
                traversal: TraversalV1::None,
                roles: vec![DependencyRoleV1::IntentionallyNonsemantic],
                visibility: if record == "TypeVarStore" {
                    DependencyVisibilityV1::Private
                } else {
                    DependencyVisibilityV1::Diagnostic
                },
                hash_targets: vec![HashTargetV1::None],
                erasure: if source.contains("boon_verify") {
                    ErasurePolicyV1::EraseAfterVerification
                } else {
                    ErasurePolicyV1::NormalizeThenErase
                },
            };
        }
        let is_unknown = is_variant
            && matches!(
                member,
                "Invalid" | "Unknown" | "UnresolvedShape" | "Unsupported"
            );
        if is_unknown {
            return DispositionSpecV1 {
                id: String::new(),
                traversal: TraversalV1::None,
                roles: vec![DependencyRoleV1::ForbiddenInVerifiedSlice],
                visibility: DependencyVisibilityV1::Private,
                hash_targets: vec![HashTargetV1::ImplementationDependency],
                erasure: ErasurePolicyV1::Reject,
            };
        }
        if record.ends_with("::Invalid")
            || record.ends_with("::Unknown")
            || record.ends_with("::UnresolvedShape")
            || record.ends_with("::Unsupported")
        {
            return DispositionSpecV1 {
                id: String::new(),
                traversal: TraversalV1::None,
                roles: vec![DependencyRoleV1::DiagnosticOrSource],
                visibility: DependencyVisibilityV1::Diagnostic,
                hash_targets: vec![HashTargetV1::SourceOnly],
                erasure: ErasurePolicyV1::NormalizeThenErase,
            };
        }
        if record == "SemanticValueOrigin"
            && member == "Runtime"
            && variant_has_fields == Some(false)
        {
            return DispositionSpecV1 {
                id: String::new(),
                traversal: TraversalV1::SemanticAtom,
                roles: vec![DependencyRoleV1::CoverageOrRouting],
                visibility: DependencyVisibilityV1::Private,
                hash_targets: vec![
                    HashTargetV1::SemanticProgram,
                    HashTargetV1::ImplementationDependency,
                    HashTargetV1::ProofContext,
                ],
                erasure: ErasurePolicyV1::NormalizeThenErase,
            };
        }

        let member_is_resource_identity = contains_identifier_word(
            member,
            &[
                "alias",
                "authority",
                "external",
                "host",
                "hold",
                "list",
                "materialization",
                "memory",
                "migration",
                "out",
                "origin",
                "owner",
                "passed",
                "port",
                "producer",
                "provenance",
                "provider",
                "resource",
                "row",
                "source",
                "state",
            ],
        );
        let diagnostic_record = contains_identifier_word(record, &["diagnostic"])
            || record.starts_with("TypeDisplay")
            || record.starts_with("TypeHint");
        let diagnostic_member = contains_identifier_word(
            member,
            &[
                "diagnostic",
                "display",
                "error",
                "message",
                "reason",
                "summary",
                "hint",
                "token",
            ],
        );
        let source_position = contains_identifier_word(member, &["end", "line", "span", "start"]);
        let parameter_semantics = record.starts_with("CheckedParameter");
        let assurance = source.contains("boon_verify")
            || contains_identifier_word(
                record,
                &[
                    "assurance",
                    "compatibility",
                    "contract",
                    "coverage",
                    "default",
                    "digest",
                    "effect",
                    "hash",
                    "manifest",
                    "policy",
                    "presence",
                    "readiness",
                    "render",
                    "requirement",
                    "reuse",
                    "status",
                    "theorem",
                    "verified",
                ],
            )
            || contains_identifier_word(
                member,
                &[
                    "assurance",
                    "ambiguity",
                    "contract",
                    "coverage",
                    "digest",
                    "effect",
                    "evidence",
                    "failure",
                    "fallback",
                    "hash",
                    "hashes",
                    "gated",
                    "manifest",
                    "obligation",
                    "policy",
                    "presence",
                    "profile",
                    "proof",
                    "pure",
                    "readiness",
                    "render",
                    "required",
                    "requirement",
                    "reuse",
                    "schema",
                    "status",
                    "theorem",
                    "total",
                    "unresolved",
                    "verified",
                    "version",
                ],
            );
        let pure_diagnostic = !source.contains("boon_verify")
            && !assurance
            && (diagnostic_record || (diagnostic_member && !member_is_resource_identity));
        let resource_record = record != "CheckOutput"
            && contains_identifier_word(
                record,
                &[
                    "alias",
                    "authority",
                    "external",
                    "host",
                    "hold",
                    "list",
                    "materialization",
                    "memory",
                    "migration",
                    "out",
                    "output",
                    "origin",
                    "owner",
                    "passed",
                    "port",
                    "producer",
                    "provenance",
                    "resource",
                    "row",
                    "source",
                    "state",
                ],
            );
        let resource = resource_record || member_is_resource_identity;
        let resource = resource
            || record == "CheckedSemanticPath"
            || (is_variant
                && contains_identifier_word(record, &["scope"])
                && contains_identifier_word(member, &["output"]));
        let formula_binding_record = contains_identifier_word(
            record,
            &[
                "argument",
                "binding",
                "call",
                "callable",
                "capture",
                "constant",
                "constraint",
                "contract",
                "context",
                "declaration",
                "derived",
                "evaluation",
                "execution",
                "expression",
                "field",
                "function",
                "local",
                "operation",
                "order",
                "parameter",
                "read",
                "select",
                "shape",
                "signature",
                "statement",
                "type",
                "value",
                "variant",
            ],
        );
        let formula_name = member == "name"
            && contains_identifier_word(
                record,
                &[
                    "argument",
                    "binding",
                    "callable",
                    "capture",
                    "field",
                    "function",
                    "parameter",
                    "symbol",
                    "variant",
                ],
            );
        let formula_record_variant = is_variant
            && contains_identifier_word(
                record,
                &[
                    "callable",
                    "expression",
                    "flow",
                    "initializer",
                    "pattern",
                    "text",
                    "type",
                    "value",
                ],
            );
        let formula_variant_field = !is_variant
            && record.contains("::")
            && contains_identifier_word(
                record,
                &[
                    "callable",
                    "expression",
                    "flow",
                    "initializer",
                    "pattern",
                    "text",
                    "type",
                    "value",
                ],
            );
        let formula = formula_name
            || formula_record_variant
            || formula_variant_field
            || ((parameter_semantics || formula_binding_record) && !source_position)
            || contains_identifier_word(
                member,
                &[
                    "arg",
                    "argument",
                    "binder",
                    "body",
                    "binding",
                    "call",
                    "callable",
                    "condition",
                    "constant",
                    "constraint",
                    "contract",
                    "constructor",
                    "default",
                    "declaration",
                    "expr",
                    "expression",
                    "field",
                    "flow",
                    "formal",
                    "formula",
                    "function",
                    "initial",
                    "input",
                    "item",
                    "left",
                    "literal",
                    "local",
                    "order",
                    "operation",
                    "parameter",
                    "pattern",
                    "predicate",
                    "profile",
                    "program",
                    "projection",
                    "read",
                    "result",
                    "right",
                    "root",
                    "row",
                    "selector",
                    "statement",
                    "tag",
                    "ty",
                    "type",
                    "value",
                ],
            );

        if pure_diagnostic {
            let recurse_diagnostic = member == "call_sites"
                || member == "diagnostic_spans"
                || member == "display_tree"
                || member == "result"
                || member == "ty"
                || member.ends_with('s')
                || contains_identifier_word(
                    member,
                    &[
                        "anchor",
                        "arg",
                        "declaration",
                        "diagnostic",
                        "entry",
                        "field",
                        "item",
                        "table",
                        "token",
                        "variant",
                    ],
                );
            return DispositionSpecV1 {
                id: String::new(),
                traversal: match variant_has_fields {
                    Some(true) => TraversalV1::Recurse,
                    Some(false) => TraversalV1::SemanticAtom,
                    None if recurse_diagnostic => TraversalV1::Recurse,
                    None => TraversalV1::SemanticAtom,
                },
                roles: vec![DependencyRoleV1::DiagnosticOrSource],
                visibility: DependencyVisibilityV1::Diagnostic,
                hash_targets: vec![HashTargetV1::SourceOnly],
                erasure: if source.contains("boon_verify") {
                    ErasurePolicyV1::EraseAfterVerification
                } else {
                    ErasurePolicyV1::NormalizeThenErase
                },
            };
        }

        let mut roles = BTreeSet::from([DependencyRoleV1::CoverageOrRouting]);
        if formula {
            roles.insert(DependencyRoleV1::FormulaBinder);
        }
        if resource {
            roles.insert(DependencyRoleV1::ResourceOrProvider);
        }
        if assurance {
            roles.insert(DependencyRoleV1::AssuranceOrActivation);
        }
        if source_position {
            roles.insert(DependencyRoleV1::DiagnosticOrSource);
        }

        let public_contract = source.contains("boon_document_model")
            || (source.contains("boon_verify")
                && !record.contains("DigestPayload")
                && !record.contains("VerifyError"));
        let visibility = if source_position {
            DependencyVisibilityV1::Diagnostic
        } else if public_contract {
            DependencyVisibilityV1::Public
        } else {
            DependencyVisibilityV1::Private
        };

        let hash_targets = if source.contains("boon_document_model") {
            vec![HashTargetV1::PublicStatement, HashTargetV1::SemanticProgram]
        } else if source.contains("boon_verify") {
            vec![
                HashTargetV1::ProofContext,
                HashTargetV1::Obligation,
                HashTargetV1::EvidenceCache,
            ]
        } else if source.contains("boon_ir") {
            vec![
                HashTargetV1::ImplementationDependency,
                HashTargetV1::ProofContext,
            ]
        } else if source.contains("boon_semantic") {
            vec![
                HashTargetV1::SemanticProgram,
                HashTargetV1::ImplementationDependency,
                HashTargetV1::ProofContext,
            ]
        } else {
            vec![
                HashTargetV1::SemanticProgram,
                HashTargetV1::ImplementationDependency,
            ]
        };

        let tuple_newtype_atom = member == "0"
            && !record.contains("::")
            && contains_identifier_word(record, &["digest", "id", "var"]);
        let path_collection =
            member == "call_path" || member == "paths" || member.ends_with("_paths");
        let scalar_identity = tuple_newtype_atom
            || source_position
            || (!path_collection
                && contains_identifier_word(
                    member,
                    &[
                        "capacity",
                        "count",
                        "digest",
                        "hash",
                        "id",
                        "identity",
                        "interval",
                        "module",
                        "name",
                        "open",
                        "ordinal",
                        "path",
                        "profile",
                        "published",
                        "schema",
                        "scoped",
                        "spread",
                    ],
                ));
        DispositionSpecV1 {
            id: String::new(),
            traversal: match variant_has_fields {
                Some(false) => TraversalV1::SemanticAtom,
                Some(true) => TraversalV1::Recurse,
                None if scalar_identity => TraversalV1::SemanticAtom,
                None => TraversalV1::Recurse,
            },
            roles: roles.into_iter().collect(),
            visibility,
            hash_targets,
            erasure: if source.contains("boon_verify") {
                ErasurePolicyV1::EraseAfterVerification
            } else if source.contains("boon_document_model") {
                ErasurePolicyV1::PreserveSemantic
            } else {
                ErasurePolicyV1::NormalizeThenErase
            },
        }
    }

    fn disposition_id(disposition: &DispositionSpecV1) -> String {
        let traversal = match disposition.traversal {
            TraversalV1::Recurse => "r",
            TraversalV1::SemanticAtom => "a",
            TraversalV1::None => "n",
        };
        let roles = disposition
            .roles
            .iter()
            .map(|role| match role {
                DependencyRoleV1::FormulaBinder => "f",
                DependencyRoleV1::ResourceOrProvider => "r",
                DependencyRoleV1::CoverageOrRouting => "c",
                DependencyRoleV1::AssuranceOrActivation => "a",
                DependencyRoleV1::DiagnosticOrSource => "d",
                DependencyRoleV1::IntentionallyNonsemantic => "i",
                DependencyRoleV1::ForbiddenInVerifiedSlice => "x",
            })
            .collect::<String>();
        let visibility = match disposition.visibility {
            DependencyVisibilityV1::Public => "pub",
            DependencyVisibilityV1::Private => "priv",
            DependencyVisibilityV1::Diagnostic => "diag",
        };
        let hashes = disposition
            .hash_targets
            .iter()
            .map(|target| match target {
                HashTargetV1::PublicStatement => "ps",
                HashTargetV1::SemanticProgram => "sp",
                HashTargetV1::ImplementationDependency => "id",
                HashTargetV1::ProofContext => "pc",
                HashTargetV1::Obligation => "ob",
                HashTargetV1::EvidenceCache => "ec",
                HashTargetV1::SourceOnly => "so",
                HashTargetV1::None => "n",
            })
            .collect::<Vec<_>>()
            .join("-");
        let erasure = match disposition.erasure {
            ErasurePolicyV1::PreserveSemantic => "p",
            ErasurePolicyV1::NormalizeThenErase => "n",
            ErasurePolicyV1::EraseAfterVerification => "e",
            ErasurePolicyV1::Reject => "x",
        };
        format!("{traversal}-{roles}-{visibility}-{hashes}-{erasure}")
    }

    fn contains_identifier_word(value: &str, needles: &[&str]) -> bool {
        let characters = value.chars().collect::<Vec<_>>();
        let mut words = Vec::new();
        let mut current = String::new();
        for (index, character) in characters.iter().copied().enumerate() {
            if !character.is_ascii_alphanumeric() {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
                continue;
            }
            let previous = index
                .checked_sub(1)
                .and_then(|previous| characters.get(previous))
                .copied();
            let next = characters.get(index + 1).copied();
            let starts_word = character.is_ascii_uppercase()
                && !current.is_empty()
                && (previous.is_some_and(|previous| {
                    previous.is_ascii_lowercase() || previous.is_ascii_digit()
                }) || (previous.is_some_and(|previous| previous.is_ascii_uppercase())
                    && next.is_some_and(|next| next.is_ascii_lowercase())));
            if starts_word {
                words.push(std::mem::take(&mut current));
            }
            current.push(character.to_ascii_lowercase());
        }
        if !current.is_empty() {
            words.push(current);
        }
        needles.iter().any(|needle| {
            words.iter().any(|word| {
                word == needle
                    || word.strip_suffix('s').is_some_and(|word| word == *needle)
                    || word.strip_suffix("es").is_some_and(|word| word == *needle)
                    || word
                        .strip_suffix("ies")
                        .is_some_and(|word| format!("{word}y") == *needle)
            })
        })
    }
}
