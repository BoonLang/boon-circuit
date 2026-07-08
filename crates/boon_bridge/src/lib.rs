use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

pub const BRIDGE_ABI_VERSION: &str = "boon-bridge-abi-0.1";
pub const CANONICAL_SCHEMA_VERSION: u32 = 1;

pub type BridgeResult<T> = Result<T, BridgeError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeError {
    pub code: BridgeErrorCode,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: BridgeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for BridgeError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeErrorCode {
    MissingModule,
    MissingExport,
    WrongExportKind,
    SchemaMismatch,
    DuplicateExport,
    DuplicateCompletion,
    StaleCompletion,
    Canceled,
    GrantDenied,
    PayloadCapExceeded,
    RustHandleLeak,
    ReplayMiss,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeExportKind {
    Pure,
    Effect,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeCapability {
    pub name: String,
    pub grant_id: String,
}

impl BridgeCapability {
    pub fn new(name: impl Into<String>, grant_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            grant_id: grant_id.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeProviderMetadata {
    pub provider: String,
    pub provider_version: String,
    pub bridge_crate: String,
    pub bridge_crate_version: String,
    pub features: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeSchema {
    pub name: String,
    pub version: u32,
    pub shape: BridgeSchemaShape,
}

impl BridgeSchema {
    pub fn hash(&self) -> String {
        canonical_hash(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BridgeSchemaShape {
    Null,
    Bool,
    Int,
    Decimal,
    Text,
    Bytes,
    List {
        item: Box<BridgeSchemaShape>,
    },
    Record {
        fields: BTreeMap<String, BridgeSchemaShape>,
    },
    Tagged {
        variants: BTreeMap<String, BridgeSchemaShape>,
    },
    Result {
        ok: Box<BridgeSchemaShape>,
        err: Box<BridgeSchemaShape>,
    },
    Diagnostic,
    BlobRef,
    ArtifactRef,
    PageRef,
    Completion {
        output: Box<BridgeSchemaShape>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum BridgeValue {
    Null,
    Bool(bool),
    Int(i128),
    Decimal(String),
    Text(String),
    Bytes {
        digest: String,
        byte_len: u64,
        #[serde(with = "bridge_bytes_serde")]
        bytes: Bytes,
    },
    List(Vec<BridgeValue>),
    Record(BTreeMap<String, BridgeValue>),
    Tagged {
        tag: String,
        value: Box<BridgeValue>,
    },
    Result {
        ok: bool,
        value: Box<BridgeValue>,
    },
    Diagnostic(BridgeDiagnostic),
    BlobRef(BridgeBlobRef),
    ArtifactRef(BridgeArtifactRef),
    PageRef(BridgePageRef),
}

impl BridgeValue {
    pub fn inline_bytes(digest: impl Into<String>, bytes: impl Into<Bytes>) -> Self {
        let bytes = bytes.into();
        Self::Bytes {
            digest: digest.into(),
            byte_len: bytes.len() as u64,
            bytes,
        }
    }
}

mod bridge_bytes_serde {
    use bytes::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &Bytes, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<u8>::deserialize(deserializer).map(Bytes::from)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeDiagnostic {
    pub code: String,
    pub message: String,
    pub severity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeBlobRef {
    pub digest: String,
    pub byte_len: u64,
    pub media_type: String,
    pub storage: String,
    pub encoding: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeArtifactRef {
    pub kind: String,
    pub provider: String,
    pub provider_version: String,
    pub bridge_module: String,
    pub contract_version: String,
    pub identity: String,
    pub locator: BTreeMap<String, String>,
    pub reproducibility: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgePageRef {
    pub artifact_digest: String,
    pub schema_version: u32,
    pub schema_hash: String,
    pub request_fingerprint: String,
    pub response_fingerprint: String,
    pub input_digest: String,
    pub page_digest: String,
    pub generation: u64,
    pub offset: u64,
    pub limit: u64,
    pub row_count: u64,
    pub sample_count: u64,
    pub transition_count: u64,
    pub byte_length: u64,
    pub byte_len: u64,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeExportMetadata {
    pub name: String,
    pub kind: BridgeExportKind,
    pub input_schema_version: u32,
    pub input_schema_hash: String,
    pub output_schema_version: u32,
    pub output_schema_hash: String,
    pub required_capabilities: Vec<BridgeCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeModuleMetadata {
    pub module: String,
    pub abi_version: String,
    pub canonical_schema_version: u32,
    pub provider: BridgeProviderMetadata,
    pub exports: BTreeMap<String, BridgeExportMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeExportSchemas {
    pub input: BridgeSchema,
    pub output: BridgeSchema,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeRegistry {
    modules: BTreeMap<String, BridgeModuleMetadata>,
    #[serde(skip)]
    export_schemas: BTreeMap<(String, String), BridgeExportSchemas>,
}

impl BridgeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_module(&mut self, module: BridgeModuleMetadata) -> BridgeResult<()> {
        if self.modules.contains_key(&module.module) {
            return Err(BridgeError::new(
                BridgeErrorCode::DuplicateExport,
                format!("bridge module `{}` is already registered", module.module),
            ));
        }
        self.modules.insert(module.module.clone(), module);
        Ok(())
    }

    pub fn register_export_schemas(
        &mut self,
        module: &str,
        export: &str,
        input: BridgeSchema,
        output: BridgeSchema,
    ) -> BridgeResult<()> {
        let metadata = self.export(module, export)?;
        if metadata.input_schema_version != input.version
            || metadata.input_schema_hash != input.hash()
            || metadata.output_schema_version != output.version
            || metadata.output_schema_hash != output.hash()
        {
            return Err(BridgeError::new(
                BridgeErrorCode::SchemaMismatch,
                format!("bridge export `{module}.{export}` schema shape does not match metadata"),
            ));
        }
        self.export_schemas.insert(
            (module.to_owned(), export.to_owned()),
            BridgeExportSchemas { input, output },
        );
        Ok(())
    }

    pub fn module(&self, module: &str) -> Option<&BridgeModuleMetadata> {
        self.modules.get(module)
    }

    pub fn modules(&self) -> &BTreeMap<String, BridgeModuleMetadata> {
        &self.modules
    }

    pub fn export_schemas(&self, module: &str, export: &str) -> Option<&BridgeExportSchemas> {
        self.export_schemas
            .get(&(module.to_owned(), export.to_owned()))
    }

    pub fn export(&self, module: &str, export: &str) -> BridgeResult<&BridgeExportMetadata> {
        self.modules
            .get(module)
            .ok_or_else(|| {
                BridgeError::new(
                    BridgeErrorCode::MissingModule,
                    format!("bridge module `{module}` is not registered"),
                )
            })?
            .exports
            .get(export)
            .ok_or_else(|| {
                BridgeError::new(
                    BridgeErrorCode::MissingExport,
                    format!("bridge export `{module}.{export}` is not registered"),
                )
            })
    }

    pub fn validate_import(
        &self,
        module: &str,
        export: &str,
        expected_kind: BridgeExportKind,
        input_schema_hash: &str,
        output_schema_hash: &str,
    ) -> BridgeResult<&BridgeExportMetadata> {
        let metadata = self.export(module, export)?;
        if metadata.kind != expected_kind {
            return Err(BridgeError::new(
                BridgeErrorCode::WrongExportKind,
                format!("bridge export `{module}.{export}` has wrong kind"),
            ));
        }
        if metadata.input_schema_hash != input_schema_hash
            || metadata.output_schema_hash != output_schema_hash
        {
            return Err(BridgeError::new(
                BridgeErrorCode::SchemaMismatch,
                format!("bridge export `{module}.{export}` schema hash changed"),
            ));
        }
        Ok(metadata)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeTaskRequest {
    pub module: String,
    pub export: String,
    pub request_id: String,
    pub request_key: String,
    pub request_epoch: u64,
    pub input_schema_hash: String,
    pub output_schema_hash: String,
    pub input_digest: String,
    pub input: BridgeValue,
    pub capability_grants: Vec<String>,
    pub cancellation_key: String,
    pub cancellation_epoch: u64,
}

impl BridgeTaskRequest {
    pub fn new(
        export: &BridgeExportMetadata,
        module: impl Into<String>,
        request_id: impl Into<String>,
        request_epoch: u64,
        input: BridgeValue,
        capability_grants: Vec<String>,
        cancellation_key: impl Into<String>,
        cancellation_epoch: u64,
    ) -> Self {
        let module = module.into();
        let request_id = request_id.into();
        let cancellation_key = cancellation_key.into();
        let input_digest = canonical_hash(&input);
        let request_key = canonical_hash(&json!({
            "module": module,
            "export": export.name,
            "input_schema_hash": export.input_schema_hash,
            "input_digest": input_digest,
            "capability_grants": capability_grants,
            "cancellation_key": cancellation_key
        }));
        Self {
            module,
            export: export.name.clone(),
            request_id,
            request_key,
            request_epoch,
            input_schema_hash: export.input_schema_hash.clone(),
            output_schema_hash: export.output_schema_hash.clone(),
            input_digest,
            input,
            capability_grants,
            cancellation_key,
            cancellation_epoch,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeCompletionStatus {
    Ok,
    Diagnostic,
    Canceled,
    Timeout,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeTaskCompletion {
    pub request_id: String,
    pub request_key: String,
    pub request_epoch: u64,
    pub status: BridgeCompletionStatus,
    pub output_schema_hash: String,
    pub input_digest: String,
    pub cancellation_epoch: u64,
    pub output: Option<BridgeValue>,
    pub diagnostics: Vec<BridgeDiagnostic>,
}

impl BridgeTaskCompletion {
    pub fn for_request(
        request: &BridgeTaskRequest,
        status: BridgeCompletionStatus,
        output: Option<BridgeValue>,
        diagnostics: Vec<BridgeDiagnostic>,
    ) -> Self {
        Self {
            request_id: request.request_id.clone(),
            request_key: request.request_key.clone(),
            request_epoch: request.request_epoch,
            status,
            output_schema_hash: request.output_schema_hash.clone(),
            input_digest: request.input_digest.clone(),
            cancellation_epoch: request.cancellation_epoch,
            output,
            diagnostics,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeScheduleOutcome {
    Scheduled,
    Deduplicated,
    Replayed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BridgeScheduleResult {
    pub outcome: BridgeScheduleOutcome,
    pub request_key: String,
    pub replayed_completion: Option<BridgeTaskCompletion>,
}

#[derive(Clone, Debug, Default)]
pub struct BridgeEffectScheduler {
    live: BTreeMap<String, BridgeTaskRequest>,
    live_output_shapes: BTreeMap<String, BridgeSchemaShape>,
    completed: BTreeSet<String>,
    canceled: BTreeMap<String, u64>,
    replay: BTreeMap<String, BridgeTaskCompletion>,
    max_inline_bytes: u64,
}

impl BridgeEffectScheduler {
    pub fn new(max_inline_bytes: u64) -> Self {
        Self {
            max_inline_bytes,
            ..Self::default()
        }
    }

    pub fn with_replay(max_inline_bytes: u64, completions: Vec<BridgeTaskCompletion>) -> Self {
        let mut scheduler = Self::new(max_inline_bytes);
        scheduler.replay = completions
            .into_iter()
            .map(|completion| (completion.request_key.clone(), completion))
            .collect();
        scheduler
    }

    pub fn schedule(
        &mut self,
        registry: &BridgeRegistry,
        request: BridgeTaskRequest,
    ) -> BridgeResult<BridgeScheduleResult> {
        let export = registry.validate_import(
            &request.module,
            &request.export,
            BridgeExportKind::Effect,
            &request.input_schema_hash,
            &request.output_schema_hash,
        )?;
        validate_no_rust_handles(&request.input)?;
        validate_payload_cap(&request.input, self.max_inline_bytes)?;
        let grants = request
            .capability_grants
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if let Some(missing) = export
            .required_capabilities
            .iter()
            .find(|capability| !grants.contains(&capability.grant_id))
        {
            return Err(BridgeError::new(
                BridgeErrorCode::GrantDenied,
                format!(
                    "bridge export `{}.{}` requires capability grant `{}`",
                    request.module, request.export, missing.grant_id
                ),
            ));
        }
        let output_shape =
            if let Some(schemas) = registry.export_schemas(&request.module, &request.export) {
                validate_bridge_value_shape(&request.input, &schemas.input.shape)?;
                Some(schemas.output.shape.clone())
            } else {
                None
            };
        if let Some(replayed_completion) = self.replay.get(&request.request_key).cloned() {
            validate_replayed_completion(
                &request,
                output_shape.as_ref(),
                &replayed_completion,
                self.max_inline_bytes,
            )?;
            return Ok(BridgeScheduleResult {
                outcome: BridgeScheduleOutcome::Replayed,
                request_key: request.request_key,
                replayed_completion: Some(replayed_completion),
            });
        }
        if self.live.contains_key(&request.request_key) {
            return Ok(BridgeScheduleResult {
                outcome: BridgeScheduleOutcome::Deduplicated,
                request_key: request.request_key,
                replayed_completion: None,
            });
        }
        let request_key = request.request_key.clone();
        self.live.insert(request_key.clone(), request);
        if let Some(output_shape) = output_shape {
            self.live_output_shapes
                .insert(request_key.clone(), output_shape);
        }
        Ok(BridgeScheduleResult {
            outcome: BridgeScheduleOutcome::Scheduled,
            request_key,
            replayed_completion: None,
        })
    }

    pub fn cancel(&mut self, request_key: &str, cancellation_epoch: u64) -> BridgeResult<()> {
        let request = self.live.get(request_key).ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::StaleCompletion,
                format!("cannot cancel unknown bridge request `{request_key}`"),
            )
        })?;
        if cancellation_epoch < request.cancellation_epoch {
            return Err(BridgeError::new(
                BridgeErrorCode::StaleCompletion,
                "cancellation epoch is older than the live request",
            ));
        }
        self.canceled
            .insert(request_key.to_owned(), cancellation_epoch);
        Ok(())
    }

    pub fn complete(
        &mut self,
        completion: BridgeTaskCompletion,
    ) -> BridgeResult<BridgeTaskCompletion> {
        self.complete_inner(completion, None)
    }

    pub fn complete_with_payloads(
        &mut self,
        completion: BridgeTaskCompletion,
        payloads: &BridgeCompletionPayloads,
    ) -> BridgeResult<BridgeTaskCompletion> {
        self.complete_inner(completion, Some(payloads))
    }

    fn complete_inner(
        &mut self,
        completion: BridgeTaskCompletion,
        payloads: Option<&BridgeCompletionPayloads>,
    ) -> BridgeResult<BridgeTaskCompletion> {
        if self.completed.contains(&completion.request_key) {
            return Err(BridgeError::new(
                BridgeErrorCode::DuplicateCompletion,
                format!(
                    "bridge completion for `{}` was already accepted",
                    completion.request_key
                ),
            ));
        }
        let request = self.live.get(&completion.request_key).ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::StaleCompletion,
                format!(
                    "bridge completion for `{}` has no live request",
                    completion.request_key
                ),
            )
        })?;
        if completion.request_id != request.request_id
            || completion.request_epoch != request.request_epoch
            || completion.input_digest != request.input_digest
            || completion.output_schema_hash != request.output_schema_hash
        {
            return Err(BridgeError::new(
                BridgeErrorCode::StaleCompletion,
                "bridge completion metadata no longer matches the live request",
            ));
        }
        if let Some(canceled_epoch) = self.canceled.get(&completion.request_key).copied() {
            if !matches!(completion.status, BridgeCompletionStatus::Canceled)
                || completion.cancellation_epoch != canceled_epoch
            {
                return Err(BridgeError::new(
                    BridgeErrorCode::Canceled,
                    "bridge completion arrived after cancellation",
                ));
            }
        } else if completion.cancellation_epoch != request.cancellation_epoch {
            return Err(BridgeError::new(
                BridgeErrorCode::StaleCompletion,
                "bridge completion cancellation epoch changed",
            ));
        }
        let output_shape = self.live_output_shapes.get(&completion.request_key);
        validate_completion_payload_and_shape(self.max_inline_bytes, output_shape, &completion)?;
        if let Some(payloads) = payloads {
            validate_completion_payload_refs(&completion, payloads)?;
        }
        self.live.remove(&completion.request_key);
        self.live_output_shapes.remove(&completion.request_key);
        self.canceled.remove(&completion.request_key);
        self.completed.insert(completion.request_key.clone());
        Ok(completion)
    }
}

fn validate_replayed_completion(
    request: &BridgeTaskRequest,
    output_shape: Option<&BridgeSchemaShape>,
    completion: &BridgeTaskCompletion,
    max_inline_bytes: u64,
) -> BridgeResult<()> {
    validate_completion_metadata(request, completion)?;
    validate_completion_payload_and_shape(max_inline_bytes, output_shape, completion)
}

fn validate_completion_metadata(
    request: &BridgeTaskRequest,
    completion: &BridgeTaskCompletion,
) -> BridgeResult<()> {
    if completion.request_id != request.request_id
        || completion.request_epoch != request.request_epoch
        || completion.input_digest != request.input_digest
        || completion.output_schema_hash != request.output_schema_hash
        || completion.cancellation_epoch != request.cancellation_epoch
    {
        return Err(BridgeError::new(
            BridgeErrorCode::StaleCompletion,
            "bridge completion metadata no longer matches the request",
        ));
    }
    Ok(())
}

fn validate_completion_payload_and_shape(
    max_inline_bytes: u64,
    output_shape: Option<&BridgeSchemaShape>,
    completion: &BridgeTaskCompletion,
) -> BridgeResult<()> {
    match completion.output.as_ref() {
        Some(output) => {
            validate_no_rust_handles(output)?;
            validate_payload_cap(output, max_inline_bytes)?;
            if let Some(shape) = output_shape {
                validate_bridge_value_shape(output, shape)?;
            }
            Ok(())
        }
        None if matches!(completion.status, BridgeCompletionStatus::Ok)
            && output_shape.is_some() =>
        {
            Err(BridgeError::new(
                BridgeErrorCode::SchemaMismatch,
                "bridge OK completion must carry output for the registered schema shape",
            ))
        }
        None => Ok(()),
    }
}

fn validate_completion_payload_refs(
    completion: &BridgeTaskCompletion,
    payloads: &BridgeCompletionPayloads,
) -> BridgeResult<()> {
    if let Some(output) = completion.output.as_ref() {
        validate_bridge_value_payload_refs(output, payloads, "$.output")?;
    }
    Ok(())
}

fn validate_bridge_value_payload_refs(
    value: &BridgeValue,
    payloads: &BridgeCompletionPayloads,
    path: &str,
) -> BridgeResult<()> {
    match value {
        BridgeValue::BlobRef(reference) => payloads.validate_blob_ref(path, reference),
        BridgeValue::PageRef(reference) => payloads.validate_page_ref(path, reference),
        BridgeValue::List(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_bridge_value_payload_refs(value, payloads, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        BridgeValue::Record(fields) => {
            for (field, value) in fields {
                validate_bridge_value_payload_refs(value, payloads, &format!("{path}.{field}"))?;
            }
            Ok(())
        }
        BridgeValue::Tagged { tag, value } => {
            validate_bridge_value_payload_refs(value, payloads, &format!("{path}#{tag}"))
        }
        BridgeValue::Result { value, .. } => {
            validate_bridge_value_payload_refs(value, payloads, &format!("{path}.value"))
        }
        BridgeValue::Null
        | BridgeValue::Bool(_)
        | BridgeValue::Int(_)
        | BridgeValue::Decimal(_)
        | BridgeValue::Text(_)
        | BridgeValue::Bytes { .. }
        | BridgeValue::Diagnostic(_)
        | BridgeValue::ArtifactRef(_) => Ok(()),
    }
}

pub fn canonical_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("bridge canonical value should serialize")
}

pub fn canonical_hash<T: Serialize>(value: &T) -> String {
    let mut writer = Sha256Writer(Sha256::new());
    serde_json::to_writer(&mut writer, value).expect("bridge canonical value should serialize");
    format!("sha256:{:x}", writer.0.finalize())
}

pub fn bridge_bytes_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

struct Sha256Writer(Sha256);

impl Write for Sha256Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct BridgePayloadStore {
    blobs: BTreeMap<String, Bytes>,
    pages: BTreeMap<String, Bytes>,
}

impl BridgePayloadStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_blob(
        &mut self,
        reference: &BridgeBlobRef,
        bytes: impl Into<Bytes>,
    ) -> BridgeResult<()> {
        validate_bridge_blob_ref_shape("$", reference)?;
        let bytes = bytes.into();
        validate_bridge_payload_bytes("blob", &reference.digest, reference.byte_len, &bytes)?;
        self.blobs.insert(reference.digest.clone(), bytes);
        Ok(())
    }

    pub fn insert_page(
        &mut self,
        reference: &BridgePageRef,
        bytes: impl Into<Bytes>,
    ) -> BridgeResult<()> {
        validate_bridge_page_ref_shape("$", reference)?;
        let bytes = bytes.into();
        validate_bridge_payload_bytes("page", &reference.page_digest, reference.byte_len, &bytes)?;
        self.pages.insert(reference.page_digest.clone(), bytes);
        Ok(())
    }

    pub fn blob(&self, digest: &str) -> Option<&Bytes> {
        self.blobs.get(digest)
    }

    pub fn page(&self, digest: &str) -> Option<&Bytes> {
        self.pages.get(digest)
    }

    pub fn blob_count(&self) -> usize {
        self.blobs.len()
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

#[derive(Clone, Debug, Default)]
pub struct BridgeCompletionPayloads {
    store: BridgePayloadStore,
}

impl BridgeCompletionPayloads {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_blob(
        &mut self,
        reference: &BridgeBlobRef,
        bytes: impl Into<Bytes>,
    ) -> BridgeResult<()> {
        self.store.insert_blob(reference, bytes)
    }

    pub fn insert_page(
        &mut self,
        reference: &BridgePageRef,
        bytes: impl Into<Bytes>,
    ) -> BridgeResult<()> {
        self.store.insert_page(reference, bytes)
    }

    pub fn blob_count(&self) -> usize {
        self.store.blob_count()
    }

    pub fn page_count(&self) -> usize {
        self.store.page_count()
    }

    fn validate_blob_ref(&self, path: &str, reference: &BridgeBlobRef) -> BridgeResult<()> {
        let Some(bytes) = self.store.blob(&reference.digest) else {
            return Err(BridgeError::new(
                BridgeErrorCode::SchemaMismatch,
                format!(
                    "bridge completion payload at `{path}` is missing blob `{}`",
                    reference.digest
                ),
            ));
        };
        validate_bridge_payload_bytes("blob", &reference.digest, reference.byte_len, bytes)
    }

    fn validate_page_ref(&self, path: &str, reference: &BridgePageRef) -> BridgeResult<()> {
        let Some(bytes) = self.store.page(&reference.page_digest) else {
            return Err(BridgeError::new(
                BridgeErrorCode::SchemaMismatch,
                format!(
                    "bridge completion payload at `{path}` is missing page `{}`",
                    reference.page_digest
                ),
            ));
        };
        validate_bridge_payload_bytes("page", &reference.page_digest, reference.byte_len, bytes)
    }
}

fn validate_bridge_payload_bytes(
    kind: &str,
    expected_digest: &str,
    expected_len: u64,
    bytes: &Bytes,
) -> BridgeResult<()> {
    let actual_len = bytes.len() as u64;
    if actual_len != expected_len {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!(
                "bridge {kind} payload declares byte_len {expected_len} but carries {actual_len} byte(s)"
            ),
        ));
    }
    let actual_digest = bridge_bytes_digest(bytes);
    if expected_digest != actual_digest {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!(
                "bridge {kind} payload digest mismatch: expected `{expected_digest}` but got `{actual_digest}`"
            ),
        ));
    }
    Ok(())
}

pub fn validate_no_rust_handles(value: &BridgeValue) -> BridgeResult<()> {
    match value {
        BridgeValue::Record(fields) => {
            for (key, value) in fields {
                let lowered = key.to_ascii_lowercase();
                if lowered.contains("rust_handle")
                    || lowered.contains("native_handle")
                    || lowered.contains("session_id")
                {
                    return Err(BridgeError::new(
                        BridgeErrorCode::RustHandleLeak,
                        format!("bridge value contains forbidden host identity field `{key}`"),
                    ));
                }
                validate_no_rust_handles(value)?;
            }
        }
        BridgeValue::List(values) => {
            for value in values {
                validate_no_rust_handles(value)?;
            }
        }
        BridgeValue::Tagged { tag, value } => {
            let lowered = tag.to_ascii_lowercase();
            if lowered.contains("rust_handle") || lowered.contains("native_handle") {
                return Err(BridgeError::new(
                    BridgeErrorCode::RustHandleLeak,
                    format!("bridge tagged value leaks host identity tag `{tag}`"),
                ));
            }
            validate_no_rust_handles(value)?;
        }
        BridgeValue::Result { value, .. } => validate_no_rust_handles(value)?,
        BridgeValue::Null
        | BridgeValue::Bool(_)
        | BridgeValue::Int(_)
        | BridgeValue::Decimal(_)
        | BridgeValue::Text(_)
        | BridgeValue::Bytes { .. }
        | BridgeValue::Diagnostic(_)
        | BridgeValue::BlobRef(_)
        | BridgeValue::ArtifactRef(_)
        | BridgeValue::PageRef(_) => {}
    }
    Ok(())
}

pub fn validate_payload_cap(value: &BridgeValue, max_inline_bytes: u64) -> BridgeResult<()> {
    match value {
        BridgeValue::Bytes {
            byte_len, bytes, ..
        } => {
            if *byte_len > max_inline_bytes || bytes.len() as u64 > max_inline_bytes {
                return Err(BridgeError::new(
                    BridgeErrorCode::PayloadCapExceeded,
                    format!("inline bridge bytes exceed cap {max_inline_bytes}"),
                ));
            }
        }
        BridgeValue::List(values) => {
            for value in values {
                validate_payload_cap(value, max_inline_bytes)?;
            }
        }
        BridgeValue::Record(fields) => {
            for value in fields.values() {
                validate_payload_cap(value, max_inline_bytes)?;
            }
        }
        BridgeValue::Tagged { value, .. } | BridgeValue::Result { value, .. } => {
            validate_payload_cap(value, max_inline_bytes)?;
        }
        BridgeValue::Null
        | BridgeValue::Bool(_)
        | BridgeValue::Int(_)
        | BridgeValue::Decimal(_)
        | BridgeValue::Text(_)
        | BridgeValue::Diagnostic(_)
        | BridgeValue::BlobRef(_)
        | BridgeValue::ArtifactRef(_)
        | BridgeValue::PageRef(_) => {}
    }
    Ok(())
}

pub fn validate_bridge_value_shape(
    value: &BridgeValue,
    shape: &BridgeSchemaShape,
) -> BridgeResult<()> {
    validate_bridge_value_shape_at(value, shape, "$")
}

fn validate_bridge_value_shape_at(
    value: &BridgeValue,
    shape: &BridgeSchemaShape,
    path: &str,
) -> BridgeResult<()> {
    match (shape, value) {
        (BridgeSchemaShape::Null, BridgeValue::Null)
        | (BridgeSchemaShape::Bool, BridgeValue::Bool(_))
        | (BridgeSchemaShape::Int, BridgeValue::Int(_))
        | (BridgeSchemaShape::Decimal, BridgeValue::Decimal(_))
        | (BridgeSchemaShape::Text, BridgeValue::Text(_))
        | (BridgeSchemaShape::Diagnostic, BridgeValue::Diagnostic(_)) => Ok(()),
        (
            BridgeSchemaShape::Bytes,
            BridgeValue::Bytes {
                digest,
                byte_len,
                bytes,
            },
        ) => validate_bridge_bytes_shape(path, digest, *byte_len, bytes),
        (BridgeSchemaShape::BlobRef, BridgeValue::BlobRef(value)) => {
            validate_bridge_blob_ref_shape(path, value)
        }
        (BridgeSchemaShape::ArtifactRef, BridgeValue::ArtifactRef(value)) => {
            validate_bridge_artifact_ref_shape(path, value)
        }
        (BridgeSchemaShape::PageRef, BridgeValue::PageRef(value)) => {
            validate_bridge_page_ref_shape(path, value)
        }
        (BridgeSchemaShape::List { item }, BridgeValue::List(values)) => {
            for (index, value) in values.iter().enumerate() {
                validate_bridge_value_shape_at(value, item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        (BridgeSchemaShape::Record { fields }, BridgeValue::Record(values)) => {
            for (field, field_shape) in fields {
                let Some(value) = values.get(field) else {
                    return Err(BridgeError::new(
                        BridgeErrorCode::SchemaMismatch,
                        format!("bridge value at `{path}` is missing record field `{field}`"),
                    ));
                };
                validate_bridge_value_shape_at(value, field_shape, &format!("{path}.{field}"))?;
            }
            Ok(())
        }
        (BridgeSchemaShape::Tagged { variants }, BridgeValue::Tagged { tag, value }) => {
            let Some(variant_shape) = variants.get(tag) else {
                return Err(BridgeError::new(
                    BridgeErrorCode::SchemaMismatch,
                    format!("bridge tagged value at `{path}` has unknown variant `{tag}`"),
                ));
            };
            validate_bridge_value_shape_at(value, variant_shape, &format!("{path}#{tag}"))
        }
        (BridgeSchemaShape::Result { ok, err }, BridgeValue::Result { ok: is_ok, value }) => {
            let result_shape = if *is_ok { ok } else { err };
            validate_bridge_value_shape_at(value, result_shape, &format!("{path}.value"))
        }
        (BridgeSchemaShape::Completion { output }, _) => {
            validate_bridge_value_shape_at(value, output, path)
        }
        _ => Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!(
                "bridge value at `{path}` expected `{}` but got `{}`",
                bridge_schema_shape_label(shape),
                bridge_value_label(value)
            ),
        )),
    }
}

fn validate_bridge_bytes_shape(
    path: &str,
    digest: &str,
    byte_len: u64,
    bytes: &Bytes,
) -> BridgeResult<()> {
    if digest.trim().is_empty() {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!("bridge bytes at `{path}` must carry a digest"),
        ));
    }
    if byte_len != bytes.len() as u64 {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!(
                "bridge bytes at `{path}` declare byte_len {byte_len} but carry {} byte(s)",
                bytes.len()
            ),
        ));
    }
    Ok(())
}

fn validate_bridge_blob_ref_shape(path: &str, value: &BridgeBlobRef) -> BridgeResult<()> {
    validate_non_empty_bridge_field(path, "digest", &value.digest)?;
    validate_non_empty_bridge_field(path, "media_type", &value.media_type)?;
    validate_non_empty_bridge_field(path, "storage", &value.storage)?;
    validate_non_empty_bridge_field(path, "encoding", &value.encoding)
}

fn validate_bridge_artifact_ref_shape(path: &str, value: &BridgeArtifactRef) -> BridgeResult<()> {
    validate_non_empty_bridge_field(path, "kind", &value.kind)?;
    validate_non_empty_bridge_field(path, "provider", &value.provider)?;
    validate_non_empty_bridge_field(path, "provider_version", &value.provider_version)?;
    validate_non_empty_bridge_field(path, "bridge_module", &value.bridge_module)?;
    validate_non_empty_bridge_field(path, "contract_version", &value.contract_version)?;
    validate_non_empty_bridge_field(path, "identity", &value.identity)?;
    validate_non_empty_bridge_field(path, "reproducibility", &value.reproducibility)?;
    if value.locator.is_empty() {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!("bridge artifact ref at `{path}` must carry a locator"),
        ));
    }
    Ok(())
}

fn validate_bridge_page_ref_shape(path: &str, value: &BridgePageRef) -> BridgeResult<()> {
    validate_non_empty_bridge_field(path, "artifact_digest", &value.artifact_digest)?;
    validate_non_empty_bridge_field(path, "schema_hash", &value.schema_hash)?;
    validate_non_empty_bridge_field(path, "request_fingerprint", &value.request_fingerprint)?;
    validate_non_empty_bridge_field(path, "response_fingerprint", &value.response_fingerprint)?;
    validate_non_empty_bridge_field(path, "input_digest", &value.input_digest)?;
    validate_non_empty_bridge_field(path, "page_digest", &value.page_digest)?;
    validate_non_empty_bridge_field(path, "status", &value.status)?;
    if value.schema_version == 0 {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!("bridge page ref at `{path}` must carry a nonzero schema_version"),
        ));
    }
    if value.byte_length != value.byte_len {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!(
                "bridge page ref at `{path}` has byte_length {} but byte_len {}",
                value.byte_length, value.byte_len
            ),
        ));
    }
    Ok(())
}

fn validate_non_empty_bridge_field(path: &str, field: &str, value: &str) -> BridgeResult<()> {
    if value.trim().is_empty() {
        return Err(BridgeError::new(
            BridgeErrorCode::SchemaMismatch,
            format!("bridge value at `{path}` must carry non-empty `{field}`"),
        ));
    }
    Ok(())
}

fn bridge_schema_shape_label(shape: &BridgeSchemaShape) -> &'static str {
    match shape {
        BridgeSchemaShape::Null => "null",
        BridgeSchemaShape::Bool => "bool",
        BridgeSchemaShape::Int => "int",
        BridgeSchemaShape::Decimal => "decimal",
        BridgeSchemaShape::Text => "text",
        BridgeSchemaShape::Bytes => "bytes",
        BridgeSchemaShape::List { .. } => "list",
        BridgeSchemaShape::Record { .. } => "record",
        BridgeSchemaShape::Tagged { .. } => "tagged",
        BridgeSchemaShape::Result { .. } => "result",
        BridgeSchemaShape::Diagnostic => "diagnostic",
        BridgeSchemaShape::BlobRef => "blob_ref",
        BridgeSchemaShape::ArtifactRef => "artifact_ref",
        BridgeSchemaShape::PageRef => "page_ref",
        BridgeSchemaShape::Completion { .. } => "completion",
    }
}

fn bridge_value_label(value: &BridgeValue) -> &'static str {
    match value {
        BridgeValue::Null => "null",
        BridgeValue::Bool(_) => "bool",
        BridgeValue::Int(_) => "int",
        BridgeValue::Decimal(_) => "decimal",
        BridgeValue::Text(_) => "text",
        BridgeValue::Bytes { .. } => "bytes",
        BridgeValue::List(_) => "list",
        BridgeValue::Record(_) => "record",
        BridgeValue::Tagged { .. } => "tagged",
        BridgeValue::Result { .. } => "result",
        BridgeValue::Diagnostic(_) => "diagnostic",
        BridgeValue::BlobRef(_) => "blob_ref",
        BridgeValue::ArtifactRef(_) => "artifact_ref",
        BridgeValue::PageRef(_) => "page_ref",
    }
}

pub fn bridge_fixture_registry() -> BridgeRegistry {
    let open_request = bridge_open_request_schema();
    let opened = bridge_opened_schema();
    let mut exports = BTreeMap::new();
    exports.insert(
        "open".to_owned(),
        BridgeExportMetadata {
            name: "open".to_owned(),
            kind: BridgeExportKind::Effect,
            input_schema_version: open_request.version,
            input_schema_hash: open_request.hash(),
            output_schema_version: opened.version,
            output_schema_hash: opened.hash(),
            required_capabilities: vec![BridgeCapability::new(
                "filesystem.wave_traces",
                "grant:wave-traces",
            )],
        },
    );
    exports.insert(
        "format_label".to_owned(),
        BridgeExportMetadata {
            name: "format_label".to_owned(),
            kind: BridgeExportKind::Pure,
            input_schema_version: opened.version,
            input_schema_hash: opened.hash(),
            output_schema_version: CANONICAL_SCHEMA_VERSION,
            output_schema_hash: bridge_text_schema("FormatLabel").hash(),
            required_capabilities: Vec::new(),
        },
    );
    let module = BridgeModuleMetadata {
        module: "wellen.v1".to_owned(),
        abi_version: BRIDGE_ABI_VERSION.to_owned(),
        canonical_schema_version: CANONICAL_SCHEMA_VERSION,
        provider: BridgeProviderMetadata {
            provider: "wellen".to_owned(),
            provider_version: "0.24.fixture".to_owned(),
            bridge_crate: "boon_fixture_bridge".to_owned(),
            bridge_crate_version: "0.1.0".to_owned(),
            features: vec!["vcd".to_owned(), "fst".to_owned(), "ghw".to_owned()],
        },
        exports,
    };
    let mut registry = BridgeRegistry::new();
    registry
        .register_module(module)
        .expect("fixture module should register once");
    registry
        .register_export_schemas("wellen.v1", "open", open_request.clone(), opened.clone())
        .expect("fixture open export schemas should match metadata");
    registry
        .register_export_schemas(
            "wellen.v1",
            "format_label",
            opened,
            bridge_text_schema("FormatLabel"),
        )
        .expect("fixture format_label export schemas should match metadata");
    registry
}

pub fn bridge_open_request_schema() -> BridgeSchema {
    BridgeSchema {
        name: "OpenWaveformRequest".to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Record {
            fields: BTreeMap::from([
                ("file".to_owned(), BridgeSchemaShape::ArtifactRef),
                (
                    "options".to_owned(),
                    BridgeSchemaShape::Record {
                        fields: BTreeMap::from([("hierarchy".to_owned(), BridgeSchemaShape::Bool)]),
                    },
                ),
            ]),
        },
    }
}

pub fn bridge_opened_schema() -> BridgeSchema {
    BridgeSchema {
        name: "WaveformOpened".to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Record {
            fields: BTreeMap::from([
                ("waveform".to_owned(), BridgeSchemaShape::ArtifactRef),
                ("hierarchy".to_owned(), BridgeSchemaShape::PageRef),
                (
                    "diagnostics".to_owned(),
                    BridgeSchemaShape::List {
                        item: Box::new(BridgeSchemaShape::Diagnostic),
                    },
                ),
            ]),
        },
    }
}

pub fn bridge_text_schema(name: &str) -> BridgeSchema {
    BridgeSchema {
        name: name.to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Text,
    }
}

pub fn bridge_golden_vectors() -> BTreeMap<String, String> {
    let diagnostic = BridgeValue::Diagnostic(BridgeDiagnostic {
        code: "fixture.info".to_owned(),
        message: "loaded".to_owned(),
        severity: "info".to_owned(),
    });
    let blob = BridgeValue::BlobRef(BridgeBlobRef {
        digest: "sha256:blob".to_owned(),
        byte_len: 4096,
        media_type: "application/vnd.boon.wave-page".to_owned(),
        storage: "bridge-cache".to_owned(),
        encoding: "arrow-ipc".to_owned(),
    });
    let artifact = BridgeValue::ArtifactRef(fixture_artifact_ref());
    let page = BridgeValue::PageRef(fixture_page_ref());
    let tagged = BridgeValue::Tagged {
        tag: "Loaded".to_owned(),
        value: Box::new(BridgeValue::Record(BTreeMap::from([
            ("artifact".to_owned(), artifact.clone()),
            ("page".to_owned(), page.clone()),
        ]))),
    };
    let list = BridgeValue::List(vec![diagnostic.clone(), blob.clone(), page.clone()]);
    let record = BridgeValue::Record(BTreeMap::from([
        ("blob".to_owned(), blob),
        ("diagnostic".to_owned(), diagnostic),
        ("list".to_owned(), list),
        (
            "result".to_owned(),
            BridgeValue::Result {
                ok: true,
                value: Box::new(tagged),
            },
        ),
    ]));
    let registry = bridge_fixture_registry();
    let export = registry.export("wellen.v1", "open").unwrap();
    let request = BridgeTaskRequest::new(
        export,
        "wellen.v1",
        "wave-open:fixture",
        7,
        fixture_open_request_value(),
        vec!["grant:wave-traces".to_owned()],
        "cancel:wave-open:fixture",
        0,
    );
    let completion = BridgeTaskCompletion::for_request(
        &request,
        BridgeCompletionStatus::Ok,
        Some(fixture_opened_value()),
        Vec::new(),
    );
    BTreeMap::from([
        ("record".to_owned(), canonical_hash(&record)),
        (
            "schema".to_owned(),
            canonical_hash(&BridgeSchemaShape::Completion {
                output: Box::new(bridge_opened_schema().shape),
            }),
        ),
        ("completion".to_owned(), canonical_hash(&completion)),
    ])
}

fn fixture_artifact_ref() -> BridgeArtifactRef {
    BridgeArtifactRef {
        kind: "wellen.waveform".to_owned(),
        provider: "wellen".to_owned(),
        provider_version: "0.24.fixture".to_owned(),
        bridge_module: "wellen.v1".to_owned(),
        contract_version: "waveform.v1".to_owned(),
        identity: "sha256:waveform".to_owned(),
        locator: BTreeMap::from([
            ("kind".to_owned(), "file".to_owned()),
            ("path".to_owned(), "/fixtures/simple.vcd".to_owned()),
        ]),
        reproducibility: "content-addressed".to_owned(),
    }
}

fn fixture_page_ref() -> BridgePageRef {
    BridgePageRef {
        artifact_digest: "sha256:waveform".to_owned(),
        schema_version: CANONICAL_SCHEMA_VERSION,
        schema_hash: bridge_opened_schema().hash(),
        request_fingerprint: "req:fixture-open".to_owned(),
        response_fingerprint: "resp:fixture-open".to_owned(),
        input_digest: "sha256:request-input".to_owned(),
        page_digest: "sha256:page".to_owned(),
        generation: 1,
        offset: 0,
        limit: 128,
        row_count: 3,
        sample_count: 12,
        transition_count: 12,
        byte_length: 1024,
        byte_len: 1024,
        status: "ready".to_owned(),
    }
}

fn fixture_diagnostic() -> BridgeDiagnostic {
    BridgeDiagnostic {
        code: "fixture.info".to_owned(),
        message: "loaded".to_owned(),
        severity: "info".to_owned(),
    }
}

fn fixture_open_request_value() -> BridgeValue {
    BridgeValue::Record(BTreeMap::from([
        (
            "file".to_owned(),
            BridgeValue::ArtifactRef(fixture_artifact_ref()),
        ),
        (
            "options".to_owned(),
            BridgeValue::Record(BTreeMap::from([(
                "hierarchy".to_owned(),
                BridgeValue::Bool(true),
            )])),
        ),
    ]))
}

fn fixture_opened_value() -> BridgeValue {
    BridgeValue::Record(BTreeMap::from([
        (
            "waveform".to_owned(),
            BridgeValue::ArtifactRef(fixture_artifact_ref()),
        ),
        (
            "hierarchy".to_owned(),
            BridgeValue::PageRef(fixture_page_ref()),
        ),
        (
            "diagnostics".to_owned(),
            BridgeValue::List(vec![BridgeValue::Diagnostic(fixture_diagnostic())]),
        ),
    ]))
}

fn fixture_open_request(
    export: &BridgeExportMetadata,
    request_id: &str,
    request_epoch: u64,
    input: BridgeValue,
) -> BridgeTaskRequest {
    BridgeTaskRequest::new(
        export,
        "wellen.v1",
        request_id,
        request_epoch,
        input,
        vec!["grant:wave-traces".to_owned()],
        format!("cancel:{request_id}"),
        0,
    )
}

fn fixture_blob_ref_for_bytes(bytes: &Bytes) -> BridgeBlobRef {
    BridgeBlobRef {
        digest: bridge_bytes_digest(bytes),
        byte_len: bytes.len() as u64,
        media_type: "application/vnd.boon.wave-page".to_owned(),
        storage: "bridge-cache".to_owned(),
        encoding: "packed-wave-page".to_owned(),
    }
}

fn fixture_page_ref_for_bytes(bytes: &Bytes) -> BridgePageRef {
    let mut page = fixture_page_ref();
    page.page_digest = bridge_bytes_digest(bytes);
    page.byte_length = bytes.len() as u64;
    page.byte_len = bytes.len() as u64;
    page
}

fn bridge_payload_request_schema() -> BridgeSchema {
    BridgeSchema {
        name: "PayloadRequest".to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Record {
            fields: BTreeMap::from([("file".to_owned(), BridgeSchemaShape::ArtifactRef)]),
        },
    }
}

fn bridge_payload_completion_schema() -> BridgeSchema {
    BridgeSchema {
        name: "PayloadCompletion".to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Record {
            fields: BTreeMap::from([
                ("blob".to_owned(), BridgeSchemaShape::BlobRef),
                ("page".to_owned(), BridgeSchemaShape::PageRef),
                ("status".to_owned(), BridgeSchemaShape::Text),
            ]),
        },
    }
}

fn bridge_payload_fixture_registry() -> BridgeRegistry {
    let input = bridge_payload_request_schema();
    let output = bridge_payload_completion_schema();
    let mut exports = BTreeMap::new();
    exports.insert(
        "load_payloads".to_owned(),
        BridgeExportMetadata {
            name: "load_payloads".to_owned(),
            kind: BridgeExportKind::Effect,
            input_schema_version: input.version,
            input_schema_hash: input.hash(),
            output_schema_version: output.version,
            output_schema_hash: output.hash(),
            required_capabilities: Vec::new(),
        },
    );
    let module = BridgeModuleMetadata {
        module: "payloads.v1".to_owned(),
        abi_version: BRIDGE_ABI_VERSION.to_owned(),
        canonical_schema_version: CANONICAL_SCHEMA_VERSION,
        provider: BridgeProviderMetadata {
            provider: "payload-fixture".to_owned(),
            provider_version: "0.1.fixture".to_owned(),
            bridge_crate: "boon_payload_fixture_bridge".to_owned(),
            bridge_crate_version: "0.1.0".to_owned(),
            features: vec!["blob-ref".to_owned(), "page-ref".to_owned()],
        },
        exports,
    };
    let mut registry = BridgeRegistry::new();
    registry
        .register_module(module)
        .expect("payload fixture module should register once");
    registry
        .register_export_schemas("payloads.v1", "load_payloads", input, output)
        .expect("payload fixture schemas should match metadata");
    registry
}

fn fixture_payload_request_value() -> BridgeValue {
    BridgeValue::Record(BTreeMap::from([(
        "file".to_owned(),
        BridgeValue::ArtifactRef(fixture_artifact_ref()),
    )]))
}

fn fixture_payload_completion_value(
    blob_ref: BridgeBlobRef,
    page_ref: BridgePageRef,
) -> BridgeValue {
    BridgeValue::Record(BTreeMap::from([
        ("blob".to_owned(), BridgeValue::BlobRef(blob_ref)),
        ("page".to_owned(), BridgeValue::PageRef(page_ref)),
        ("status".to_owned(), BridgeValue::Text("ready".to_owned())),
    ]))
}

fn fixture_payload_request(export: &BridgeExportMetadata, request_id: &str) -> BridgeTaskRequest {
    BridgeTaskRequest::new(
        export,
        "payloads.v1",
        request_id,
        1,
        fixture_payload_request_value(),
        Vec::new(),
        format!("cancel:{request_id}"),
        0,
    )
}

fn bridge_payload_store_contract_check() -> (bool, JsonValue) {
    let blob_bytes = Bytes::from_static(b"raw waveform blob page bytes");
    let page_bytes = Bytes::from_static(b"decoded page bytes");
    let blob_ref = fixture_blob_ref_for_bytes(&blob_bytes);
    let page_ref = fixture_page_ref_for_bytes(&page_bytes);
    let mut store = BridgePayloadStore::new();
    let blob_insert = store.insert_blob(&blob_ref, blob_bytes.clone());
    let page_insert = store.insert_page(&page_ref, page_bytes.clone());
    let blob_replayed = store
        .blob(&blob_ref.digest)
        .is_some_and(|stored| stored == &blob_bytes);
    let page_replayed = store
        .page(&page_ref.page_digest)
        .is_some_and(|stored| stored == &page_bytes);

    let mut bad_digest_ref = blob_ref.clone();
    bad_digest_ref.digest = "sha256:not-the-payload".to_owned();
    let digest_drift = BridgePayloadStore::new().insert_blob(&bad_digest_ref, blob_bytes.clone());

    let mut bad_len_ref = page_ref.clone();
    bad_len_ref.byte_length = bad_len_ref.byte_length.saturating_add(1);
    bad_len_ref.byte_len = bad_len_ref.byte_len.saturating_add(1);
    let len_drift = BridgePayloadStore::new().insert_page(&bad_len_ref, page_bytes.clone());

    let pass = blob_insert.is_ok()
        && page_insert.is_ok()
        && blob_replayed
        && page_replayed
        && digest_drift
            .as_ref()
            .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch)
        && len_drift
            .as_ref()
            .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch);

    (
        pass,
        json!({
            "blob_digest": blob_ref.digest,
            "blob_byte_len": blob_ref.byte_len,
            "page_digest": page_ref.page_digest,
            "page_byte_len": page_ref.byte_len,
            "stored_blobs": store.blob_count(),
            "stored_pages": store.page_count(),
            "blob_replayed": blob_replayed,
            "page_replayed": page_replayed,
            "digest_drift": digest_drift.err().map(|error| format!("{:?}", error.code)),
            "len_drift": len_drift.err().map(|error| format!("{:?}", error.code)),
        }),
    )
}

fn bridge_completion_payloads_contract_check() -> (bool, JsonValue) {
    let registry = bridge_payload_fixture_registry();
    let export = registry.export("payloads.v1", "load_payloads").unwrap();
    let blob_bytes = Bytes::from_static(b"completion raw blob bytes");
    let page_bytes = Bytes::from_static(b"completion page bytes");
    let blob_ref = fixture_blob_ref_for_bytes(&blob_bytes);
    let page_ref = fixture_page_ref_for_bytes(&page_bytes);

    let valid = {
        let request = fixture_payload_request(export, "payloads:valid");
        let mut scheduler = BridgeEffectScheduler::new(16);
        let schedule = scheduler.schedule(&registry, request.clone());
        let mut payloads = BridgeCompletionPayloads::new();
        let blob_insert = payloads.insert_blob(&blob_ref, blob_bytes.clone());
        let page_insert = payloads.insert_page(&page_ref, page_bytes.clone());
        let completion = BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            Some(fixture_payload_completion_value(
                blob_ref.clone(),
                page_ref.clone(),
            )),
            Vec::new(),
        );
        (
            schedule,
            blob_insert,
            page_insert,
            scheduler.complete_with_payloads(completion, &payloads),
            payloads.blob_count(),
            payloads.page_count(),
        )
    };

    let descriptor_only = {
        let request = fixture_payload_request(export, "payloads:descriptor-only");
        let mut scheduler = BridgeEffectScheduler::new(16);
        let _ = scheduler.schedule(&registry, request.clone());
        scheduler.complete(BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            Some(fixture_payload_completion_value(
                blob_ref.clone(),
                page_ref.clone(),
            )),
            Vec::new(),
        ))
    };

    let missing_page = {
        let request = fixture_payload_request(export, "payloads:missing-page");
        let mut scheduler = BridgeEffectScheduler::new(16);
        let _ = scheduler.schedule(&registry, request.clone());
        let mut payloads = BridgeCompletionPayloads::new();
        let _ = payloads.insert_blob(&blob_ref, blob_bytes.clone());
        scheduler.complete_with_payloads(
            BridgeTaskCompletion::for_request(
                &request,
                BridgeCompletionStatus::Ok,
                Some(fixture_payload_completion_value(
                    blob_ref.clone(),
                    page_ref.clone(),
                )),
                Vec::new(),
            ),
            &payloads,
        )
    };

    let drifted_page = {
        let request = fixture_payload_request(export, "payloads:drifted-page");
        let mut scheduler = BridgeEffectScheduler::new(16);
        let _ = scheduler.schedule(&registry, request.clone());
        let mut payloads = BridgeCompletionPayloads::new();
        let _ = payloads.insert_blob(&blob_ref, blob_bytes.clone());
        payloads.store.pages.insert(
            page_ref.page_digest.clone(),
            Bytes::from(vec![b'x'; page_bytes.len()]),
        );
        scheduler.complete_with_payloads(
            BridgeTaskCompletion::for_request(
                &request,
                BridgeCompletionStatus::Ok,
                Some(fixture_payload_completion_value(
                    blob_ref.clone(),
                    page_ref.clone(),
                )),
                Vec::new(),
            ),
            &payloads,
        )
    };

    let pass = valid.0.is_ok()
        && valid.1.is_ok()
        && valid.2.is_ok()
        && valid.3.is_ok()
        && valid.4 == 1
        && valid.5 == 1
        && descriptor_only.is_ok()
        && missing_page
            .as_ref()
            .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch)
        && drifted_page
            .as_ref()
            .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch);

    (
        pass,
        json!({
            "blob_digest": blob_ref.digest,
            "blob_byte_len": blob_ref.byte_len,
            "page_digest": page_ref.page_digest,
            "page_byte_len": page_ref.byte_len,
            "stored_blobs": valid.4,
            "stored_pages": valid.5,
            "descriptor_only_complete": descriptor_only.is_ok(),
            "missing_page": missing_page.err().map(|error| format!("{:?}", error.code)),
            "drifted_page": drifted_page.err().map(|error| format!("{:?}", error.code)),
        }),
    )
}

pub fn bridge_contract_checks() -> Vec<(String, bool, JsonValue)> {
    let registry = bridge_fixture_registry();
    let open = registry.export("wellen.v1", "open").unwrap();
    let mut scheduler = BridgeEffectScheduler::new(16);
    let request = fixture_open_request(open, "wave-open:fixture", 1, fixture_open_request_value());
    let first = scheduler.schedule(&registry, request.clone());
    let dedup = scheduler.schedule(&registry, request.clone());
    let stale = {
        let mut stale = BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            Some(fixture_opened_value()),
            Vec::new(),
        );
        stale.request_epoch = 0;
        scheduler.complete(stale)
    };
    let accepted = scheduler.complete(BridgeTaskCompletion::for_request(
        &request,
        BridgeCompletionStatus::Ok,
        Some(fixture_opened_value()),
        Vec::new(),
    ));
    let duplicate = scheduler.complete(BridgeTaskCompletion::for_request(
        &request,
        BridgeCompletionStatus::Ok,
        Some(fixture_opened_value()),
        Vec::new(),
    ));
    let missing_module = registry.validate_import(
        "missing.v1",
        "open",
        BridgeExportKind::Effect,
        &open.input_schema_hash,
        &open.output_schema_hash,
    );
    let changed_schema = registry.validate_import(
        "wellen.v1",
        "open",
        BridgeExportKind::Effect,
        "sha256:changed",
        &open.output_schema_hash,
    );
    let wrong_kind = registry.validate_import(
        "wellen.v1",
        "format_label",
        BridgeExportKind::Effect,
        &registry
            .export("wellen.v1", "format_label")
            .unwrap()
            .input_schema_hash,
        &registry
            .export("wellen.v1", "format_label")
            .unwrap()
            .output_schema_hash,
    );
    let grant_denied = BridgeEffectScheduler::new(16).schedule(
        &registry,
        BridgeTaskRequest::new(
            open,
            "wellen.v1",
            "wave-open:no-grant",
            1,
            BridgeValue::Record(BTreeMap::new()),
            Vec::new(),
            "cancel:no-grant",
            0,
        ),
    );
    let payload_cap = BridgeEffectScheduler::new(2).schedule(
        &registry,
        BridgeTaskRequest::new(
            open,
            "wellen.v1",
            "wave-open:large",
            1,
            BridgeValue::inline_bytes("sha256:inline", Vec::from([1, 2, 3, 4])),
            vec!["grant:wave-traces".to_owned()],
            "cancel:large",
            0,
        ),
    );
    let rust_handle = BridgeEffectScheduler::new(16).schedule(
        &registry,
        BridgeTaskRequest::new(
            open,
            "wellen.v1",
            "wave-open:handle",
            1,
            BridgeValue::Record(BTreeMap::from([(
                "rust_handle".to_owned(),
                BridgeValue::Text("0xdeadbeef".to_owned()),
            )])),
            vec!["grant:wave-traces".to_owned()],
            "cancel:handle",
            0,
        ),
    );
    let mut cancel_scheduler = BridgeEffectScheduler::new(16);
    let cancel_request =
        fixture_open_request(open, "wave-open:cancel", 1, fixture_open_request_value());
    let _ = cancel_scheduler.schedule(&registry, cancel_request.clone());
    let cancel_result = cancel_scheduler.cancel(&cancel_request.request_key, 2);
    let canceled_completion = BridgeTaskCompletion::for_request(
        &cancel_request,
        BridgeCompletionStatus::Canceled,
        None,
        Vec::new(),
    );
    let canceled_completion = BridgeTaskCompletion {
        cancellation_epoch: 2,
        ..canceled_completion
    };
    let cancel_accept = cancel_scheduler.complete(canceled_completion);
    let replay_completion = BridgeTaskCompletion::for_request(
        &request,
        BridgeCompletionStatus::Ok,
        Some(fixture_opened_value()),
        Vec::new(),
    );
    let replay = BridgeEffectScheduler::with_replay(16, vec![replay_completion])
        .schedule(&registry, request.clone());
    let request_shape = validate_bridge_value_shape(
        &fixture_open_request_value(),
        &bridge_open_request_schema().shape,
    );
    let opened_shape =
        validate_bridge_value_shape(&fixture_opened_value(), &bridge_opened_schema().shape);
    let text_shape = validate_bridge_value_shape(
        &BridgeValue::Text("waveform ready".to_owned()),
        &bridge_text_schema("FormatLabel").shape,
    );
    let missing_options_request = fixture_open_request(
        open,
        "wave-open:missing-options",
        1,
        BridgeValue::Record(BTreeMap::from([(
            "file".to_owned(),
            BridgeValue::ArtifactRef(fixture_artifact_ref()),
        )])),
    );
    let missing_options =
        BridgeEffectScheduler::new(16).schedule(&registry, missing_options_request);
    let bare_page_output = {
        let mut scheduler = BridgeEffectScheduler::new(16);
        let request = fixture_open_request(
            open,
            "wave-open:bare-page-output",
            1,
            fixture_open_request_value(),
        );
        let _ = scheduler.schedule(&registry, request.clone());
        scheduler.complete(BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            Some(BridgeValue::PageRef(fixture_page_ref())),
            Vec::new(),
        ))
    };
    let replay_shape_mismatch = {
        let request = fixture_open_request(
            open,
            "wave-open:replay-bare-page",
            1,
            fixture_open_request_value(),
        );
        let replay_completion = BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            Some(BridgeValue::PageRef(fixture_page_ref())),
            Vec::new(),
        );
        BridgeEffectScheduler::with_replay(16, vec![replay_completion]).schedule(&registry, request)
    };
    let ok_missing_output = {
        let mut scheduler = BridgeEffectScheduler::new(16);
        let request =
            fixture_open_request(open, "wave-open:no-output", 1, fixture_open_request_value());
        let _ = scheduler.schedule(&registry, request.clone());
        scheduler.complete(BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            None,
            Vec::new(),
        ))
    };
    let payload_store = bridge_payload_store_contract_check();
    let completion_payloads = bridge_completion_payloads_contract_check();

    vec![
        (
            "bridge_metadata_includes_module_exports_schema_hashes_capabilities_provider_and_abi"
                .to_owned(),
            registry.module("wellen.v1").is_some_and(|module| {
                module.abi_version == BRIDGE_ABI_VERSION
                    && module.canonical_schema_version == CANONICAL_SCHEMA_VERSION
                    && module.provider.provider == "wellen"
                    && module
                        .exports
                        .get("open")
                        .is_some_and(|export| !export.required_capabilities.is_empty())
            }),
            json!(registry.modules()),
        ),
        (
            "bridge_golden_vectors_cover_schema_value_and_completion".to_owned(),
            bridge_golden_vectors().len() == 3,
            json!(bridge_golden_vectors()),
        ),
        (
            "bridge_fixture_values_match_declared_schema_shapes".to_owned(),
            request_shape.is_ok() && opened_shape.is_ok() && text_shape.is_ok(),
            json!({
                "open_request": request_shape.err().map(|error| format!("{:?}", error.code)).unwrap_or_else(|| "ok".to_owned()),
                "opened": opened_shape.err().map(|error| format!("{:?}", error.code)).unwrap_or_else(|| "ok".to_owned()),
                "format_label": text_shape.err().map(|error| format!("{:?}", error.code)).unwrap_or_else(|| "ok".to_owned()),
            }),
        ),
        (
            "bridge_schedule_deduplicates_and_accepts_matching_completion".to_owned(),
            matches!(
                first.map(|result| result.outcome),
                Ok(BridgeScheduleOutcome::Scheduled)
            ) && matches!(
                dedup.map(|result| result.outcome),
                Ok(BridgeScheduleOutcome::Deduplicated)
            ) && accepted.is_ok(),
            json!({"request_key": request.request_key}),
        ),
        (
            "bridge_negative_cases_are_rejected".to_owned(),
            stale
                .as_ref()
                .is_err_and(|error| error.code == BridgeErrorCode::StaleCompletion)
                && duplicate
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::DuplicateCompletion)
                && missing_module
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::MissingModule)
                && changed_schema
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch)
                && wrong_kind
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::WrongExportKind)
                && grant_denied
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::GrantDenied)
                && payload_cap
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::PayloadCapExceeded)
                && rust_handle
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::RustHandleLeak),
            json!({
                "stale": stale.err().map(|error| format!("{:?}", error.code)),
                "duplicate": duplicate.err().map(|error| format!("{:?}", error.code)),
                "missing_module": missing_module.err().map(|error| format!("{:?}", error.code)),
                "changed_schema": changed_schema.err().map(|error| format!("{:?}", error.code)),
                "wrong_kind": wrong_kind.err().map(|error| format!("{:?}", error.code)),
                "grant_denied": grant_denied.err().map(|error| format!("{:?}", error.code)),
                "payload_cap": payload_cap.err().map(|error| format!("{:?}", error.code)),
                "rust_handle": rust_handle.err().map(|error| format!("{:?}", error.code)),
            }),
        ),
        (
            "bridge_scheduler_rejects_registered_shape_mismatches".to_owned(),
            missing_options
                .as_ref()
                .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch)
                && bare_page_output
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch)
                && replay_shape_mismatch
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch)
                && ok_missing_output
                    .as_ref()
                    .is_err_and(|error| error.code == BridgeErrorCode::SchemaMismatch),
            json!({
                "missing_options": missing_options.err().map(|error| format!("{:?}", error.code)),
                "bare_page_output": bare_page_output.err().map(|error| format!("{:?}", error.code)),
                "replay_shape_mismatch": replay_shape_mismatch.err().map(|error| format!("{:?}", error.code)),
                "ok_missing_output": ok_missing_output.err().map(|error| format!("{:?}", error.code)),
            }),
        ),
        (
            "bridge_payload_store_keeps_raw_bytes_behind_refs".to_owned(),
            payload_store.0,
            payload_store.1,
        ),
        (
            "bridge_scheduler_completion_payload_sidecars_validate_refs".to_owned(),
            completion_payloads.0,
            completion_payloads.1,
        ),
        (
            "bridge_cancellation_and_replay_are_data_driven".to_owned(),
            cancel_result.is_ok()
                && cancel_accept.is_ok()
                && matches!(
                    replay.map(|result| result.outcome),
                    Ok(BridgeScheduleOutcome::Replayed)
                ),
            json!({"cancellation_epoch": 2, "replay": "recorded_completion"}),
        ),
    ]
}

#[cfg(test)]
mod tests;
