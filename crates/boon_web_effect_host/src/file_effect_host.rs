use super::content_store::{
    BrowserContentImport, BrowserContentMetadata, BrowserContentStoreError,
    BrowserContentStoreLimits, BrowserIndexedDbContentStore,
};
use crate::{WebHostError, WebHostResult};
use boon_app_package::{BrowserPackageAssetDescriptor, MAX_BROWSER_PACKAGE_ASSETS};
use boon_effect_schema::{
    CONTENT_IMPORT_OPERATION, CONTENT_SAVE_OPERATION, FILE_BYTES_MAX_LIMIT,
    FILE_READ_BYTES_OPERATION, FILE_READ_STREAM_OPERATION, FILE_STREAM_DEFAULT_CHUNK_BYTES,
    FILE_STREAM_INITIAL_CREDITS, FILE_STREAM_MAX_CHUNK_BYTES, FILE_STREAM_MAX_IN_FLIGHT,
    FILE_STREAM_MIN_CHUNK_BYTES, FILE_WRITE_BYTES_OPERATION,
};
use boon_plan::{EffectId, builtin_effect_contract};
use boon_runtime::{
    ByteStreamValidator, ContentRef, EffectCommitGuard, EffectCommitPermit, EffectStopDisposition,
    EffectStopReason, HostCapabilityErrorKind, HostCapabilityRegistry, HostValueBinding,
    TransientEffectCallId, TransientEffectCreditGrant, TransientEffectInvocation, Value,
};
use js_sys::{Reflect, Uint8Array};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    AbortController, File, FileSystemFileHandle, FileSystemWritableFileStream,
    ReadableStreamDefaultReader, Request, RequestCredentials, RequestInit, RequestMode,
    RequestRedirect, Response, WritableStream,
};

const CAPABILITY_TOKEN_BYTES: usize = 32;
const CAPABILITY_TOKEN_ATTEMPTS: usize = 16;
const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
const MAX_DISPLAY_NAME_BYTES: usize = 256;
const RESULT_ENVELOPE_BYTES: usize = 512;
const SMALL_RESULT_RESERVATION_BYTES: usize = 4 * 1024;
const DEFAULT_MEDIA: &str = "application/octet-stream";
const MAX_PACKAGE_TRANSPORT_CHUNK_BYTES: usize = FILE_STREAM_MAX_CHUNK_BYTES as usize;
const DEFAULT_OPERATION_TIMEOUT_MS: i32 = 30_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserFileEffectOperation {
    ReadBytes,
    WriteBytes,
    ReadStream,
    ContentImport,
    ContentSave,
}

impl BrowserFileEffectOperation {
    pub(crate) const fn host_operation(self) -> &'static str {
        match self {
            Self::ReadBytes => FILE_READ_BYTES_OPERATION,
            Self::WriteBytes => FILE_WRITE_BYTES_OPERATION,
            Self::ReadStream => FILE_READ_STREAM_OPERATION,
            Self::ContentImport => CONTENT_IMPORT_OPERATION,
            Self::ContentSave => CONTENT_SAVE_OPERATION,
        }
    }

    const fn is_stream(self) -> bool {
        matches!(
            self,
            Self::ReadStream | Self::ContentImport | Self::ContentSave
        )
    }

    const fn owns_global_writer(self) -> bool {
        matches!(self, Self::WriteBytes | Self::ContentSave)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrowserFileEffectLimits {
    pub max_active: usize,
    pub max_capabilities: usize,
    pub max_queue_items: usize,
    pub max_queue_bytes: usize,
    pub max_content_entries: usize,
    pub max_content_bytes: usize,
    pub operation_timeout_ms: i32,
}

impl Default for BrowserFileEffectLimits {
    fn default() -> Self {
        Self {
            max_active: 32,
            max_capabilities: 128,
            max_queue_items: 160,
            max_queue_bytes: 32 * 1024 * 1024,
            max_content_entries: 32,
            max_content_bytes: 64 * 1024 * 1024,
            operation_timeout_ms: DEFAULT_OPERATION_TIMEOUT_MS,
        }
    }
}

impl BrowserFileEffectLimits {
    fn validate(self) -> WebHostResult<Self> {
        if self.max_active == 0
            || self.max_capabilities == 0
            || self.max_queue_items == 0
            || self.max_queue_bytes == 0
            || self.max_content_entries == 0
            || self.max_content_bytes == 0
            || self.operation_timeout_ms <= 0
        {
            return Err(WebHostError::InvalidInput {
                field: "browser File/Content limits".to_owned(),
                reason: "all limits must be positive".to_owned(),
            });
        }
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct BrowserFileEffectNotification {
    pub call_id: TransientEffectCallId,
    pub operation: BrowserFileEffectOperation,
    pub result_sequence: Option<u64>,
    pub terminal: bool,
    pub outcome: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserFileAccess {
    Source,
    Target,
}

#[derive(Clone)]
enum BrowserFileResource {
    Source(File),
    Target(FileSystemFileHandle),
}

#[derive(Clone)]
struct RegisteredPackageAsset {
    descriptor: Rc<BrowserPackageAssetDescriptor>,
    digest: [u8; 32],
    display_name: String,
}

#[derive(Clone)]
struct SharedDriver {
    calls: Rc<RefCell<CallState>>,
    queue: Rc<RefCell<BoundedNotificationQueue>>,
    content: BrowserIndexedDbContentStore,
    wake: Rc<dyn Fn()>,
}

struct BrowserFileDeadline {
    timeout_id: i32,
    _callback: Closure<dyn FnMut()>,
}

/// Browser-owned platform backend for the canonical File and Content effects.
///
/// Browser objects never enter serializable Boon data. File reads are sliced
/// only after runtime credit and queue capacity have both been reserved.
pub struct BrowserFileEffectHost {
    capabilities: HostCapabilityRegistry<BrowserFileResource, BrowserFileAccess>,
    package_assets: BTreeMap<String, RegisteredPackageAsset>,
    shared: SharedDriver,
    limits: BrowserFileEffectLimits,
    deadlines: BTreeMap<TransientEffectCallId, BrowserFileDeadline>,
}

impl BrowserFileEffectHost {
    pub async fn open(
        package_id: &str,
        wake: Rc<dyn Fn()>,
        limits: BrowserFileEffectLimits,
    ) -> WebHostResult<Self> {
        let limits = limits.validate()?;
        let mut content_limits = BrowserContentStoreLimits::default();
        content_limits.max_content_entries =
            u32::try_from(limits.max_content_entries).map_err(|_| WebHostError::InvalidInput {
                field: "browser content entry limit".to_owned(),
                reason: "limit exceeds the browser content-store range".to_owned(),
            })?;
        content_limits.max_content_bytes = limits.max_content_bytes as u64;
        content_limits.max_staging_imports =
            u32::try_from(limits.max_active).map_err(|_| WebHostError::InvalidInput {
                field: "browser active File/Content limit".to_owned(),
                reason: "limit exceeds the browser content-store range".to_owned(),
            })?;
        content_limits.max_staging_bytes = limits.max_content_bytes as u64;
        let content = BrowserIndexedDbContentStore::open(package_id, content_limits)
            .await
            .map_err(content_store_open_error)?;
        let mut issuer = [0_u8; CAPABILITY_TOKEN_BYTES];
        getrandom::fill(&mut issuer).map_err(|error| WebHostError::Platform {
            operation: "initialize browser file capability issuer".to_owned(),
            message: error.to_string(),
        })?;
        let capabilities = HostCapabilityRegistry::new(issuer, limits.max_capabilities)
            .map_err(capability_configuration_error)?;
        Ok(Self {
            capabilities,
            package_assets: BTreeMap::new(),
            shared: SharedDriver {
                calls: Rc::new(RefCell::new(CallState::new())),
                queue: Rc::new(RefCell::new(BoundedNotificationQueue::new(
                    limits.max_queue_items,
                    limits.max_queue_bytes,
                ))),
                content,
                wake,
            },
            limits,
            deadlines: BTreeMap::new(),
        })
    }

    /// Atomically replaces the exact package-asset allowlist used by read effects.
    ///
    /// Browser startup integration must call this before admitting effects. An
    /// active call prevents replacement so every call retains one bootstrap
    /// allowlist for its full lifetime.
    pub fn register_package_assets(
        &mut self,
        descriptors: &[BrowserPackageAssetDescriptor],
    ) -> WebHostResult<()> {
        if !self.shared.calls.borrow().active.is_empty() {
            return Err(WebHostError::InvalidInput {
                field: "browser package assets".to_owned(),
                reason: "cannot replace the package-asset allowlist while calls are active"
                    .to_owned(),
            });
        }
        if descriptors.len() > MAX_BROWSER_PACKAGE_ASSETS {
            return Err(WebHostError::LimitExceeded {
                resource: "browser package assets".to_owned(),
                limit: MAX_BROWSER_PACKAGE_ASSETS,
            });
        }

        let mut package_assets = BTreeMap::new();
        let mut fetch_paths = BTreeSet::new();
        for descriptor in descriptors {
            let package_id =
                descriptor_package_id(descriptor).ok_or_else(|| WebHostError::InvalidInput {
                    field: "browser package asset descriptor".to_owned(),
                    reason: "asset URL does not canonically contain its fetch path".to_owned(),
                })?;
            descriptor
                .validate_for_package(package_id)
                .map_err(|error| WebHostError::InvalidInput {
                    field: "browser package asset descriptor".to_owned(),
                    reason: error.to_string(),
                })?;
            if !fetch_paths.insert(descriptor.fetch_path.clone()) {
                return Err(WebHostError::InvalidInput {
                    field: "browser package asset descriptor".to_owned(),
                    reason: "package asset allowlist repeats a same-origin fetch path".to_owned(),
                });
            }
            let registered = RegisteredPackageAsset {
                digest: decode_sha256(&descriptor.bytes_sha256).map_err(|failure| {
                    WebHostError::InvalidInput {
                        field: "browser package asset descriptor".to_owned(),
                        reason: failure.diagnostic,
                    }
                })?,
                display_name: package_asset_display_name(&descriptor.fetch_path),
                descriptor: Rc::new(descriptor.clone()),
            };
            if package_assets
                .insert(descriptor.url.clone(), registered)
                .is_some()
            {
                return Err(WebHostError::InvalidInput {
                    field: "browser package asset descriptor".to_owned(),
                    reason: "package asset allowlist repeats a canonical URL".to_owned(),
                });
            }
        }
        self.package_assets = package_assets;
        Ok(())
    }

    pub fn register_source(&mut self, file: File) -> WebHostResult<Value> {
        let binding =
            self.register_resource(BrowserFileResource::Source(file), BrowserFileAccess::Source)?;
        Ok(Value::host_bound(
            tagged("FileSelected", BTreeMap::new()),
            binding,
        ))
    }

    pub fn register_target(&mut self, target: FileSystemFileHandle) -> WebHostResult<Value> {
        let binding = self.register_resource(
            BrowserFileResource::Target(target),
            BrowserFileAccess::Target,
        )?;
        Ok(Value::host_bound(
            tagged("FileTarget", BTreeMap::new()),
            binding,
        ))
    }

    fn register_resource(
        &mut self,
        resource: BrowserFileResource,
        access: BrowserFileAccess,
    ) -> WebHostResult<HostValueBinding> {
        for _ in 0..CAPABILITY_TOKEN_ATTEMPTS {
            let mut handle = [0_u8; CAPABILITY_TOKEN_BYTES];
            getrandom::fill(&mut handle).map_err(|error| WebHostError::Platform {
                operation: "mint browser file capability".to_owned(),
                message: error.to_string(),
            })?;
            match self.capabilities.register(handle, resource.clone(), access) {
                Ok(binding) => return Ok(binding),
                Err(error) if error.kind() == HostCapabilityErrorKind::DuplicateHandle => {}
                Err(error) => return Err(capability_error(error)),
            }
        }
        Err(WebHostError::InvalidInput {
            field: "browser file capability".to_owned(),
            reason: "could not mint a unique bounded handle".to_owned(),
        })
    }

    pub fn submit(
        &mut self,
        operation: BrowserFileEffectOperation,
        invocation: TransientEffectInvocation,
    ) -> WebHostResult<()> {
        validate_invocation(operation, &invocation)?;
        if self.shared.calls.borrow().active.len() >= self.limits.max_active {
            return Err(WebHostError::LimitExceeded {
                resource: "active browser File/Content effects".to_owned(),
                limit: self.limits.max_active,
            });
        }
        if operation.owns_global_writer() && self.shared.calls.borrow().writer_busy.is_some() {
            return self.queue_immediate(invocation, operation, tagged("Busy", BTreeMap::new()));
        }

        let decoded = match self.decode(operation, &invocation.intent) {
            Ok(decoded) => decoded,
            Err(failure) => {
                return self.queue_immediate(invocation, operation, failure.outcome());
            }
        };
        let call_id = invocation.call_id;
        if let Some(nonce) = self.start(invocation, operation, decoded)? {
            let still_active = self
                .shared
                .calls
                .borrow()
                .active
                .get(&call_id)
                .is_some_and(|active| active.nonce == nonce);
            if still_active && let Err(error) = self.schedule_deadline(call_id, nonce) {
                self.shared.queue.borrow_mut().remove_call(call_id);
                discard_call(&self.shared, call_id);
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn grant_credits(&mut self, grant: TransientEffectCreditGrant) -> WebHostResult<()> {
        if grant.credits == 0 {
            return Err(WebHostError::InvalidInput {
                field: "browser File/Content stream credit".to_owned(),
                reason: "credit grant must be positive".to_owned(),
            });
        }
        {
            let mut calls = self.shared.calls.borrow_mut();
            let active =
                calls
                    .active
                    .get_mut(&grant.call_id)
                    .ok_or_else(|| WebHostError::InvalidInput {
                        field: "browser File/Content stream credit".to_owned(),
                        reason: format!("credit targets stale or foreign call {}", grant.call_id),
                    })?;
            if !active.operation.is_stream() || active.cancelled {
                return Err(WebHostError::InvalidInput {
                    field: "browser File/Content stream credit".to_owned(),
                    reason: "credit targets a non-stream or cancelled call".to_owned(),
                });
            }
            let credits = active.credits.checked_add(grant.credits).ok_or_else(|| {
                WebHostError::InvalidInput {
                    field: "browser File/Content stream credit".to_owned(),
                    reason: "credit counter overflow".to_owned(),
                }
            })?;
            if credits > active.max_in_flight {
                return Err(WebHostError::InvalidInput {
                    field: "browser File/Content stream credit".to_owned(),
                    reason: format!(
                        "outstanding credits exceed canonical in-flight limit {}",
                        active.max_in_flight
                    ),
                });
            }
            active.credits = credits;
        }
        pump(&self.shared, grant.call_id);
        Ok(())
    }

    /// Discards a runtime-owned call. Any later promise resolution is ignored.
    pub fn cancel(&mut self, call_id: TransientEffectCallId) -> bool {
        self.clear_deadline(call_id);
        let removed_ready = self.shared.queue.borrow_mut().remove_call(call_id);
        let abort = {
            let mut calls = self.shared.calls.borrow_mut();
            let Some(active) = calls.active.get_mut(&call_id) else {
                return removed_ready;
            };
            active.cancelled = true;
            active.permit.discard();
            if active.abort_started {
                None
            } else {
                let package = active.package_abort_handles();
                let writer = active.writer();
                if package.is_some() || writer.is_some() {
                    active.abort_started = true;
                }
                if writer.is_some() {
                    active.pending_tasks = active.pending_tasks.saturating_add(1);
                }
                (package.is_some() || writer.is_some()).then_some((active.nonce, package, writer))
            }
        };
        if let Some((nonce, package, writer)) = abort {
            if let Some(package) = package {
                package.abort();
            }
            if let Some(writer) = writer {
                spawn_abort(self.shared.clone(), call_id, nonce, writer);
            } else {
                cleanup_cancelled_if_idle(&self.shared, call_id);
            }
        } else {
            cleanup_cancelled_if_idle(&self.shared, call_id);
        }
        true
    }

    pub fn dequeue_notification(&mut self) -> Option<BrowserFileEffectNotification> {
        let notification = self.shared.queue.borrow_mut().pop();
        if notification
            .as_ref()
            .is_some_and(|notification| notification.terminal)
            && let Some(notification) = notification.as_ref()
        {
            self.clear_deadline(notification.call_id);
        }
        if notification.is_some() {
            pump_all(&self.shared);
        }
        notification
    }

    fn schedule_deadline(
        &mut self,
        call_id: TransientEffectCallId,
        nonce: u64,
    ) -> WebHostResult<()> {
        if self.deadlines.contains_key(&call_id) {
            return Err(WebHostError::InvalidInput {
                field: "browser File/Content deadline".to_owned(),
                reason: format!("call {call_id} already owns a deadline"),
            });
        }
        let shared = self.shared.clone();
        let callback = Closure::wrap(Box::new(move || {
            timeout_call(&shared, call_id, nonce);
        }) as Box<dyn FnMut()>);
        let window = web_sys::window().ok_or_else(|| WebHostError::Platform {
            operation: "schedule browser File/Content deadline".to_owned(),
            message: "browser window is unavailable".to_owned(),
        })?;
        let timeout_id = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                self.limits.operation_timeout_ms,
            )
            .map_err(|error| WebHostError::Platform {
                operation: "schedule browser File/Content deadline".to_owned(),
                message: format!("{error:?}"),
            })?;
        let replaced = self.deadlines.insert(
            call_id,
            BrowserFileDeadline {
                timeout_id,
                _callback: callback,
            },
        );
        debug_assert!(replaced.is_none());
        Ok(())
    }

    fn clear_deadline(&mut self, call_id: TransientEffectCallId) {
        let Some(deadline) = self.deadlines.remove(&call_id) else {
            return;
        };
        if let Some(window) = web_sys::window() {
            window.clear_timeout_with_handle(deadline.timeout_id);
        }
    }

    #[cfg(test)]
    pub(crate) fn active_count(&self) -> usize {
        self.shared.calls.borrow().active.len()
    }

    #[cfg(test)]
    pub(crate) fn queued_count(&self) -> usize {
        self.shared.queue.borrow().entries.len()
    }

    fn decode(
        &self,
        operation: BrowserFileEffectOperation,
        intent: &Value,
    ) -> Result<DecodedOperation, FileFailure> {
        match operation {
            BrowserFileEffectOperation::ReadBytes => {
                let fields = exact_record(intent, &["file", "max_bytes"], "read-bytes intent")?;
                let source = self.resolve_source(required_field(fields, "file")?)?;
                let max_bytes = positive_usize(fields, "max_bytes")?;
                if max_bytes as u64 > FILE_BYTES_MAX_LIMIT {
                    return Err(FileFailure::invalid(
                        "read-bytes max_bytes exceeds the canonical bounded limit",
                    ));
                }
                if matches!(
                    &source,
                    ResolvedFileSource::Package(asset)
                        if asset.descriptor.bytes_len > max_bytes
                ) {
                    return Err(FileFailure::new(
                        "file_too_large",
                        "package asset exceeds the requested bounded byte limit",
                    ));
                }
                Ok(DecodedOperation::ReadBytes { source, max_bytes })
            }
            BrowserFileEffectOperation::WriteBytes => {
                let fields = exact_record(intent, &["bytes", "file"], "write-bytes intent")?;
                let target = self.resolve_target(required_field(fields, "file")?)?;
                let bytes = match required_field(fields, "bytes")? {
                    Value::Bytes(bytes) if bytes.len() as u64 <= FILE_BYTES_MAX_LIMIT => {
                        Rc::<[u8]>::from(bytes.as_ref())
                    }
                    Value::Bytes(_) => {
                        return Err(FileFailure::new(
                            "file_too_large",
                            "write-bytes payload exceeds the canonical bounded limit",
                        ));
                    }
                    _ => return Err(FileFailure::invalid("write-bytes bytes must be Bytes")),
                };
                Ok(DecodedOperation::WriteBytes { target, bytes })
            }
            BrowserFileEffectOperation::ReadStream => {
                let fields = exact_record(
                    intent,
                    &["chunk_bytes", "file", "retain_content"],
                    "read-stream intent",
                )?;
                let source = self.resolve_source(required_field(fields, "file")?)?;
                let chunk_bytes = positive_usize(fields, "chunk_bytes")?;
                if !(FILE_STREAM_MIN_CHUNK_BYTES as usize..=FILE_STREAM_MAX_CHUNK_BYTES as usize)
                    .contains(&chunk_bytes)
                {
                    return Err(FileFailure::invalid(
                        "read-stream chunk_bytes is outside the canonical bounded range",
                    ));
                }
                let retain_content = match required_field(fields, "retain_content")? {
                    Value::Bool(value) => *value,
                    _ => {
                        return Err(FileFailure::invalid(
                            "read-stream retain_content must be Bool",
                        ));
                    }
                };
                Ok(DecodedOperation::ReadStream {
                    source,
                    chunk_bytes,
                    retain_content,
                })
            }
            BrowserFileEffectOperation::ContentImport => {
                let fields = exact_record(intent, &["file"], "content import intent")?;
                let source = self.resolve_source(required_field(fields, "file")?)?;
                Ok(DecodedOperation::ContentImport { source })
            }
            BrowserFileEffectOperation::ContentSave => {
                let fields = exact_record(intent, &["content", "file"], "content save intent")?;
                let target = self.resolve_target(required_field(fields, "file")?)?;
                let content = ContentRef::from_value(required_field(fields, "content")?)
                    .map_err(|error| FileFailure::new("content_invalid", error.to_string()))?;
                Ok(DecodedOperation::ContentSave { target, content })
            }
        }
    }

    fn resolve_source(&self, value: &Value) -> Result<ResolvedFileSource, FileFailure> {
        let fields = exact_record(
            value.visible(),
            match value.visible() {
                Value::Record(fields) if matches!(fields.get("$tag"), Some(Value::Text(tag)) if tag == "PackageAsset") => {
                    &["$tag", "url"]
                }
                _ => &["$tag"],
            },
            "browser file source",
        )?;
        match fields.get("$tag") {
            Some(Value::Text(tag)) if tag == "FileSelected" => {
                let binding = value
                    .host_binding()
                    .ok_or_else(|| FileFailure::invalid("selected file has no host binding"))?;
                let resolved = self
                    .capabilities
                    .resolve(binding, BrowserFileAccess::Source)
                    .map_err(capability_lookup_failure)?;
                match resolved.resource {
                    BrowserFileResource::Source(file) => Ok(ResolvedFileSource::User(file.clone())),
                    BrowserFileResource::Target(_) => Err(FileFailure::invalid(
                        "selected file capability resolves to the wrong resource kind",
                    )),
                }
            }
            Some(Value::Text(tag)) if tag == "PackageAsset" => {
                if value.host_binding().is_some() {
                    return Err(FileFailure::invalid(
                        "package assets must not carry a host binding",
                    ));
                }
                let url = match required_field(fields, "url")? {
                    Value::Text(url) => url,
                    _ => return Err(FileFailure::invalid("package asset url must be Text")),
                };
                self.package_assets
                    .get(url)
                    .cloned()
                    .map(ResolvedFileSource::Package)
                    .ok_or_else(|| {
                        FileFailure::new(
                            "unknown_package_asset",
                            "package asset is absent from the browser bootstrap allowlist",
                        )
                    })
            }
            _ => Err(FileFailure::invalid(
                "file input must be FileSelected or PackageAsset",
            )),
        }
    }

    fn resolve_target(&self, value: &Value) -> Result<FileSystemFileHandle, FileFailure> {
        validate_bound_tag(value, "FileTarget")?;
        let binding = value
            .host_binding()
            .ok_or_else(|| FileFailure::invalid("file target has no host binding"))?;
        let resolved = self
            .capabilities
            .resolve(binding, BrowserFileAccess::Target)
            .map_err(capability_lookup_failure)?;
        match resolved.resource {
            BrowserFileResource::Target(target) => Ok(target.clone()),
            BrowserFileResource::Source(_) => Err(FileFailure::invalid(
                "file target capability resolves to the wrong resource kind",
            )),
        }
    }

    fn start(
        &mut self,
        invocation: TransientEffectInvocation,
        operation: BrowserFileEffectOperation,
        decoded: DecodedOperation,
    ) -> WebHostResult<Option<u64>> {
        let state = match ActiveOperationState::from_decoded(decoded) {
            Ok(state) => state,
            Err(failure) => {
                self.queue_immediate(invocation, operation, failure.outcome())?;
                return Ok(None);
            }
        };
        let nonce = self.shared.calls.borrow_mut().next_nonce()?;
        let permit = EffectCommitPermit::new();
        let mut active = ActiveCall {
            operation,
            nonce,
            permit,
            commit_guard: None,
            next_result_sequence: 0,
            credits: if operation.is_stream() {
                FILE_STREAM_INITIAL_CREDITS
            } else {
                0
            },
            max_in_flight: if operation.is_stream() {
                FILE_STREAM_MAX_IN_FLIGHT
            } else {
                0
            },
            pending_tasks: 0,
            cancelled: false,
            abort_started: false,
            validator: None,
            terminal_pending: None,
            state,
        };

        let initial = (|| -> Result<Option<Value>, FileFailure> {
            match &mut active.state {
                ActiveOperationState::ReadStream(state) => {
                    let (size, media, display_name) = state.source.metadata()?;
                    state.size = size;
                    state.media = media;
                    state.display_name = display_name;
                    let outcome = opened_outcome(size, &state.media, &state.display_name)?;
                    let mut validator = ByteStreamValidator::new(state.chunk_bytes)
                        .map_err(|error| FileFailure::invalid(error.to_string()))?;
                    let is_user_file = matches!(&state.source, ActiveReadSource::User(_));
                    if is_user_file {
                        validator
                            .accept(0, &outcome, false)
                            .map_err(|error| FileFailure::invalid(error.to_string()))?;
                        active.next_result_sequence = 1;
                    }
                    active.validator = Some(validator);
                    Ok(is_user_file.then_some(outcome))
                }
                ActiveOperationState::Import(state) => {
                    let (size, media, display_name) = state.source.metadata()?;
                    state.size = size;
                    state.media = media;
                    state.display_name = display_name;
                    let outcome = started_import_outcome(size, &state.media, &state.display_name)?;
                    let is_user_file = matches!(&state.source, ActiveReadSource::User(_));
                    if is_user_file {
                        active.next_result_sequence = 1;
                    }
                    Ok(is_user_file.then_some(outcome))
                }
                ActiveOperationState::Save(_) => Ok(None),
                ActiveOperationState::ReadBytes(_) | ActiveOperationState::WriteBytes(_) => {
                    Ok(None)
                }
            }
        })();
        let initial = match initial {
            Ok(initial) => initial,
            Err(failure) => {
                self.queue_immediate(invocation, operation, failure.outcome())?;
                return Ok(None);
            }
        };

        if let Some(outcome) = initial {
            let notification = BrowserFileEffectNotification {
                call_id: invocation.call_id,
                operation,
                result_sequence: Some(0),
                terminal: false,
                outcome,
            };
            if let Err(error) = self.shared.queue.borrow_mut().push(notification) {
                return Err(error);
            }
        }
        {
            let mut calls = self.shared.calls.borrow_mut();
            if operation.owns_global_writer() {
                calls.writer_busy = Some(invocation.call_id);
            }
            calls.active.insert(invocation.call_id, active);
        }
        if operation.is_stream() {
            (self.shared.wake)();
        }
        pump(&self.shared, invocation.call_id);
        Ok(Some(nonce))
    }

    fn queue_immediate(
        &mut self,
        invocation: TransientEffectInvocation,
        operation: BrowserFileEffectOperation,
        outcome: Value,
    ) -> WebHostResult<()> {
        self.shared
            .queue
            .borrow_mut()
            .push(BrowserFileEffectNotification {
                call_id: invocation.call_id,
                operation,
                result_sequence: operation.is_stream().then_some(0),
                terminal: true,
                outcome,
            })?;
        (self.shared.wake)();
        Ok(())
    }
}

impl Drop for BrowserFileEffectHost {
    fn drop(&mut self) {
        let calls = self
            .shared
            .calls
            .borrow()
            .active
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for call_id in calls {
            self.cancel(call_id);
        }
        let deadlines = std::mem::take(&mut self.deadlines);
        if let Some(window) = web_sys::window() {
            for deadline in deadlines.into_values() {
                window.clear_timeout_with_handle(deadline.timeout_id);
            }
        }
        self.shared.queue.borrow_mut().clear();
    }
}

enum DecodedOperation {
    ReadBytes {
        source: ResolvedFileSource,
        max_bytes: usize,
    },
    WriteBytes {
        target: FileSystemFileHandle,
        bytes: Rc<[u8]>,
    },
    ReadStream {
        source: ResolvedFileSource,
        chunk_bytes: usize,
        retain_content: bool,
    },
    ContentImport {
        source: ResolvedFileSource,
    },
    ContentSave {
        target: FileSystemFileHandle,
        content: ContentRef,
    },
}

#[derive(Clone)]
enum ResolvedFileSource {
    User(File),
    Package(RegisteredPackageAsset),
}

struct ActiveCall {
    operation: BrowserFileEffectOperation,
    nonce: u64,
    permit: EffectCommitPermit,
    commit_guard: Option<EffectCommitGuard>,
    next_result_sequence: u64,
    credits: u32,
    max_in_flight: u32,
    pending_tasks: u32,
    cancelled: bool,
    abort_started: bool,
    validator: Option<ByteStreamValidator>,
    terminal_pending: Option<Value>,
    state: ActiveOperationState,
}

impl ActiveCall {
    fn writer(&self) -> Option<FileSystemWritableFileStream> {
        match &self.state {
            ActiveOperationState::WriteBytes(state) => state.writer.clone(),
            ActiveOperationState::Save(state) => state.writer.clone(),
            ActiveOperationState::ReadBytes(_)
            | ActiveOperationState::ReadStream(_)
            | ActiveOperationState::Import(_) => None,
        }
    }

    fn take_writer(&mut self) -> Option<FileSystemWritableFileStream> {
        match &mut self.state {
            ActiveOperationState::WriteBytes(state) => state.writer.take(),
            ActiveOperationState::Save(state) => state.writer.take(),
            ActiveOperationState::ReadBytes(_)
            | ActiveOperationState::ReadStream(_)
            | ActiveOperationState::Import(_) => None,
        }
    }

    fn package_abort_handles(&self) -> Option<PackageAbortHandles> {
        self.state
            .package_read()
            .map(PackageAssetRead::abort_handles)
    }

    fn take_package_reservation(&mut self) -> Option<QueueReservation> {
        self.state
            .package_read_mut()
            .and_then(|package| package.held_reservation.take())
    }
}

enum ActiveOperationState {
    ReadBytes(ReadBytesState),
    WriteBytes(WriteBytesState),
    ReadStream(ReadStreamState),
    Import(ImportState),
    Save(SaveState),
}

impl ActiveOperationState {
    fn from_decoded(decoded: DecodedOperation) -> Result<Self, FileFailure> {
        Ok(match decoded {
            DecodedOperation::ReadBytes { source, max_bytes } => Self::ReadBytes(ReadBytesState {
                source: ActiveReadSource::new(source, max_bytes, max_bytes)?,
                max_bytes,
                started: false,
            }),
            DecodedOperation::WriteBytes { target, bytes } => Self::WriteBytes(WriteBytesState {
                target: Some(target),
                bytes,
                writer: None,
                phase: WritePhase::Opening,
            }),
            DecodedOperation::ReadStream {
                source,
                chunk_bytes,
                retain_content,
            } => Self::ReadStream(ReadStreamState {
                source: ActiveReadSource::new(
                    source,
                    chunk_bytes,
                    rechunk_buffer_limit(chunk_bytes)?,
                )?,
                size: 0,
                media: String::new(),
                display_name: String::new(),
                chunk_bytes,
                offset: 0,
                chunk_sequence: 0,
                digest: Sha256::new(),
                retained: retain_content.then_some(DurableImportState::Opening),
            }),
            DecodedOperation::ContentImport { source } => Self::Import(ImportState {
                source: ActiveReadSource::new(
                    source,
                    FILE_STREAM_DEFAULT_CHUNK_BYTES as usize,
                    rechunk_buffer_limit(FILE_STREAM_DEFAULT_CHUNK_BYTES as usize)?,
                )?,
                size: 0,
                media: String::new(),
                display_name: String::new(),
                offset: 0,
                digest: Sha256::new(),
                durable: DurableImportState::Opening,
            }),
            DecodedOperation::ContentSave { target, content } => Self::Save(SaveState {
                target: Some(target),
                content,
                offset: 0,
                next_chunk: 0,
                metadata: None,
                digest: Sha256::new(),
                writer: None,
                phase: WritePhase::Opening,
            }),
        })
    }

    fn package_read(&self) -> Option<&PackageAssetRead> {
        match self {
            Self::ReadBytes(state) => match &state.source {
                ActiveReadSource::Package(package) => Some(package),
                ActiveReadSource::User(_) => None,
            },
            Self::ReadStream(state) => match &state.source {
                ActiveReadSource::Package(package) => Some(package),
                ActiveReadSource::User(_) => None,
            },
            Self::Import(state) => match &state.source {
                ActiveReadSource::Package(package) => Some(package),
                ActiveReadSource::User(_) => None,
            },
            Self::WriteBytes(_) | Self::Save(_) => None,
        }
    }

    fn package_read_mut(&mut self) -> Option<&mut PackageAssetRead> {
        match self {
            Self::ReadBytes(state) => match &mut state.source {
                ActiveReadSource::Package(package) => Some(package),
                ActiveReadSource::User(_) => None,
            },
            Self::ReadStream(state) => match &mut state.source {
                ActiveReadSource::Package(package) => Some(package),
                ActiveReadSource::User(_) => None,
            },
            Self::Import(state) => match &mut state.source {
                ActiveReadSource::Package(package) => Some(package),
                ActiveReadSource::User(_) => None,
            },
            Self::WriteBytes(_) | Self::Save(_) => None,
        }
    }
}

struct ReadBytesState {
    source: ActiveReadSource,
    max_bytes: usize,
    started: bool,
}

enum ActiveReadSource {
    User(File),
    Package(PackageAssetRead),
}

impl ActiveReadSource {
    fn new(
        source: ResolvedFileSource,
        output_chunk_bytes: usize,
        max_buffer_bytes: usize,
    ) -> Result<Self, FileFailure> {
        match source {
            ResolvedFileSource::User(file) => Ok(Self::User(file)),
            ResolvedFileSource::Package(asset) => Ok(Self::Package(PackageAssetRead::new(
                asset,
                output_chunk_bytes,
                max_buffer_bytes,
            )?)),
        }
    }

    fn metadata(&self) -> Result<(u64, String, String), FileFailure> {
        match self {
            Self::User(file) => Ok((
                blob_size(file)?,
                file_media(file),
                bounded_text(&file.name(), MAX_DISPLAY_NAME_BYTES),
            )),
            Self::Package(package) => Ok((
                package.asset.descriptor.bytes_len as u64,
                package.asset.descriptor.media_type.clone(),
                package.asset.display_name.clone(),
            )),
        }
    }
}

struct PackageAssetRead {
    asset: RegisteredPackageAsset,
    controller: AbortController,
    reader: Option<ReadableStreamDefaultReader>,
    open_started: bool,
    held_reservation: Option<QueueReservation>,
    body: PackageStreamState,
}

impl PackageAssetRead {
    fn new(
        asset: RegisteredPackageAsset,
        output_chunk_bytes: usize,
        max_buffer_bytes: usize,
    ) -> Result<Self, FileFailure> {
        let controller = AbortController::new().map_err(|error| {
            js_failure(
                "fetch_failed",
                "cannot create package asset fetch cancellation",
                error,
            )
        })?;
        let body = PackageStreamState::new(
            asset.descriptor.bytes_len,
            asset.digest,
            output_chunk_bytes,
            max_buffer_bytes,
        )?;
        Ok(Self {
            asset,
            controller,
            reader: None,
            open_started: false,
            held_reservation: None,
            body,
        })
    }

    fn abort_handles(&self) -> PackageAbortHandles {
        PackageAbortHandles {
            controller: self.controller.clone(),
            reader: self.reader.clone(),
        }
    }
}

impl Drop for PackageAssetRead {
    fn drop(&mut self) {
        self.controller.abort();
        if let Some(reader) = &self.reader {
            let _ = reader.cancel();
        }
    }
}

struct PackageAbortHandles {
    controller: AbortController,
    reader: Option<ReadableStreamDefaultReader>,
}

impl PackageAbortHandles {
    fn abort(self) {
        self.controller.abort();
        if let Some(reader) = self.reader {
            let _ = reader.cancel();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WritePhase {
    Opening,
    Ready,
    Closing,
    Finished,
}

struct WriteBytesState {
    target: Option<FileSystemFileHandle>,
    bytes: Rc<[u8]>,
    writer: Option<FileSystemWritableFileStream>,
    phase: WritePhase,
}

struct ReadStreamState {
    source: ActiveReadSource,
    size: u64,
    media: String,
    display_name: String,
    chunk_bytes: usize,
    offset: u64,
    chunk_sequence: u64,
    digest: Sha256,
    retained: Option<DurableImportState>,
}

struct ImportState {
    source: ActiveReadSource,
    size: u64,
    media: String,
    display_name: String,
    offset: u64,
    digest: Sha256,
    durable: DurableImportState,
}

enum DurableImportState {
    Opening,
    Ready(BrowserContentImport),
    InFlight,
    Finished,
}

impl DurableImportState {
    fn take_ready(&mut self) -> Option<BrowserContentImport> {
        let previous = std::mem::replace(self, Self::InFlight);
        match previous {
            Self::Ready(import) => Some(import),
            other => {
                *self = other;
                None
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DurableImportOwner {
    ReadStream,
    ContentImport,
}

struct SaveState {
    target: Option<FileSystemFileHandle>,
    content: ContentRef,
    offset: u64,
    next_chunk: u32,
    metadata: Option<BrowserContentMetadata>,
    digest: Sha256,
    writer: Option<FileSystemWritableFileStream>,
    phase: WritePhase,
}

struct CallState {
    next_nonce: u64,
    writer_busy: Option<TransientEffectCallId>,
    active: BTreeMap<TransientEffectCallId, ActiveCall>,
}

impl CallState {
    fn new() -> Self {
        Self {
            next_nonce: 1,
            writer_busy: None,
            active: BTreeMap::new(),
        }
    }

    fn next_nonce(&mut self) -> WebHostResult<u64> {
        let nonce = self.next_nonce;
        self.next_nonce =
            self.next_nonce
                .checked_add(1)
                .ok_or_else(|| WebHostError::LimitExceeded {
                    resource: "browser File/Content call generation".to_owned(),
                    limit: usize::MAX,
                })?;
        Ok(nonce)
    }
}

#[derive(Clone, Copy)]
struct QueueReservation {
    bytes: usize,
}

struct PackageStreamState {
    expected_bytes: usize,
    expected_digest: [u8; 32],
    output_chunk_bytes: usize,
    max_buffer_bytes: usize,
    observed_bytes: usize,
    digest: Sha256,
    buffer: VecDeque<u8>,
    verified: bool,
}

impl PackageStreamState {
    fn new(
        expected_bytes: usize,
        expected_digest: [u8; 32],
        output_chunk_bytes: usize,
        max_buffer_bytes: usize,
    ) -> Result<Self, FileFailure> {
        if output_chunk_bytes == 0 || max_buffer_bytes < output_chunk_bytes {
            return Err(FileFailure::new(
                "invalid_state",
                "package stream re-chunk bounds are invalid",
            ));
        }
        Ok(Self {
            expected_bytes,
            expected_digest,
            output_chunk_bytes,
            max_buffer_bytes,
            observed_bytes: 0,
            digest: Sha256::new(),
            buffer: VecDeque::new(),
            verified: false,
        })
    }

    fn push_transport_chunk(&mut self, bytes: &[u8]) -> Result<(), FileFailure> {
        if self.verified {
            return Err(FileFailure::new(
                "package_asset_corrupt",
                "package asset transport produced bytes after EOF",
            ));
        }
        if bytes.is_empty() {
            return Err(FileFailure::new(
                "read_failed",
                "package asset transport produced an empty non-terminal chunk",
            ));
        }
        if bytes.len() > MAX_PACKAGE_TRANSPORT_CHUNK_BYTES {
            return Err(FileFailure::new(
                "transport_chunk_too_large",
                "package asset transport chunk exceeds the bounded reader limit",
            ));
        }
        let observed_bytes = self
            .observed_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| {
                FileFailure::new(
                    "package_asset_size_mismatch",
                    "package asset transport byte count overflowed",
                )
            })?;
        if observed_bytes > self.expected_bytes {
            return Err(FileFailure::new(
                "package_asset_size_mismatch",
                "package asset response exceeds its declared size",
            ));
        }
        let buffered_bytes = self.buffer.len().checked_add(bytes.len()).ok_or_else(|| {
            FileFailure::new(
                "transport_buffer_full",
                "package asset re-chunk buffer size overflowed",
            )
        })?;
        if buffered_bytes > self.max_buffer_bytes {
            return Err(FileFailure::new(
                "transport_buffer_full",
                "package asset re-chunk buffer exceeds its bounded capacity",
            ));
        }
        self.digest.update(bytes);
        self.buffer.extend(bytes.iter().copied());
        self.observed_bytes = observed_bytes;
        Ok(())
    }

    fn finish_transport(&mut self) -> Result<(), FileFailure> {
        if self.verified {
            return Err(FileFailure::new(
                "package_asset_corrupt",
                "package asset transport reported EOF more than once",
            ));
        }
        if self.observed_bytes != self.expected_bytes {
            return Err(FileFailure::new(
                "package_asset_size_mismatch",
                "package asset response differs from its declared size",
            ));
        }
        let digest = <[u8; 32]>::from(self.digest.clone().finalize());
        if digest != self.expected_digest {
            return Err(FileFailure::new(
                "package_asset_digest_mismatch",
                "package asset response differs from its declared SHA-256 digest",
            ));
        }
        self.verified = true;
        Ok(())
    }

    fn has_output(&self) -> bool {
        self.buffer.len() >= self.output_chunk_bytes || (self.verified && !self.buffer.is_empty())
    }

    fn take_output(&mut self) -> Option<Vec<u8>> {
        if !self.has_output() {
            return None;
        }
        let bytes = self.output_chunk_bytes.min(self.buffer.len());
        Some(self.buffer.drain(..bytes).collect())
    }

    fn take_all_verified(&mut self) -> Result<Vec<u8>, FileFailure> {
        if !self.verified || self.buffer.len() != self.expected_bytes {
            return Err(FileFailure::new(
                "package_asset_corrupt",
                "package asset bytes were requested before exact verification",
            ));
        }
        Ok(self.buffer.drain(..).collect())
    }

    fn needs_eof_probe(&self) -> bool {
        !self.verified && self.observed_bytes == self.expected_bytes && self.buffer.is_empty()
    }

    fn is_complete(&self) -> bool {
        self.verified && self.buffer.is_empty()
    }
}

enum PackageTransportRead {
    Chunk(Vec<u8>),
    Done,
}

struct QueuedNotification {
    notification: BrowserFileEffectNotification,
    bytes: usize,
}

struct BoundedNotificationQueue {
    max_items: usize,
    max_bytes: usize,
    used_bytes: usize,
    reserved_items: usize,
    reserved_bytes: usize,
    entries: VecDeque<QueuedNotification>,
}

impl BoundedNotificationQueue {
    fn new(max_items: usize, max_bytes: usize) -> Self {
        Self {
            max_items,
            max_bytes,
            used_bytes: 0,
            reserved_items: 0,
            reserved_bytes: 0,
            entries: VecDeque::new(),
        }
    }

    fn reserve(&mut self, bytes: usize) -> Option<QueueReservation> {
        if self.entries.len().saturating_add(self.reserved_items) >= self.max_items
            || self
                .used_bytes
                .saturating_add(self.reserved_bytes)
                .saturating_add(bytes)
                > self.max_bytes
        {
            return None;
        }
        self.reserved_items += 1;
        self.reserved_bytes += bytes;
        Some(QueueReservation { bytes })
    }

    fn release(&mut self, reservation: QueueReservation) {
        self.reserved_items = self.reserved_items.saturating_sub(1);
        self.reserved_bytes = self.reserved_bytes.saturating_sub(reservation.bytes);
    }

    fn finish(
        &mut self,
        reservation: QueueReservation,
        notification: BrowserFileEffectNotification,
    ) -> WebHostResult<()> {
        let bytes = notification_weight(&notification);
        self.release(reservation);
        if bytes > reservation.bytes {
            return Err(WebHostError::QueueOverflow {
                queue: "browser File/Content result bytes".to_owned(),
                capacity: reservation.bytes,
            });
        }
        self.push_known_weight(notification, bytes)
    }

    fn push(&mut self, notification: BrowserFileEffectNotification) -> WebHostResult<()> {
        let bytes = notification_weight(&notification);
        self.push_known_weight(notification, bytes)
    }

    fn push_known_weight(
        &mut self,
        notification: BrowserFileEffectNotification,
        bytes: usize,
    ) -> WebHostResult<()> {
        if self.entries.len().saturating_add(self.reserved_items) >= self.max_items {
            return Err(WebHostError::QueueOverflow {
                queue: "browser File/Content results".to_owned(),
                capacity: self.max_items,
            });
        }
        if self
            .used_bytes
            .saturating_add(self.reserved_bytes)
            .saturating_add(bytes)
            > self.max_bytes
        {
            return Err(WebHostError::QueueOverflow {
                queue: "browser File/Content result bytes".to_owned(),
                capacity: self.max_bytes,
            });
        }
        self.used_bytes += bytes;
        self.entries.push_back(QueuedNotification {
            notification,
            bytes,
        });
        Ok(())
    }

    fn pop(&mut self) -> Option<BrowserFileEffectNotification> {
        let queued = self.entries.pop_front()?;
        self.used_bytes = self.used_bytes.saturating_sub(queued.bytes);
        Some(queued.notification)
    }

    fn remove_call(&mut self, call_id: TransientEffectCallId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|queued| {
            if queued.notification.call_id == call_id {
                self.used_bytes = self.used_bytes.saturating_sub(queued.bytes);
                false
            } else {
                true
            }
        });
        self.entries.len() != before
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.used_bytes = 0;
    }
}

enum PumpAction {
    ReadWhole {
        file: File,
        max_bytes: usize,
        reservation: QueueReservation,
    },
    ReadStreamChunk {
        file: File,
        start: u64,
        end: u64,
        reservation: QueueReservation,
    },
    ImportChunk {
        file: File,
        start: u64,
        end: u64,
        reservation: QueueReservation,
    },
    BeginDurableImport {
        owner: DurableImportOwner,
        size: u64,
        media: String,
    },
    PersistDurableChunk {
        owner: DurableImportOwner,
        import: BrowserContentImport,
        bytes: Vec<u8>,
        reservation: QueueReservation,
    },
    FinishDurableImport {
        owner: DurableImportOwner,
        import: BrowserContentImport,
        digest: [u8; 32],
        byte_count: u64,
        reservation: QueueReservation,
        commit_guard: EffectCommitGuard,
    },
    ResolveContent {
        content: ContentRef,
        reservation: QueueReservation,
    },
    OpenPackage {
        fetch_path: String,
        controller: AbortController,
        reservation: QueueReservation,
    },
    ReadPackageTransport {
        reader: ReadableStreamDefaultReader,
        reservation: QueueReservation,
    },
    OpenWriter {
        target: FileSystemFileHandle,
    },
    WriteBytes {
        writer: FileSystemWritableFileStream,
        bytes: Rc<[u8]>,
    },
    SaveStoredChunk {
        writer: FileSystemWritableFileStream,
        content: ContentRef,
        sequence: u32,
        reservation: QueueReservation,
    },
    CloseWriter {
        writer: FileSystemWritableFileStream,
        reservation: QueueReservation,
    },
    Emit {
        outcome: Value,
        terminal: bool,
    },
    EmitReserved {
        outcome: Value,
        terminal: bool,
        reservation: QueueReservation,
    },
}

fn pump(shared: &SharedDriver, call_id: TransientEffectCallId) {
    loop {
        let action = plan_action(shared, call_id);
        let Some((nonce, action)) = action else {
            return;
        };
        match action {
            PumpAction::Emit { outcome, terminal } => {
                let retry = outcome.clone();
                if !emit_unreserved(shared, call_id, nonce, outcome, terminal) {
                    if let Some(active) = shared.calls.borrow_mut().active.get_mut(&call_id)
                        && active.nonce == nonce
                    {
                        active.terminal_pending = Some(retry);
                    }
                    return;
                }
                if terminal {
                    return;
                }
            }
            PumpAction::EmitReserved {
                outcome,
                terminal,
                reservation,
            } => {
                finish_task_with_reserved_event(
                    shared,
                    call_id,
                    nonce,
                    outcome,
                    terminal,
                    reservation,
                );
                return;
            }
            action => {
                spawn_action(shared.clone(), call_id, nonce, action);
                return;
            }
        }
    }
}

fn pump_all(shared: &SharedDriver) {
    let calls = shared
        .calls
        .borrow()
        .active
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for call_id in calls {
        pump(shared, call_id);
    }
}

fn plan_action(shared: &SharedDriver, call_id: TransientEffectCallId) -> Option<(u64, PumpAction)> {
    let mut calls = shared.calls.borrow_mut();
    let active = calls.active.get_mut(&call_id)?;
    if active.cancelled || active.pending_tasks > 0 {
        return None;
    }
    if active.permit.stop_reason() == Some(EffectStopReason::TimedOut) {
        return Some((
            active.nonce,
            PumpAction::Emit {
                outcome: timeout_failure().outcome(),
                terminal: true,
            },
        ));
    }
    if let Some(outcome) = active.terminal_pending.take() {
        return Some((
            active.nonce,
            PumpAction::Emit {
                outcome,
                terminal: true,
            },
        ));
    }

    let nonce = active.nonce;
    let reserve = |bytes| shared.queue.borrow_mut().reserve(bytes);
    let action = match &mut active.state {
        ActiveOperationState::ReadBytes(state) => match &mut state.source {
            ActiveReadSource::User(file) => {
                if state.started {
                    return None;
                }
                let reservation = reserve(
                    state
                        .max_bytes
                        .saturating_add(SMALL_RESULT_RESERVATION_BYTES),
                )?;
                state.started = true;
                active.pending_tasks += 1;
                PumpAction::ReadWhole {
                    file: file.clone(),
                    max_bytes: state.max_bytes,
                    reservation,
                }
            }
            ActiveReadSource::Package(package) if !package.open_started => {
                let reservation = reserve(
                    state
                        .max_bytes
                        .saturating_add(SMALL_RESULT_RESERVATION_BYTES),
                )?;
                package.open_started = true;
                state.started = true;
                active.pending_tasks += 1;
                PumpAction::OpenPackage {
                    fetch_path: package.asset.descriptor.fetch_path.clone(),
                    controller: package.controller.clone(),
                    reservation,
                }
            }
            ActiveReadSource::Package(package) => {
                let reservation = package.held_reservation.take()?;
                if package.body.verified {
                    let outcome = package
                        .body
                        .take_all_verified()
                        .and_then(|bytes| {
                            package
                                .asset
                                .descriptor
                                .verify_bytes(&bytes)
                                .map_err(|error| {
                                    FileFailure::new("package_asset_corrupt", error.to_string())
                                })?;
                            bytes_read_outcome(
                                bytes,
                                &package.asset.descriptor.media_type,
                                &package.asset.display_name,
                            )
                        })
                        .unwrap_or_else(FileFailure::outcome);
                    active.pending_tasks += 1;
                    PumpAction::EmitReserved {
                        outcome,
                        terminal: true,
                        reservation,
                    }
                } else {
                    let Some(reader) = package.reader.clone() else {
                        package.held_reservation = Some(reservation);
                        return None;
                    };
                    active.pending_tasks += 1;
                    PumpAction::ReadPackageTransport {
                        reader,
                        reservation,
                    }
                }
            }
        },
        ActiveOperationState::WriteBytes(state) => match state.phase {
            WritePhase::Opening => {
                let target = state.target.take()?;
                active.pending_tasks += 1;
                PumpAction::OpenWriter { target }
            }
            WritePhase::Ready => {
                let writer = state.writer.clone()?;
                state.phase = WritePhase::Closing;
                active.pending_tasks += 1;
                PumpAction::WriteBytes {
                    writer,
                    bytes: Rc::clone(&state.bytes),
                }
            }
            WritePhase::Closing => {
                let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                let commit_guard = match active.permit.begin_commit() {
                    Ok(guard) => guard,
                    Err(_) => {
                        shared.queue.borrow_mut().release(reservation);
                        return None;
                    }
                };
                active.commit_guard = Some(commit_guard);
                state.phase = WritePhase::Finished;
                active.pending_tasks += 1;
                PumpAction::CloseWriter {
                    writer: state.writer.clone()?,
                    reservation,
                }
            }
            WritePhase::Finished => return None,
        },
        ActiveOperationState::ReadStream(state) => {
            if matches!(state.retained, Some(DurableImportState::Opening)) {
                state.retained = Some(DurableImportState::InFlight);
                active.pending_tasks += 1;
                return Some((
                    nonce,
                    PumpAction::BeginDurableImport {
                        owner: DurableImportOwner::ReadStream,
                        size: state.size,
                        media: state.media.clone(),
                    },
                ));
            }
            let terminal_reservation = match &mut state.source {
                ActiveReadSource::User(file) if state.offset < state.size => {
                    if active.credits == 0 {
                        return None;
                    }
                    let end = state
                        .offset
                        .saturating_add(state.chunk_bytes as u64)
                        .min(state.size);
                    let chunk_bytes = usize::try_from(end - state.offset).ok()?;
                    let reservation =
                        reserve(chunk_bytes.saturating_add(SMALL_RESULT_RESERVATION_BYTES))?;
                    active.credits -= 1;
                    active.pending_tasks += 1;
                    return Some((
                        nonce,
                        PumpAction::ReadStreamChunk {
                            file: file.clone(),
                            start: state.offset,
                            end,
                            reservation,
                        },
                    ));
                }
                ActiveReadSource::User(_) => None,
                ActiveReadSource::Package(package) if !package.open_started => {
                    let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                    package.open_started = true;
                    active.pending_tasks += 1;
                    return Some((
                        nonce,
                        PumpAction::OpenPackage {
                            fetch_path: package.asset.descriptor.fetch_path.clone(),
                            controller: package.controller.clone(),
                            reservation,
                        },
                    ));
                }
                ActiveReadSource::Package(package) if !package.body.is_complete() => {
                    let reservation = match package.held_reservation.take() {
                        Some(reservation) => reservation,
                        None => {
                            let eof_probe = package.body.needs_eof_probe();
                            if !eof_probe && active.credits == 0 {
                                return None;
                            }
                            let bytes = if eof_probe {
                                SMALL_RESULT_RESERVATION_BYTES
                            } else {
                                state
                                    .chunk_bytes
                                    .saturating_add(SMALL_RESULT_RESERVATION_BYTES)
                            };
                            let reservation = reserve(bytes)?;
                            if !eof_probe {
                                active.credits -= 1;
                            }
                            reservation
                        }
                    };
                    if let Some(bytes) = package.body.take_output() {
                        if let Some(retained) = state.retained.as_mut() {
                            let Some(import) = retained.take_ready() else {
                                package.held_reservation = Some(reservation);
                                return None;
                            };
                            active.pending_tasks += 1;
                            return Some((
                                nonce,
                                PumpAction::PersistDurableChunk {
                                    owner: DurableImportOwner::ReadStream,
                                    import,
                                    bytes,
                                    reservation,
                                },
                            ));
                        }
                        let (outcome, terminal) = match accept_stream_chunk(state, bytes) {
                            Ok(outcome) => (outcome, false),
                            Err(error) => (error.outcome(), true),
                        };
                        active.pending_tasks += 1;
                        return Some((
                            nonce,
                            PumpAction::EmitReserved {
                                outcome,
                                terminal,
                                reservation,
                            },
                        ));
                    }
                    let Some(reader) = package.reader.clone() else {
                        package.held_reservation = Some(reservation);
                        return None;
                    };
                    active.pending_tasks += 1;
                    return Some((
                        nonce,
                        PumpAction::ReadPackageTransport {
                            reader,
                            reservation,
                        },
                    ));
                }
                ActiveReadSource::Package(package) => package.held_reservation.take(),
            };

            let digest = <[u8; 32]>::from(state.digest.clone().finalize());
            if let Some(retained) = state.retained.as_mut() {
                let import = retained.take_ready()?;
                let reservation = match terminal_reservation {
                    Some(reservation) => reservation,
                    None => reserve(SMALL_RESULT_RESERVATION_BYTES)?,
                };
                let commit_guard = match active.permit.begin_commit() {
                    Ok(guard) => guard,
                    Err(_) => {
                        shared.queue.borrow_mut().release(reservation);
                        return None;
                    }
                };
                active.pending_tasks += 1;
                return Some((
                    nonce,
                    PumpAction::FinishDurableImport {
                        owner: DurableImportOwner::ReadStream,
                        import,
                        digest,
                        byte_count: state.offset,
                        reservation,
                        commit_guard,
                    },
                ));
            }
            let retained = tagged("NotRetained", BTreeMap::new());
            let outcome = tagged(
                "Finished",
                BTreeMap::from([
                    ("byte_count".to_owned(), number(state.offset).ok()?),
                    ("digest".to_owned(), Value::Bytes(digest.to_vec().into())),
                    ("retained".to_owned(), retained),
                ]),
            );
            match terminal_reservation {
                Some(reservation) => {
                    active.pending_tasks += 1;
                    PumpAction::EmitReserved {
                        outcome,
                        terminal: true,
                        reservation,
                    }
                }
                None => PumpAction::Emit {
                    outcome,
                    terminal: true,
                },
            }
        }
        ActiveOperationState::Import(state) => {
            if matches!(state.durable, DurableImportState::Opening) {
                state.durable = DurableImportState::InFlight;
                active.pending_tasks += 1;
                return Some((
                    nonce,
                    PumpAction::BeginDurableImport {
                        owner: DurableImportOwner::ContentImport,
                        size: state.size,
                        media: state.media.clone(),
                    },
                ));
            }
            let terminal_reservation = match &mut state.source {
                ActiveReadSource::User(file) if state.offset < state.size => {
                    if active.credits == 0 {
                        return None;
                    }
                    let end = state
                        .offset
                        .saturating_add(FILE_STREAM_DEFAULT_CHUNK_BYTES as u64)
                        .min(state.size);
                    let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                    active.credits -= 1;
                    active.pending_tasks += 1;
                    return Some((
                        nonce,
                        PumpAction::ImportChunk {
                            file: file.clone(),
                            start: state.offset,
                            end,
                            reservation,
                        },
                    ));
                }
                ActiveReadSource::User(_) => None,
                ActiveReadSource::Package(package) if !package.open_started => {
                    let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                    package.open_started = true;
                    active.pending_tasks += 1;
                    return Some((
                        nonce,
                        PumpAction::OpenPackage {
                            fetch_path: package.asset.descriptor.fetch_path.clone(),
                            controller: package.controller.clone(),
                            reservation,
                        },
                    ));
                }
                ActiveReadSource::Package(package) if !package.body.is_complete() => {
                    let reservation = match package.held_reservation.take() {
                        Some(reservation) => reservation,
                        None => {
                            let eof_probe = package.body.needs_eof_probe();
                            if !eof_probe && active.credits == 0 {
                                return None;
                            }
                            let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                            if !eof_probe {
                                active.credits -= 1;
                            }
                            reservation
                        }
                    };
                    if let Some(bytes) = package.body.take_output() {
                        let Some(import) = state.durable.take_ready() else {
                            package.held_reservation = Some(reservation);
                            return None;
                        };
                        active.pending_tasks += 1;
                        return Some((
                            nonce,
                            PumpAction::PersistDurableChunk {
                                owner: DurableImportOwner::ContentImport,
                                import,
                                bytes,
                                reservation,
                            },
                        ));
                    }
                    let Some(reader) = package.reader.clone() else {
                        package.held_reservation = Some(reservation);
                        return None;
                    };
                    active.pending_tasks += 1;
                    return Some((
                        nonce,
                        PumpAction::ReadPackageTransport {
                            reader,
                            reservation,
                        },
                    ));
                }
                ActiveReadSource::Package(package) => package.held_reservation.take(),
            };

            let expected_digest = <[u8; 32]>::from(state.digest.clone().finalize());
            let import = state.durable.take_ready()?;
            let reservation = match terminal_reservation {
                Some(reservation) => reservation,
                None => reserve(SMALL_RESULT_RESERVATION_BYTES)?,
            };
            let commit_guard = match active.permit.begin_commit() {
                Ok(guard) => guard,
                Err(_) => {
                    shared.queue.borrow_mut().release(reservation);
                    return None;
                }
            };
            active.pending_tasks += 1;
            PumpAction::FinishDurableImport {
                owner: DurableImportOwner::ContentImport,
                import,
                digest: expected_digest,
                byte_count: state.offset,
                reservation,
                commit_guard,
            }
        }
        ActiveOperationState::Save(state) => {
            if state.metadata.is_none() {
                let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                active.pending_tasks += 1;
                PumpAction::ResolveContent {
                    content: state.content.clone(),
                    reservation,
                }
            } else {
                match state.phase {
                    WritePhase::Opening => {
                        let target = state.target.take()?;
                        active.pending_tasks += 1;
                        PumpAction::OpenWriter { target }
                    }
                    WritePhase::Ready if state.offset < state.content.size() => {
                        if active.credits == 0 {
                            return None;
                        }
                        let metadata = state.metadata.as_ref()?;
                        if state.next_chunk >= metadata.chunk_count() {
                            return Some((
                                nonce,
                                PumpAction::Emit {
                                    outcome: FileFailure::new(
                                        "content_corrupt",
                                        "stored content ended before its declared byte count",
                                    )
                                    .outcome(),
                                    terminal: true,
                                },
                            ));
                        }
                        let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                        active.credits -= 1;
                        active.pending_tasks += 1;
                        PumpAction::SaveStoredChunk {
                            writer: state.writer.clone()?,
                            content: state.content.clone(),
                            sequence: state.next_chunk,
                            reservation,
                        }
                    }
                    WritePhase::Ready => {
                        let metadata = state.metadata.as_ref()?;
                        let digest = <[u8; 32]>::from(state.digest.clone().finalize());
                        if state.next_chunk != metadata.chunk_count()
                            || state.offset != state.content.size()
                            || digest != state.content.digest()
                        {
                            return Some((
                                nonce,
                                PumpAction::Emit {
                                    outcome: FileFailure::new(
                                        "content_corrupt",
                                        "stored content differs from its durable descriptor",
                                    )
                                    .outcome(),
                                    terminal: true,
                                },
                            ));
                        }
                        let reservation = reserve(SMALL_RESULT_RESERVATION_BYTES)?;
                        let commit_guard = match active.permit.begin_commit() {
                            Ok(guard) => guard,
                            Err(_) => {
                                shared.queue.borrow_mut().release(reservation);
                                return None;
                            }
                        };
                        active.commit_guard = Some(commit_guard);
                        state.phase = WritePhase::Finished;
                        active.pending_tasks += 1;
                        PumpAction::CloseWriter {
                            writer: state.writer.clone()?,
                            reservation,
                        }
                    }
                    WritePhase::Closing | WritePhase::Finished => return None,
                }
            }
        }
    };
    Some((nonce, action))
}

fn spawn_action(
    shared: SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    action: PumpAction,
) {
    spawn_local(async move {
        match action {
            PumpAction::ReadWhole {
                file,
                max_bytes,
                reservation,
            } => {
                let size = blob_size(&file);
                let result = match size {
                    Ok(size) if size <= max_bytes as u64 => {
                        read_blob(&file).await.and_then(|bytes| {
                            if bytes.len() <= max_bytes {
                                Ok(bytes)
                            } else {
                                Err(FileFailure::new(
                                    "file_too_large",
                                    "selected file grew beyond the requested bounded byte limit",
                                ))
                            }
                        })
                    }
                    Ok(_) => Err(FileFailure::new(
                        "file_too_large",
                        "selected file exceeds the requested bounded byte limit",
                    )),
                    Err(error) => Err(error),
                };
                complete_read_whole(&shared, call_id, nonce, file, result, reservation);
            }
            PumpAction::ReadStreamChunk {
                file,
                start,
                end,
                reservation,
            } => {
                let result = read_blob_slice(&file, start, end).await;
                complete_read_stream_chunk(
                    &shared,
                    call_id,
                    nonce,
                    start,
                    end,
                    result,
                    reservation,
                )
                .await;
            }
            PumpAction::ImportChunk {
                file,
                start,
                end,
                reservation,
            } => {
                let result = read_blob_slice(&file, start, end).await;
                complete_import_chunk(&shared, call_id, nonce, start, end, result, reservation)
                    .await;
            }
            PumpAction::BeginDurableImport { owner, size, media } => {
                let result = shared
                    .content
                    .begin_import(size, media)
                    .await
                    .map_err(content_store_failure);
                complete_begin_durable_import(&shared, call_id, nonce, owner, result);
            }
            PumpAction::PersistDurableChunk {
                owner,
                import,
                bytes,
                reservation,
            } => {
                persist_durable_chunk(&shared, call_id, nonce, owner, import, bytes, reservation)
                    .await;
            }
            PumpAction::FinishDurableImport {
                owner,
                import,
                digest,
                byte_count,
                reservation,
                commit_guard,
            } => {
                let result = import.finish(digest).await.map_err(content_store_failure);
                commit_guard.finish();
                complete_finish_durable_import(
                    &shared,
                    call_id,
                    nonce,
                    owner,
                    byte_count,
                    digest,
                    result,
                    reservation,
                );
            }
            PumpAction::ResolveContent {
                content,
                reservation,
            } => {
                let result = shared
                    .content
                    .resolve_metadata(&content)
                    .await
                    .map_err(content_store_failure);
                complete_resolve_content(&shared, call_id, nonce, result, reservation);
            }
            PumpAction::OpenPackage {
                fetch_path,
                controller,
                reservation,
            } => {
                let result = open_package_fetch(&fetch_path, &controller).await;
                complete_open_package(&shared, call_id, nonce, result, reservation);
            }
            PumpAction::ReadPackageTransport {
                reader,
                reservation,
            } => {
                let result = read_package_transport(&reader).await;
                complete_package_transport(&shared, call_id, nonce, reader, result, reservation);
            }
            PumpAction::OpenWriter { target } => {
                let result = JsFuture::from(target.create_writable())
                    .await
                    .map_err(|error| {
                        js_failure("open_failed", "cannot open browser file target", error)
                    })
                    .and_then(browser_writable_from_value);
                complete_open_writer(&shared, call_id, nonce, result);
            }
            PumpAction::WriteBytes { writer, bytes } => {
                let result = writer.write_with_u8_array(bytes.as_ref()).map_err(|error| {
                    js_failure("write_failed", "cannot stage browser file bytes", error)
                });
                let result = match result {
                    Ok(promise) => JsFuture::from(promise).await.map(|_| ()).map_err(|error| {
                        js_failure("write_failed", "cannot stage browser file bytes", error)
                    }),
                    Err(error) => Err(error),
                };
                complete_write_bytes(&shared, call_id, nonce, result);
            }
            PumpAction::SaveStoredChunk {
                writer,
                content,
                sequence,
                reservation,
            } => {
                let result =
                    match shared.content.read_chunk(&content, sequence).await {
                        Ok(bytes) => {
                            let write = writer.write_with_u8_array(&bytes).map_err(|error| {
                                js_failure(
                                    "write_failed",
                                    "cannot stage retained browser content",
                                    error,
                                )
                            });
                            match write {
                                Ok(promise) => JsFuture::from(promise)
                                    .await
                                    .map(|_| bytes)
                                    .map_err(|error| {
                                        js_failure(
                                            "write_failed",
                                            "cannot stage retained browser content",
                                            error,
                                        )
                                    }),
                                Err(error) => Err(error),
                            }
                        }
                        Err(error) => Err(content_store_failure(error)),
                    };
                complete_save_chunk(&shared, call_id, nonce, sequence, result, reservation);
            }
            PumpAction::CloseWriter {
                writer,
                reservation,
            } => {
                let writable: &WritableStream = writer.unchecked_ref();
                let result = JsFuture::from(writable.close())
                    .await
                    .map(|_| ())
                    .map_err(|error| {
                        js_failure("commit_failed", "cannot commit browser file target", error)
                    });
                complete_close_writer(&shared, call_id, nonce, result, reservation);
            }
            PumpAction::Emit { .. } | PumpAction::EmitReserved { .. } => {
                unreachable!("emit actions are synchronous")
            }
        }
    });
}

fn browser_writable_from_value(
    value: JsValue,
) -> Result<FileSystemWritableFileStream, FileFailure> {
    for method in ["write", "close", "abort"] {
        let callable = Reflect::get(&value, &JsValue::from_str(method)).map_err(|error| {
            js_failure(
                "open_failed",
                "cannot inspect browser writable file stream",
                error,
            )
        })?;
        if !callable.is_function() {
            return Err(FileFailure::new(
                "open_failed",
                format!("browser writable file stream is missing {method}()"),
            ));
        }
    }
    Ok(value.unchecked_into())
}

async fn open_package_fetch(
    fetch_path: &str,
    controller: &AbortController,
) -> Result<ReadableStreamDefaultReader, FileFailure> {
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::SameOrigin);
    init.set_credentials(RequestCredentials::SameOrigin);
    init.set_redirect(RequestRedirect::Error);
    init.set_signal(Some(&controller.signal()));
    let request = Request::new_with_str_and_init(fetch_path, &init).map_err(|error| {
        js_failure(
            "fetch_failed",
            "cannot construct same-origin package asset request",
            error,
        )
    })?;
    let window = web_sys::window().ok_or_else(|| {
        FileFailure::new(
            "fetch_failed",
            "browser window is unavailable for package asset fetch",
        )
    })?;
    let response = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|error| {
            if controller.signal().aborted() {
                FileFailure::new("cancelled", "package asset fetch was cancelled")
            } else {
                js_failure(
                    "fetch_failed",
                    "same-origin package asset fetch failed",
                    error,
                )
            }
        })?
        .dyn_into::<Response>()
        .map_err(|error| {
            js_failure(
                "fetch_failed",
                "package asset fetch returned an invalid Response",
                error,
            )
        })?;
    if response.status() != 200 {
        return Err(FileFailure::new(
            "fetch_status",
            format!(
                "package asset fetch returned HTTP status {} instead of 200",
                response.status()
            ),
        ));
    }
    let body = response.body().ok_or_else(|| {
        FileFailure::new(
            "fetch_body_missing",
            "package asset fetch returned no ReadableStream body",
        )
    })?;
    body.get_reader()
        .dyn_into::<ReadableStreamDefaultReader>()
        .map_err(|error| {
            js_failure(
                "fetch_body_invalid",
                "package asset Response body returned an invalid reader",
                error.into(),
            )
        })
}

async fn read_package_transport(
    reader: &ReadableStreamDefaultReader,
) -> Result<PackageTransportRead, FileFailure> {
    let result = JsFuture::from(reader.read()).await.map_err(|error| {
        js_failure(
            "read_failed",
            "cannot read package asset Response body",
            error,
        )
    })?;
    let done = Reflect::get(&result, &JsValue::from_str("done"))
        .map_err(|error| {
            js_failure(
                "read_failed",
                "cannot inspect package asset stream completion",
                error,
            )
        })?
        .as_bool()
        .ok_or_else(|| {
            FileFailure::new(
                "read_failed",
                "package asset stream completion flag is not Boolean",
            )
        })?;
    if done {
        return Ok(PackageTransportRead::Done);
    }
    let value = Reflect::get(&result, &JsValue::from_str("value")).map_err(|error| {
        js_failure(
            "read_failed",
            "cannot inspect package asset stream chunk",
            error,
        )
    })?;
    let chunk = value.dyn_into::<Uint8Array>().map_err(|error| {
        js_failure(
            "read_failed",
            "package asset stream chunk is not Uint8Array",
            error.into(),
        )
    })?;
    let chunk_len = usize::try_from(chunk.length()).unwrap_or(usize::MAX);
    if chunk_len > MAX_PACKAGE_TRANSPORT_CHUNK_BYTES {
        return Err(FileFailure::new(
            "transport_chunk_too_large",
            "package asset transport chunk exceeds the bounded reader limit",
        ));
    }
    if chunk_len == 0 {
        return Err(FileFailure::new(
            "read_failed",
            "package asset transport produced an empty non-terminal chunk",
        ));
    }
    let mut bytes = vec![0_u8; chunk_len];
    chunk.copy_to(&mut bytes);
    Ok(PackageTransportRead::Chunk(bytes))
}

fn complete_open_package(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    result: Result<ReadableStreamDefaultReader, FileFailure>,
    reservation: QueueReservation,
) {
    let current = shared
        .calls
        .borrow()
        .active
        .get(&call_id)
        .is_some_and(|active| active.nonce == nonce);
    if !current {
        if let Ok(reader) = result {
            let _ = reader.cancel();
        }
        shared.queue.borrow_mut().release(reservation);
        return;
    }
    let cancelled = shared
        .calls
        .borrow()
        .active
        .get(&call_id)
        .is_some_and(|active| active.cancelled);
    if cancelled {
        if let Ok(reader) = result {
            let _ = reader.cancel();
        }
        finish_task_with_reserved_event(
            shared,
            call_id,
            nonce,
            FileFailure::new("cancelled", "package asset fetch was cancelled").outcome(),
            true,
            reservation,
        );
        return;
    }
    let reader = match result {
        Ok(reader) => reader,
        Err(error) => {
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                error.outcome(),
                true,
                reservation,
            );
            return;
        }
    };

    let outcome = {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce && !active.cancelled)
        else {
            let _ = reader.cancel();
            shared.queue.borrow_mut().release(reservation);
            return;
        };
        match &mut active.state {
            ActiveOperationState::ReadBytes(state) => {
                if let ActiveReadSource::Package(package) = &mut state.source {
                    package.reader = Some(reader.clone());
                    package.held_reservation = Some(reservation);
                    active.pending_tasks = active.pending_tasks.saturating_sub(1);
                    None
                } else {
                    let _ = reader.cancel();
                    Some(Err(FileFailure::new(
                        "invalid_state",
                        "package fetch completed for a user file",
                    )))
                }
            }
            ActiveOperationState::ReadStream(state) => {
                if let ActiveReadSource::Package(package) = &mut state.source {
                    package.reader = Some(reader.clone());
                    Some(opened_outcome(
                        state.size,
                        &state.media,
                        &state.display_name,
                    ))
                } else {
                    let _ = reader.cancel();
                    Some(Err(FileFailure::new(
                        "invalid_state",
                        "package fetch completed for a user file stream",
                    )))
                }
            }
            ActiveOperationState::Import(state) => {
                if let ActiveReadSource::Package(package) = &mut state.source {
                    package.reader = Some(reader.clone());
                    Some(started_import_outcome(
                        state.size,
                        &state.media,
                        &state.display_name,
                    ))
                } else {
                    let _ = reader.cancel();
                    Some(Err(FileFailure::new(
                        "invalid_state",
                        "package fetch completed for a user content import",
                    )))
                }
            }
            ActiveOperationState::WriteBytes(_) | ActiveOperationState::Save(_) => {
                let _ = reader.cancel();
                Some(Err(FileFailure::new(
                    "invalid_state",
                    "package fetch completed for a writing operation",
                )))
            }
        }
    };
    match outcome {
        None => pump(shared, call_id),
        Some(outcome) => {
            let terminal = outcome.is_err();
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                outcome.unwrap_or_else(FileFailure::outcome),
                terminal,
                reservation,
            );
        }
    }
}

fn complete_package_transport(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    reader: ReadableStreamDefaultReader,
    result: Result<PackageTransportRead, FileFailure>,
    reservation: QueueReservation,
) {
    let ingest = result.and_then(|read| {
        let mut calls = shared.calls.borrow_mut();
        let active = matching_active(&mut calls, call_id, nonce)?;
        let package = active.state.package_read_mut().ok_or_else(|| {
            FileFailure::new(
                "invalid_state",
                "package stream promise completed for another operation",
            )
        })?;
        match read {
            PackageTransportRead::Chunk(bytes) => package.body.push_transport_chunk(&bytes),
            PackageTransportRead::Done => package.body.finish_transport(),
        }
    });
    if let Err(error) = ingest {
        let _ = reader.cancel();
        finish_task_with_reserved_event(shared, call_id, nonce, error.outcome(), true, reservation);
        return;
    }
    {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce && !active.cancelled)
        else {
            let _ = reader.cancel();
            shared.queue.borrow_mut().release(reservation);
            return;
        };
        let Some(package) = active.state.package_read_mut() else {
            let _ = reader.cancel();
            shared.queue.borrow_mut().release(reservation);
            return;
        };
        package.held_reservation = Some(reservation);
        active.pending_tasks = active.pending_tasks.saturating_sub(1);
    }
    pump(shared, call_id);
}

fn bytes_read_outcome(
    bytes: Vec<u8>,
    media: &str,
    display_name: &str,
) -> Result<Value, FileFailure> {
    Ok(tagged(
        "BytesRead",
        BTreeMap::from([
            ("byte_count".to_owned(), number(bytes.len() as u64)?),
            ("bytes".to_owned(), Value::Bytes(bytes.into())),
            ("media".to_owned(), Value::Text(media.to_owned())),
            (
                "display_name".to_owned(),
                Value::Text(display_name.to_owned()),
            ),
        ]),
    ))
}

fn accept_stream_chunk(state: &mut ReadStreamState, bytes: Vec<u8>) -> Result<Value, FileFailure> {
    if bytes.is_empty() || bytes.len() > state.chunk_bytes {
        return Err(FileFailure::new(
            "read_failed",
            "browser stream produced a non-canonical output chunk",
        ));
    }
    let start = state.offset;
    let end = start
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| FileFailure::new("file_too_large", "browser stream offset overflowed"))?;
    if end > state.size {
        return Err(FileFailure::new(
            "package_asset_size_mismatch",
            "browser stream output exceeds its declared size",
        ));
    }
    state.digest.update(&bytes);
    let outcome = tagged(
        "Chunk",
        BTreeMap::from([
            ("sequence".to_owned(), number(state.chunk_sequence)?),
            ("offset".to_owned(), number(start)?),
            ("bytes".to_owned(), Value::Bytes(bytes.into())),
        ]),
    );
    state.offset = end;
    state.chunk_sequence = state
        .chunk_sequence
        .checked_add(1)
        .ok_or_else(|| FileFailure::new("file_too_large", "browser chunk sequence overflow"))?;
    Ok(outcome)
}

fn accept_import_chunk(state: &mut ImportState, bytes: Vec<u8>) -> Result<Value, FileFailure> {
    if bytes.is_empty() || bytes.len() > FILE_STREAM_DEFAULT_CHUNK_BYTES as usize {
        return Err(FileFailure::new(
            "read_failed",
            "browser import produced a non-canonical output chunk",
        ));
    }
    let end = state
        .offset
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| FileFailure::new("file_too_large", "browser import offset overflowed"))?;
    if end > state.size {
        return Err(FileFailure::new(
            "package_asset_size_mismatch",
            "browser import output exceeds its declared size",
        ));
    }
    state.digest.update(&bytes);
    state.offset = end;
    Ok(tagged(
        "Progress",
        BTreeMap::from([
            ("completed_bytes".to_owned(), number(end)?),
            ("total_bytes".to_owned(), number(state.size)?),
        ]),
    ))
}

fn complete_read_whole(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    file: File,
    result: Result<Vec<u8>, FileFailure>,
    reservation: QueueReservation,
) {
    let outcome = result.and_then(|bytes| {
        bytes_read_outcome(
            bytes,
            &file_media(&file),
            &bounded_text(&file.name(), MAX_DISPLAY_NAME_BYTES),
        )
    });
    finish_task_with_reserved_event(
        shared,
        call_id,
        nonce,
        outcome.unwrap_or_else(FileFailure::outcome),
        true,
        reservation,
    );
}

async fn complete_read_stream_chunk(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    start: u64,
    end: u64,
    result: Result<Vec<u8>, FileFailure>,
    reservation: QueueReservation,
) {
    let bytes = match result.and_then(|bytes| {
        if bytes.len() as u64 != end - start || bytes.is_empty() {
            return Err(FileFailure::new(
                "read_failed",
                "browser Blob slice returned a non-exact chunk",
            ));
        }
        Ok(bytes)
    }) {
        Ok(bytes) => bytes,
        Err(error) => {
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                error.outcome(),
                true,
                reservation,
            );
            return;
        }
    };
    let import = {
        let mut calls = shared.calls.borrow_mut();
        let active = match matching_active(&mut calls, call_id, nonce) {
            Ok(active) => active,
            Err(error) => {
                drop(calls);
                finish_task_with_reserved_event(
                    shared,
                    call_id,
                    nonce,
                    error.outcome(),
                    true,
                    reservation,
                );
                return;
            }
        };
        let ActiveOperationState::ReadStream(state) = &mut active.state else {
            drop(calls);
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                FileFailure::new(
                    "invalid_state",
                    "read-stream promise completed for another operation",
                )
                .outcome(),
                true,
                reservation,
            );
            return;
        };
        if state.offset != start {
            drop(calls);
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                FileFailure::new(
                    "invalid_state",
                    "browser Blob chunk completed at a stale stream offset",
                )
                .outcome(),
                true,
                reservation,
            );
            return;
        }
        state
            .retained
            .as_mut()
            .and_then(DurableImportState::take_ready)
    };
    if let Some(import) = import {
        persist_durable_chunk(
            shared,
            call_id,
            nonce,
            DurableImportOwner::ReadStream,
            import,
            bytes,
            reservation,
        )
        .await;
        return;
    }
    let outcome = {
        let mut calls = shared.calls.borrow_mut();
        matching_active(&mut calls, call_id, nonce).and_then(|active| {
            let ActiveOperationState::ReadStream(state) = &mut active.state else {
                return Err(FileFailure::new(
                    "invalid_state",
                    "read-stream promise completed for another operation",
                ));
            };
            accept_stream_chunk(state, bytes)
        })
    };
    let terminal = outcome.is_err();
    finish_task_with_reserved_event(
        shared,
        call_id,
        nonce,
        outcome.unwrap_or_else(FileFailure::outcome),
        terminal,
        reservation,
    );
}

async fn complete_import_chunk(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    start: u64,
    end: u64,
    result: Result<Vec<u8>, FileFailure>,
    reservation: QueueReservation,
) {
    let bytes = match result.and_then(|bytes| {
        if bytes.len() as u64 != end - start || bytes.is_empty() {
            return Err(FileFailure::new(
                "read_failed",
                "browser Blob slice returned a non-exact import chunk",
            ));
        }
        Ok(bytes)
    }) {
        Ok(bytes) => bytes,
        Err(error) => {
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                error.outcome(),
                true,
                reservation,
            );
            return;
        }
    };
    let import = {
        let mut calls = shared.calls.borrow_mut();
        let active = match matching_active(&mut calls, call_id, nonce) {
            Ok(active) => active,
            Err(error) => {
                drop(calls);
                finish_task_with_reserved_event(
                    shared,
                    call_id,
                    nonce,
                    error.outcome(),
                    true,
                    reservation,
                );
                return;
            }
        };
        let ActiveOperationState::Import(state) = &mut active.state else {
            drop(calls);
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                FileFailure::new(
                    "invalid_state",
                    "content-import promise completed for another operation",
                )
                .outcome(),
                true,
                reservation,
            );
            return;
        };
        if state.offset != start {
            drop(calls);
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                FileFailure::new(
                    "invalid_state",
                    "browser Blob import completed at a stale stream offset",
                )
                .outcome(),
                true,
                reservation,
            );
            return;
        }
        state.durable.take_ready()
    };
    let Some(import) = import else {
        finish_task_with_reserved_event(
            shared,
            call_id,
            nonce,
            FileFailure::new(
                "invalid_state",
                "content import has no current durable staging owner",
            )
            .outcome(),
            true,
            reservation,
        );
        return;
    };
    persist_durable_chunk(
        shared,
        call_id,
        nonce,
        DurableImportOwner::ContentImport,
        import,
        bytes,
        reservation,
    )
    .await;
}

fn complete_begin_durable_import(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    owner: DurableImportOwner,
    result: Result<BrowserContentImport, FileFailure>,
) {
    {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            return;
        };
        active.pending_tasks = active.pending_tasks.saturating_sub(1);
        if !active.cancelled {
            let installed = match result {
                Ok(import) => install_durable_import(&mut active.state, owner, import),
                Err(error) => Err(error),
            };
            if let Err(error) = installed {
                if let Ok(slot) = durable_import_slot_mut(&mut active.state, owner) {
                    *slot = DurableImportState::Finished;
                }
                active.terminal_pending = Some(error.outcome());
            }
        }
    }
    cleanup_cancelled_if_idle(shared, call_id);
    pump(shared, call_id);
}

fn install_durable_import(
    state: &mut ActiveOperationState,
    owner: DurableImportOwner,
    import: BrowserContentImport,
) -> Result<(), FileFailure> {
    let expected_size = match (&*state, owner) {
        (ActiveOperationState::ReadStream(state), DurableImportOwner::ReadStream) => state.size,
        (ActiveOperationState::Import(state), DurableImportOwner::ContentImport) => state.size,
        _ => {
            return Err(FileFailure::new(
                "invalid_state",
                "durable content staging opened for another operation",
            ));
        }
    };
    if import.declared_size() != expected_size {
        return Err(FileFailure::new(
            "content_corrupt",
            "durable content staging differs from its declared source size",
        ));
    }
    let slot = durable_import_slot_mut(state, owner)?;
    if !matches!(slot, DurableImportState::InFlight) {
        return Err(FileFailure::new(
            "invalid_state",
            "durable content staging completed outside its in-flight state",
        ));
    }
    *slot = DurableImportState::Ready(import);
    Ok(())
}

fn durable_import_slot_mut(
    state: &mut ActiveOperationState,
    owner: DurableImportOwner,
) -> Result<&mut DurableImportState, FileFailure> {
    match (state, owner) {
        (ActiveOperationState::ReadStream(state), DurableImportOwner::ReadStream) => {
            state.retained.as_mut().ok_or_else(|| {
                FileFailure::new(
                    "invalid_state",
                    "non-retaining byte stream received durable content work",
                )
            })
        }
        (ActiveOperationState::Import(state), DurableImportOwner::ContentImport) => {
            Ok(&mut state.durable)
        }
        _ => Err(FileFailure::new(
            "invalid_state",
            "durable content work completed for another operation",
        )),
    }
}

async fn persist_durable_chunk(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    owner: DurableImportOwner,
    mut import: BrowserContentImport,
    bytes: Vec<u8>,
    reservation: QueueReservation,
) {
    let sequence = import.next_sequence();
    let progress = match import
        .append_chunk(sequence, &bytes)
        .await
        .map_err(content_store_failure)
    {
        Ok(progress) => progress,
        Err(error) => {
            finish_task_with_reserved_event(
                shared,
                call_id,
                nonce,
                error.outcome(),
                true,
                reservation,
            );
            return;
        }
    };

    let outcome = {
        let mut calls = shared.calls.borrow_mut();
        matching_active(&mut calls, call_id, nonce).and_then(|active| {
            match (&mut active.state, owner) {
                (ActiveOperationState::ReadStream(state), DurableImportOwner::ReadStream) => {
                    if !matches!(state.retained, Some(DurableImportState::InFlight)) {
                        return Err(FileFailure::new(
                            "invalid_state",
                            "retained stream chunk completed without in-flight durable staging",
                        ));
                    }
                    let outcome = accept_stream_chunk(state, bytes)?;
                    let digest = <[u8; 32]>::from(state.digest.clone().finalize());
                    if progress.written_size != state.offset
                        || progress.next_sequence != sequence.saturating_add(1)
                        || progress.prefix_sha256 != digest
                        || import.written_size() != state.offset
                        || import.next_sequence() != progress.next_sequence
                    {
                        return Err(FileFailure::new(
                            "content_corrupt",
                            "retained stream progress differs from durable content staging",
                        ));
                    }
                    state.retained = Some(DurableImportState::Ready(import));
                    Ok(outcome)
                }
                (ActiveOperationState::Import(state), DurableImportOwner::ContentImport) => {
                    if !matches!(state.durable, DurableImportState::InFlight) {
                        return Err(FileFailure::new(
                            "invalid_state",
                            "content import chunk completed without in-flight durable staging",
                        ));
                    }
                    let outcome = accept_import_chunk(state, bytes)?;
                    let digest = <[u8; 32]>::from(state.digest.clone().finalize());
                    if progress.written_size != state.offset
                        || progress.next_sequence != sequence.saturating_add(1)
                        || progress.prefix_sha256 != digest
                        || import.written_size() != state.offset
                        || import.next_sequence() != progress.next_sequence
                    {
                        return Err(FileFailure::new(
                            "content_corrupt",
                            "content-import progress differs from durable content staging",
                        ));
                    }
                    state.durable = DurableImportState::Ready(import);
                    Ok(outcome)
                }
                _ => Err(FileFailure::new(
                    "invalid_state",
                    "durable content chunk completed for another operation",
                )),
            }
        })
    };
    let terminal = outcome.is_err();
    finish_task_with_reserved_event(
        shared,
        call_id,
        nonce,
        outcome.unwrap_or_else(FileFailure::outcome),
        terminal,
        reservation,
    );
}

fn complete_finish_durable_import(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    owner: DurableImportOwner,
    byte_count: u64,
    digest: [u8; 32],
    result: Result<ContentRef, FileFailure>,
    reservation: QueueReservation,
) {
    let outcome = {
        let mut calls = shared.calls.borrow_mut();
        matching_active(&mut calls, call_id, nonce).and_then(|active| match result {
            Err(error) => {
                if let Ok(slot) = durable_import_slot_mut(&mut active.state, owner) {
                    *slot = DurableImportState::Finished;
                }
                Err(error)
            }
            Ok(content) => match (&mut active.state, owner) {
                (ActiveOperationState::ReadStream(state), DurableImportOwner::ReadStream) => {
                    if !matches!(state.retained, Some(DurableImportState::InFlight))
                        || state.offset != byte_count
                        || content.size() != byte_count
                        || content.digest() != digest
                        || content.media() != state.media
                    {
                        return Err(FileFailure::new(
                            "content_corrupt",
                            "retained stream publication differs from its completed stream",
                        ));
                    }
                    state.retained = Some(DurableImportState::Finished);
                    let content = content
                        .value()
                        .map_err(|error| FileFailure::new("content_invalid", error.to_string()))?;
                    Ok(tagged(
                        "Finished",
                        BTreeMap::from([
                            ("byte_count".to_owned(), number(byte_count)?),
                            ("digest".to_owned(), Value::Bytes(digest.to_vec().into())),
                            (
                                "retained".to_owned(),
                                tagged(
                                    "Retained",
                                    BTreeMap::from([("content".to_owned(), content)]),
                                ),
                            ),
                        ]),
                    ))
                }
                (ActiveOperationState::Import(state), DurableImportOwner::ContentImport) => {
                    if !matches!(state.durable, DurableImportState::InFlight)
                        || state.offset != byte_count
                        || content.size() != byte_count
                        || content.digest() != digest
                        || content.media() != state.media
                    {
                        return Err(FileFailure::new(
                            "content_corrupt",
                            "content publication differs from its completed import",
                        ));
                    }
                    state.durable = DurableImportState::Finished;
                    let content = content
                        .value()
                        .map_err(|error| FileFailure::new("content_invalid", error.to_string()))?;
                    Ok(tagged(
                        "Imported",
                        BTreeMap::from([("content".to_owned(), content)]),
                    ))
                }
                _ => Err(FileFailure::new(
                    "invalid_state",
                    "durable content publication completed for another operation",
                )),
            },
        })
    };
    finish_task_with_reserved_event(
        shared,
        call_id,
        nonce,
        outcome.unwrap_or_else(FileFailure::outcome),
        true,
        reservation,
    );
}

fn complete_resolve_content(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    result: Result<BrowserContentMetadata, FileFailure>,
    reservation: QueueReservation,
) {
    let outcome = result.and_then(|metadata| {
        let mut calls = shared.calls.borrow_mut();
        let active = matching_active(&mut calls, call_id, nonce)?;
        let ActiveOperationState::Save(state) = &mut active.state else {
            return Err(FileFailure::new(
                "invalid_state",
                "durable content metadata resolved for another operation",
            ));
        };
        if state.metadata.is_some() || metadata.content() != &state.content {
            return Err(FileFailure::new(
                "content_corrupt",
                "resolved durable metadata differs from the requested content",
            ));
        }
        if (state.content.size() == 0
            && (metadata.chunk_count() != 0 || metadata.chunk_bytes() != 0))
            || (state.content.size() > 0
                && (metadata.chunk_count() == 0 || metadata.chunk_bytes() == 0))
        {
            return Err(FileFailure::new(
                "content_corrupt",
                "resolved durable metadata has invalid chunk geometry",
            ));
        }
        let byte_count = state.content.size();
        state.metadata = Some(metadata);
        Ok(tagged(
            "Started",
            BTreeMap::from([("byte_count".to_owned(), number(byte_count)?)]),
        ))
    });
    let terminal = outcome.is_err();
    finish_task_with_reserved_event(
        shared,
        call_id,
        nonce,
        outcome.unwrap_or_else(FileFailure::outcome),
        terminal,
        reservation,
    );
}

fn complete_open_writer(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    result: Result<FileSystemWritableFileStream, FileFailure>,
) {
    let mut abort = None;
    let mut timed_out = false;
    {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            if let Ok(writer) = result {
                abort_detached(writer);
            }
            return;
        };
        active.pending_tasks = active.pending_tasks.saturating_sub(1);
        match result {
            Ok(writer) if active.cancelled => {
                active.pending_tasks += 1;
                active.abort_started = true;
                abort = Some(writer);
            }
            Ok(writer) if active.permit.stop_reason() == Some(EffectStopReason::TimedOut) => {
                active.pending_tasks += 1;
                active.abort_started = true;
                timed_out = true;
                abort = Some(writer);
            }
            Ok(writer) => match &mut active.state {
                ActiveOperationState::WriteBytes(state) => {
                    state.writer = Some(writer);
                    state.phase = WritePhase::Ready;
                }
                ActiveOperationState::Save(state) => {
                    state.writer = Some(writer);
                    state.phase = WritePhase::Ready;
                }
                _ => {
                    active.terminal_pending = Some(
                        FileFailure::new(
                            "invalid_state",
                            "browser writable opened for a non-writing operation",
                        )
                        .outcome(),
                    );
                }
            },
            Err(error) if !active.cancelled => {
                active.terminal_pending = Some(error.outcome());
            }
            Err(_) => {}
        }
    }
    if let Some(writer) = abort {
        if timed_out {
            spawn_timeout_abort(shared.clone(), call_id, nonce, writer);
        } else {
            spawn_abort(shared.clone(), call_id, nonce, writer);
        }
    } else {
        cleanup_cancelled_if_idle(shared, call_id);
        pump(shared, call_id);
    }
}

fn complete_write_bytes(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    result: Result<(), FileFailure>,
) {
    {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            return;
        };
        active.pending_tasks = active.pending_tasks.saturating_sub(1);
        if !active.cancelled {
            match result {
                Ok(()) => {
                    let ActiveOperationState::WriteBytes(state) = &mut active.state else {
                        return;
                    };
                    state.phase = WritePhase::Closing;
                }
                Err(error) => active.terminal_pending = Some(error.outcome()),
            }
        }
    }
    cleanup_cancelled_if_idle(shared, call_id);
    pump(shared, call_id);
}

fn complete_save_chunk(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    sequence: u32,
    result: Result<Vec<u8>, FileFailure>,
    reservation: QueueReservation,
) {
    let outcome = result.and_then(|bytes| {
        let mut calls = shared.calls.borrow_mut();
        let active = matching_active(&mut calls, call_id, nonce)?;
        let ActiveOperationState::Save(state) = &mut active.state else {
            return Err(FileFailure::new(
                "invalid_state",
                "content-save promise completed for another operation",
            ));
        };
        if sequence != state.next_chunk || bytes.is_empty() {
            return Err(FileFailure::new(
                "content_corrupt",
                "durable content-save chunk is missing or out of sequence",
            ));
        }
        let byte_count = bytes.len() as u64;
        state.offset = state.offset.checked_add(byte_count).ok_or_else(|| {
            FileFailure::new("file_too_large", "browser content-save offset overflow")
        })?;
        if state.offset > state.content.size() {
            return Err(FileFailure::new(
                "content_corrupt",
                "durable content-save chunk exceeds its declared size",
            ));
        }
        state.digest.update(&bytes);
        state.next_chunk = state.next_chunk.checked_add(1).ok_or_else(|| {
            FileFailure::new(
                "content_corrupt",
                "durable content-save chunk sequence overflowed",
            )
        })?;
        Ok(tagged(
            "Progress",
            BTreeMap::from([
                ("completed_bytes".to_owned(), number(state.offset)?),
                ("total_bytes".to_owned(), number(state.content.size())?),
            ]),
        ))
    });
    let terminal = outcome.is_err();
    finish_task_with_reserved_event(
        shared,
        call_id,
        nonce,
        outcome.unwrap_or_else(FileFailure::outcome),
        terminal,
        reservation,
    );
}

fn complete_close_writer(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    result: Result<(), FileFailure>,
    reservation: QueueReservation,
) {
    let outcome = {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            shared.queue.borrow_mut().release(reservation);
            return;
        };
        let commit_result = match active.commit_guard.take() {
            Some(commit) => {
                commit.finish();
                Ok(())
            }
            None => Err(FileFailure::new(
                "invalid_state",
                "browser writable completed without a commit reservation",
            )),
        };
        match result.and(commit_result) {
            Ok(()) => match &mut active.state {
                ActiveOperationState::WriteBytes(state) => {
                    state.writer = None;
                    state.phase = WritePhase::Finished;
                    tagged(
                        "BytesWritten",
                        BTreeMap::from([(
                            "byte_count".to_owned(),
                            number(state.bytes.len() as u64)
                                .expect("bounded write byte count fits Boon Number"),
                        )]),
                    )
                }
                ActiveOperationState::Save(state) => {
                    state.writer = None;
                    state.phase = WritePhase::Finished;
                    tagged(
                        "Saved",
                        BTreeMap::from([(
                            "byte_count".to_owned(),
                            number(state.content.size())
                                .expect("bounded content byte count fits Boon Number"),
                        )]),
                    )
                }
                _ => FileFailure::new(
                    "invalid_state",
                    "browser writable committed for a non-writing operation",
                )
                .outcome(),
            },
            Err(error) => error.outcome(),
        }
    };
    finish_task_with_reserved_event(shared, call_id, nonce, outcome, true, reservation);
}

fn finish_task_with_reserved_event(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    mut outcome: Value,
    mut terminal: bool,
    reservation: QueueReservation,
) {
    let notification = {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            shared.queue.borrow_mut().release(reservation);
            return;
        };
        active.pending_tasks = active.pending_tasks.saturating_sub(1);
        if active.cancelled {
            shared.queue.borrow_mut().release(reservation);
            drop(calls);
            cleanup_cancelled_if_idle(shared, call_id);
            return;
        }
        if active.permit.stop_reason() == Some(EffectStopReason::TimedOut) {
            outcome = timeout_failure().outcome();
            terminal = true;
        }
        let sequence = if active.operation.is_stream() {
            let sequence = active.next_result_sequence;
            if active.operation == BrowserFileEffectOperation::ReadStream {
                let Some(validator) = active.validator.as_mut() else {
                    shared.queue.borrow_mut().release(reservation);
                    return;
                };
                if validator.accept(sequence, &outcome, terminal).is_err() {
                    shared.queue.borrow_mut().release(reservation);
                    active.terminal_pending = Some(
                        FileFailure::new(
                            "invalid_stream",
                            "browser byte stream violated its canonical sequence contract",
                        )
                        .outcome(),
                    );
                    drop(calls);
                    pump(shared, call_id);
                    return;
                }
            }
            active.next_result_sequence = match sequence.checked_add(1) {
                Some(value) => value,
                None => {
                    shared.queue.borrow_mut().release(reservation);
                    return;
                }
            };
            Some(sequence)
        } else {
            None
        };
        BrowserFileEffectNotification {
            call_id,
            operation: active.operation,
            result_sequence: sequence,
            terminal,
            outcome,
        }
    };
    if shared
        .queue
        .borrow_mut()
        .finish(reservation, notification)
        .is_err()
    {
        discard_call(shared, call_id);
        return;
    }
    (shared.wake)();
    if terminal {
        remove_active(shared, call_id);
    } else {
        pump(shared, call_id);
    }
}

fn emit_unreserved(
    shared: &SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    outcome: Value,
    terminal: bool,
) -> bool {
    let reservation_bytes = RESULT_ENVELOPE_BYTES
        .saturating_add(value_payload_bytes(&outcome))
        .max(SMALL_RESULT_RESERVATION_BYTES);
    let Some(reservation) = shared.queue.borrow_mut().reserve(reservation_bytes) else {
        return false;
    };
    let (notification, delivered_terminal) = {
        let mut outcome = outcome;
        let mut delivered_terminal = terminal;
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            shared.queue.borrow_mut().release(reservation);
            return false;
        };
        let sequence = if active.operation.is_stream() {
            let sequence = active.next_result_sequence;
            if active.operation == BrowserFileEffectOperation::ReadStream {
                let Some(validator) = active.validator.as_mut() else {
                    shared.queue.borrow_mut().release(reservation);
                    return false;
                };
                if validator.accept(sequence, &outcome, terminal).is_err() {
                    outcome = FileFailure::new(
                        "invalid_stream",
                        "browser byte stream violated its canonical sequence contract",
                    )
                    .outcome();
                    delivered_terminal = true;
                    if validator.accept(sequence, &outcome, true).is_err() {
                        shared.queue.borrow_mut().release(reservation);
                        return false;
                    }
                }
            }
            let Some(next) = sequence.checked_add(1) else {
                shared.queue.borrow_mut().release(reservation);
                return false;
            };
            active.next_result_sequence = next;
            Some(sequence)
        } else {
            None
        };
        (
            BrowserFileEffectNotification {
                call_id,
                operation: active.operation,
                result_sequence: sequence,
                terminal: delivered_terminal,
                outcome,
            },
            delivered_terminal,
        )
    };
    if shared
        .queue
        .borrow_mut()
        .finish(reservation, notification)
        .is_err()
    {
        return false;
    }
    (shared.wake)();
    if delivered_terminal {
        remove_active(shared, call_id);
    }
    true
}

fn spawn_abort(
    shared: SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    writer: FileSystemWritableFileStream,
) {
    spawn_local(async move {
        let writable: &WritableStream = writer.unchecked_ref();
        let _ = JsFuture::from(writable.abort()).await;
        {
            let mut calls = shared.calls.borrow_mut();
            if let Some(active) = calls
                .active
                .get_mut(&call_id)
                .filter(|active| active.nonce == nonce)
            {
                active.pending_tasks = active.pending_tasks.saturating_sub(1);
            }
        }
        cleanup_cancelled_if_idle(&shared, call_id);
    });
}

fn abort_detached(writer: FileSystemWritableFileStream) {
    spawn_local(async move {
        let writable: &WritableStream = writer.unchecked_ref();
        let _ = JsFuture::from(writable.abort()).await;
    });
}

fn cleanup_cancelled_if_idle(shared: &SharedDriver, call_id: TransientEffectCallId) {
    let idle = shared
        .calls
        .borrow()
        .active
        .get(&call_id)
        .is_some_and(|active| active.cancelled && active.pending_tasks == 0);
    if idle {
        remove_active(shared, call_id);
    }
}

fn discard_call(shared: &SharedDriver, call_id: TransientEffectCallId) {
    let abort = {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls.active.get_mut(&call_id) else {
            return;
        };
        active.cancelled = true;
        active.permit.discard();
        if active.abort_started {
            None
        } else {
            let package = active.package_abort_handles();
            let writer = active.writer();
            if package.is_some() || writer.is_some() {
                active.abort_started = true;
            }
            if writer.is_some() {
                active.pending_tasks = active.pending_tasks.saturating_add(1);
            }
            (package.is_some() || writer.is_some()).then_some((active.nonce, package, writer))
        }
    };
    if let Some((nonce, package, writer)) = abort {
        if let Some(package) = package {
            package.abort();
        }
        if let Some(writer) = writer {
            spawn_abort(shared.clone(), call_id, nonce, writer);
        } else {
            cleanup_cancelled_if_idle(shared, call_id);
        }
    } else {
        cleanup_cancelled_if_idle(shared, call_id);
    }
}

fn timeout_call(shared: &SharedDriver, call_id: TransientEffectCallId, nonce: u64) {
    let abort = {
        let mut calls = shared.calls.borrow_mut();
        let Some(active) = calls
            .active
            .get_mut(&call_id)
            .filter(|active| active.nonce == nonce)
        else {
            return;
        };
        if active.permit.request_timeout()
            != EffectStopDisposition::Accepted(EffectStopReason::TimedOut)
        {
            return;
        }
        let package = active.package_abort_handles();
        let writer = active.writer();
        if package.is_some() || writer.is_some() {
            active.abort_started = true;
        }
        if writer.is_some() {
            active.pending_tasks = active.pending_tasks.saturating_add(1);
        } else {
            // Pure reads and not-yet-open writers have no irreversible handle.
            // Detach their promise and ignore its late completion.
            active.pending_tasks = 0;
        }
        (package, writer)
    };
    if let Some(package) = abort.0 {
        package.abort();
    }
    if let Some(writer) = abort.1 {
        spawn_timeout_abort(shared.clone(), call_id, nonce, writer);
    }
    pump(shared, call_id);
}

fn timeout_failure() -> FileFailure {
    FileFailure::new(
        "timeout",
        "browser file operation exceeded its host deadline",
    )
}

fn spawn_timeout_abort(
    shared: SharedDriver,
    call_id: TransientEffectCallId,
    nonce: u64,
    writer: FileSystemWritableFileStream,
) {
    spawn_local(async move {
        let writable: &WritableStream = writer.unchecked_ref();
        let _ = JsFuture::from(writable.abort()).await;
        {
            let mut calls = shared.calls.borrow_mut();
            if let Some(active) = calls
                .active
                .get_mut(&call_id)
                .filter(|active| active.nonce == nonce)
            {
                // Abort completion is the writer ownership barrier. Any older
                // write promise is now stale and may finish only into no owner.
                active.pending_tasks = 0;
            }
        }
        pump(&shared, call_id);
    });
}

fn remove_active(shared: &SharedDriver, call_id: TransientEffectCallId) {
    let mut removed = {
        let mut calls = shared.calls.borrow_mut();
        let removed = calls.active.remove(&call_id);
        if calls.writer_busy == Some(call_id) {
            calls.writer_busy = None;
        }
        removed
    };
    if let Some(reservation) = removed
        .as_mut()
        .and_then(ActiveCall::take_package_reservation)
    {
        shared.queue.borrow_mut().release(reservation);
    }
    if let Some(mut active) = removed
        && !active.abort_started
        && let Some(writer) = active.take_writer()
    {
        abort_detached(writer);
    }
}

fn matching_active(
    calls: &mut CallState,
    call_id: TransientEffectCallId,
    nonce: u64,
) -> Result<&mut ActiveCall, FileFailure> {
    calls
        .active
        .get_mut(&call_id)
        .filter(|active| active.nonce == nonce && !active.cancelled)
        .ok_or_else(|| FileFailure::new("cancelled", "browser effect call is no longer current"))
}

async fn read_blob(file: &File) -> Result<Vec<u8>, FileFailure> {
    let blob: &web_sys::Blob = file.unchecked_ref();
    let value = JsFuture::from(blob.array_buffer())
        .await
        .map_err(|error| js_failure("read_failed", "cannot read selected browser file", error))?;
    Ok(Uint8Array::new(&value).to_vec())
}

async fn read_blob_slice(file: &File, start: u64, end: u64) -> Result<Vec<u8>, FileFailure> {
    let blob: &web_sys::Blob = file.unchecked_ref();
    let slice = blob
        .slice_with_f64_and_f64(start as f64, end as f64)
        .map_err(|error| js_failure("read_failed", "cannot slice selected browser file", error))?;
    let value = JsFuture::from(slice.array_buffer())
        .await
        .map_err(|error| js_failure("read_failed", "cannot read browser file slice", error))?;
    Ok(Uint8Array::new(&value).to_vec())
}

fn blob_size(file: &File) -> Result<u64, FileFailure> {
    let blob: &web_sys::Blob = file.unchecked_ref();
    let size = blob.size();
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > MAX_SAFE_INTEGER as f64 {
        return Err(FileFailure::new(
            "file_too_large",
            "browser file size is outside Boon Number's exact integer range",
        ));
    }
    Ok(size as u64)
}

fn file_media(file: &File) -> String {
    let blob: &web_sys::Blob = file.unchecked_ref();
    let media = blob.type_();
    if media.is_empty()
        || media.len() > boon_runtime::MAX_CONTENT_MEDIA_BYTES
        || media.trim() != media
        || media.bytes().any(|byte| byte.is_ascii_control())
    {
        DEFAULT_MEDIA.to_owned()
    } else {
        media
    }
}

fn descriptor_package_id(descriptor: &BrowserPackageAssetDescriptor) -> Option<&str> {
    descriptor
        .url
        .strip_prefix("asset://")?
        .strip_suffix(&descriptor.fetch_path)
        .filter(|package_id| !package_id.is_empty())
}

fn package_asset_display_name(fetch_path: &str) -> String {
    bounded_text(
        fetch_path.rsplit('/').next().unwrap_or_default(),
        MAX_DISPLAY_NAME_BYTES,
    )
}

fn decode_sha256(value: &str) -> Result<[u8; 32], FileFailure> {
    if value.len() != 64 {
        return Err(FileFailure::invalid(
            "package asset SHA-256 digest must contain 64 hexadecimal digits",
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = decode_hex_digit(pair[0]).ok_or_else(|| {
            FileFailure::invalid("package asset SHA-256 digest is not lowercase hexadecimal")
        })?;
        let low = decode_hex_digit(pair[1]).ok_or_else(|| {
            FileFailure::invalid("package asset SHA-256 digest is not lowercase hexadecimal")
        })?;
        digest[index] = (high << 4) | low;
    }
    Ok(digest)
}

fn decode_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn rechunk_buffer_limit(output_chunk_bytes: usize) -> Result<usize, FileFailure> {
    output_chunk_bytes
        .checked_add(MAX_PACKAGE_TRANSPORT_CHUNK_BYTES.saturating_sub(1))
        .ok_or_else(|| {
            FileFailure::new(
                "invalid_state",
                "package asset re-chunk buffer limit overflowed",
            )
        })
}

fn validate_invocation(
    operation: BrowserFileEffectOperation,
    invocation: &TransientEffectInvocation,
) -> WebHostResult<()> {
    let effect_id = EffectId::from_host_operation(operation.host_operation()).map_err(|error| {
        WebHostError::InvalidInput {
            field: "browser File/Content effect".to_owned(),
            reason: error.to_string(),
        }
    })?;
    let contract = builtin_effect_contract(operation.host_operation())
        .map_err(|error| WebHostError::InvalidInput {
            field: "browser File/Content effect".to_owned(),
            reason: error.to_string(),
        })?
        .ok_or_else(|| WebHostError::InvalidInput {
            field: "browser File/Content effect".to_owned(),
            reason: "canonical effect contract is absent".to_owned(),
        })?;
    if invocation.effect_id != effect_id || invocation.delivery != contract.delivery {
        return Err(WebHostError::InvalidInput {
            field: "browser File/Content effect".to_owned(),
            reason: "invocation differs from its canonical operation or delivery contract"
                .to_owned(),
        });
    }
    Ok(())
}

fn exact_record<'a>(
    value: &'a Value,
    expected: &[&str],
    context: &str,
) -> Result<&'a BTreeMap<String, Value>, FileFailure> {
    let Value::Record(fields) = value else {
        return Err(FileFailure::invalid(format!("{context} must be a record")));
    };
    let actual = fields.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(FileFailure::invalid(format!(
            "{context} fields differ from the canonical typed contract"
        )));
    }
    Ok(fields)
}

fn required_field<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
) -> Result<&'a Value, FileFailure> {
    fields
        .get(name)
        .ok_or_else(|| FileFailure::invalid(format!("effect intent is missing `{name}`")))
}

fn positive_usize(fields: &BTreeMap<String, Value>, name: &str) -> Result<usize, FileFailure> {
    let Value::Number(value) = required_field(fields, name)? else {
        return Err(FileFailure::invalid(format!("`{name}` must be Number")));
    };
    value
        .to_i64_exact()
        .ok()
        .filter(|value| *value > 0)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| FileFailure::invalid(format!("`{name}` must be a positive whole Number")))
}

fn validate_bound_tag(value: &Value, expected_tag: &str) -> Result<(), FileFailure> {
    let fields = exact_record(value.visible(), &["$tag"], "browser file capability")?;
    match fields.get("$tag") {
        Some(Value::Text(tag)) if tag == expected_tag => Ok(()),
        _ => Err(FileFailure::invalid(format!(
            "browser file capability must be {expected_tag}"
        ))),
    }
}

fn opened_outcome(size: u64, media: &str, display_name: &str) -> Result<Value, FileFailure> {
    Ok(tagged(
        "Opened",
        BTreeMap::from([
            ("size".to_owned(), number(size)?),
            ("content_type".to_owned(), Value::Text(media.to_owned())),
            (
                "display_name".to_owned(),
                Value::Text(display_name.to_owned()),
            ),
        ]),
    ))
}

fn started_import_outcome(
    size: u64,
    media: &str,
    display_name: &str,
) -> Result<Value, FileFailure> {
    Ok(tagged(
        "Started",
        BTreeMap::from([
            ("byte_count".to_owned(), number(size)?),
            ("media".to_owned(), Value::Text(media.to_owned())),
            (
                "display_name".to_owned(),
                Value::Text(display_name.to_owned()),
            ),
        ]),
    ))
}

fn tagged(tag: &str, mut fields: BTreeMap<String, Value>) -> Value {
    fields.insert("$tag".to_owned(), Value::Text(tag.to_owned()));
    Value::Record(fields)
}

fn number(value: u64) -> Result<Value, FileFailure> {
    let value = i64::try_from(value)
        .map_err(|_| FileFailure::new("file_too_large", "byte count exceeds Boon Number range"))?;
    Value::integer(value)
        .map_err(|_| FileFailure::new("file_too_large", "byte count is not exactly representable"))
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn notification_weight(notification: &BrowserFileEffectNotification) -> usize {
    RESULT_ENVELOPE_BYTES.saturating_add(value_payload_bytes(&notification.outcome))
}

fn value_payload_bytes(value: &Value) -> usize {
    match value {
        Value::Text(value) => value.len(),
        Value::Bytes(value) => value.len(),
        Value::List(values) => values.iter().map(value_payload_bytes).sum(),
        Value::Record(fields) | Value::MappedRow { fields, .. } => fields
            .iter()
            .map(|(name, value)| name.len().saturating_add(value_payload_bytes(value)))
            .sum(),
        Value::Row { fields, .. } => fields.values().map(value_payload_bytes).sum(),
        Value::HostBound { visible, .. } => value_payload_bytes(visible),
        Value::Error { code } => code.len(),
        Value::Null | Value::Bool(_) | Value::Number(_) => 16,
    }
}

#[derive(Clone, Debug)]
struct FileFailure {
    code: String,
    diagnostic: String,
}

impl FileFailure {
    fn new(code: impl Into<String>, diagnostic: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            diagnostic: bounded_text(&diagnostic.into(), 512),
        }
    }

    fn invalid(diagnostic: impl Into<String>) -> Self {
        Self::new("invalid_intent", diagnostic)
    }

    fn outcome(self) -> Value {
        tagged(
            "Failed",
            BTreeMap::from([
                ("code".to_owned(), Value::Text(self.code)),
                ("diagnostic".to_owned(), Value::Text(self.diagnostic)),
            ]),
        )
    }
}

fn js_failure(code: &str, context: &str, error: wasm_bindgen::JsValue) -> FileFailure {
    FileFailure::new(code, format!("{context}: {}", super::js_message(&error)))
}

fn content_store_failure(error: BrowserContentStoreError) -> FileFailure {
    let code = match &error {
        BrowserContentStoreError::InvalidConfiguration { .. } => "content_store_invalid",
        BrowserContentStoreError::InvalidInput { .. } => "content_invalid",
        BrowserContentStoreError::LimitExceeded { .. }
        | BrowserContentStoreError::QuotaExceeded { .. } => "content_store_full",
        BrowserContentStoreError::Aborted { .. } => "content_store_aborted",
        BrowserContentStoreError::VersionMismatch { .. }
        | BrowserContentStoreError::SchemaMismatch { .. } => "content_store_incompatible",
        BrowserContentStoreError::Missing { .. } => "content_missing",
        BrowserContentStoreError::Corrupt { .. }
        | BrowserContentStoreError::DigestMismatch { .. }
        | BrowserContentStoreError::SizeMismatch { .. }
        | BrowserContentStoreError::SequenceMismatch { .. } => "content_corrupt",
        BrowserContentStoreError::Platform { .. } => "content_store_failed",
    };
    FileFailure::new(code, error.to_string())
}

fn content_store_open_error(error: BrowserContentStoreError) -> WebHostError {
    match error {
        BrowserContentStoreError::InvalidConfiguration { reason }
        | BrowserContentStoreError::SchemaMismatch { reason } => WebHostError::InvalidInput {
            field: "browser content store".to_owned(),
            reason,
        },
        BrowserContentStoreError::InvalidInput { field, reason } => WebHostError::InvalidInput {
            field: format!("browser content store {field}"),
            reason,
        },
        BrowserContentStoreError::LimitExceeded { resource, limit } => {
            WebHostError::LimitExceeded {
                resource: format!("browser {resource}"),
                limit: usize::try_from(limit).unwrap_or(usize::MAX),
            }
        }
        error => WebHostError::Platform {
            operation: "open browser content store".to_owned(),
            message: error.to_string(),
        },
    }
}

fn capability_lookup_failure(error: boon_runtime::HostCapabilityError) -> FileFailure {
    let code = match error.kind() {
        HostCapabilityErrorKind::Stale => "stale_capability",
        HostCapabilityErrorKind::WrongAccess => "wrong_access",
        HostCapabilityErrorKind::Foreign | HostCapabilityErrorKind::Unknown => "unknown_capability",
        HostCapabilityErrorKind::InvalidConfiguration
        | HostCapabilityErrorKind::Capacity
        | HostCapabilityErrorKind::DuplicateHandle
        | HostCapabilityErrorKind::GenerationExhausted => "capability_error",
    };
    FileFailure::new(code, error.to_string())
}

fn capability_configuration_error(error: boon_runtime::HostCapabilityError) -> WebHostError {
    WebHostError::InvalidInput {
        field: "browser file capability registry".to_owned(),
        reason: error.to_string(),
    }
}

fn capability_error(error: boon_runtime::HostCapabilityError) -> WebHostError {
    match error.kind() {
        HostCapabilityErrorKind::Capacity => WebHostError::LimitExceeded {
            resource: "browser file capabilities".to_owned(),
            limit: usize::MAX,
        },
        _ => WebHostError::InvalidInput {
            field: "browser file capability".to_owned(),
            reason: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::{EffectInvocationId, OwnerInstanceId};
    use idb::Factory;
    use wasm_bindgen_test::wasm_bindgen_test;

    fn test_call_id(sequence: u64) -> TransientEffectCallId {
        TransientEffectCallId::from_host_parts(31, sequence)
    }

    fn test_invocation(
        operation: BrowserFileEffectOperation,
        sequence: u64,
        intent: Value,
    ) -> TransientEffectInvocation {
        let effect_id = EffectId::from_host_operation(operation.host_operation()).unwrap();
        let delivery = builtin_effect_contract(operation.host_operation())
            .unwrap()
            .unwrap()
            .delivery;
        TransientEffectInvocation {
            call_id: test_call_id(sequence),
            invocation_id: EffectInvocationId::from_result_owner(
                effect_id,
                &format!("browser-test-result-{sequence}"),
            )
            .unwrap(),
            effect_id,
            trigger_sequence: sequence,
            authority_turn_sequence: sequence,
            owner: OwnerInstanceId::root(),
            target: None,
            intent,
            delivery,
        }
    }

    fn test_file(bytes: &[u8]) -> File {
        let parts = js_sys::Array::new();
        let bytes = Uint8Array::from(bytes);
        parts.push(bytes.as_ref());
        File::new_with_u8_array_sequence(parts.as_ref(), "stream.bin").unwrap()
    }

    struct TestWritableTarget {
        handle: FileSystemFileHandle,
        writes: Rc<RefCell<Vec<Vec<u8>>>>,
        _write_callback: Closure<dyn FnMut(JsValue) -> js_sys::Promise>,
        _create_callback: Closure<dyn FnMut() -> js_sys::Promise>,
    }

    fn test_writable_target() -> TestWritableTarget {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let writes_for_callback = Rc::clone(&writes);
        let write_callback = Closure::wrap(Box::new(move |chunk: JsValue| {
            writes_for_callback
                .borrow_mut()
                .push(Uint8Array::new(&chunk).to_vec());
            js_sys::Promise::resolve(&JsValue::UNDEFINED)
        })
            as Box<dyn FnMut(JsValue) -> js_sys::Promise>);
        let writer = WritableStream::new().unwrap();
        Reflect::set(
            writer.as_ref(),
            &JsValue::from_str("write"),
            write_callback.as_ref(),
        )
        .unwrap();

        let writer_for_create = writer.clone();
        let create_callback = Closure::wrap(Box::new(move || {
            let writer: &JsValue = writer_for_create.as_ref();
            js_sys::Promise::resolve(writer)
        }) as Box<dyn FnMut() -> js_sys::Promise>);
        let handle = js_sys::Object::new();
        Reflect::set(
            handle.as_ref(),
            &JsValue::from_str("createWritable"),
            create_callback.as_ref(),
        )
        .unwrap();
        TestWritableTarget {
            handle: handle.unchecked_into(),
            writes,
            _write_callback: write_callback,
            _create_callback: create_callback,
        }
    }

    async fn browser_tick() {
        let promise = js_sys::Promise::new(&mut |resolve, _reject| {
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0)
                .unwrap();
        });
        JsFuture::from(promise).await.unwrap();
    }

    async fn next_notification(host: &mut BrowserFileEffectHost) -> BrowserFileEffectNotification {
        for _ in 0..200 {
            if let Some(notification) = host.dequeue_notification() {
                return notification;
            }
            browser_tick().await;
        }
        panic!("browser File/Content effect produced no bounded result");
    }

    fn outcome_tag(value: &Value) -> &str {
        let Value::Record(fields) = value else {
            panic!("effect outcome must be a tagged record");
        };
        let Some(Value::Text(tag)) = fields.get("$tag") else {
            panic!("effect outcome must contain a Text tag");
        };
        tag
    }

    #[wasm_bindgen_test]
    fn bounded_queue_accounts_payload_bytes_and_removal() {
        let call_id = TransientEffectCallId::from_host_parts(1, 1);
        let mut queue = BoundedNotificationQueue::new(2, 4 * 1024);
        queue
            .push(BrowserFileEffectNotification {
                call_id,
                operation: BrowserFileEffectOperation::ReadBytes,
                result_sequence: None,
                terminal: true,
                outcome: tagged(
                    "BytesRead",
                    BTreeMap::from([("bytes".to_owned(), Value::Bytes(vec![1; 32].into()))]),
                ),
            })
            .unwrap();
        assert_eq!(queue.entries.len(), 1);
        assert!(queue.used_bytes >= 32);
        assert!(queue.remove_call(call_id));
        assert_eq!(queue.used_bytes, 0);

        let mut invalid_limits = BrowserFileEffectLimits::default();
        invalid_limits.operation_timeout_ms = 0;
        assert!(invalid_limits.validate().is_err());
    }

    #[wasm_bindgen_test(async)]
    async fn credit_starved_stream_times_out_terminally_and_releases_ownership() {
        let package_id = format!("file-timeout-test-{}", js_sys::Date::now() as u64);
        let payload = vec![0x4d; FILE_STREAM_DEFAULT_CHUNK_BYTES as usize * 5 + 1];
        let mut limits = BrowserFileEffectLimits::default();
        limits.operation_timeout_ms = 250;
        let mut host = BrowserFileEffectHost::open(&package_id, Rc::new(|| {}), limits)
            .await
            .unwrap();
        let selected = host.register_source(test_file(&payload)).unwrap();
        let stream = test_invocation(
            BrowserFileEffectOperation::ReadStream,
            41,
            Value::Record(BTreeMap::from([
                (
                    "chunk_bytes".to_owned(),
                    Value::integer(FILE_STREAM_DEFAULT_CHUNK_BYTES as i64).unwrap(),
                ),
                ("file".to_owned(), selected),
                ("retain_content".to_owned(), Value::Bool(false)),
            ])),
        );
        let call_id = stream.call_id;
        host.submit(BrowserFileEffectOperation::ReadStream, stream)
            .unwrap();

        let opened = next_notification(&mut host).await;
        assert_eq!(opened.result_sequence, Some(0));
        assert_eq!(outcome_tag(&opened.outcome), "Opened");
        for sequence in 1..=4 {
            let chunk = next_notification(&mut host).await;
            assert_eq!(chunk.result_sequence, Some(sequence));
            assert_eq!(outcome_tag(&chunk.outcome), "Chunk");
        }
        assert_eq!(host.active_count(), 1);
        assert_eq!(host.deadlines.len(), 1);

        let timed_out = next_notification(&mut host).await;
        assert_eq!(timed_out.call_id, call_id);
        assert_eq!(timed_out.result_sequence, Some(5));
        assert!(timed_out.terminal);
        assert_eq!(outcome_tag(&timed_out.outcome), "Failed");
        let Value::Record(fields) = timed_out.outcome else {
            unreachable!();
        };
        assert_eq!(fields["code"], Value::Text("timeout".to_owned()));
        assert_eq!(host.active_count(), 0);
        assert_eq!(host.queued_count(), 0);
        assert!(host.deadlines.is_empty());

        let database_name = host.shared.content.database_name().to_owned();
        drop(host);
        Factory::new()
            .unwrap()
            .delete(&database_name)
            .unwrap()
            .await
            .unwrap();
    }

    #[wasm_bindgen_test(async)]
    async fn bounded_read_bytes_uses_a_host_bound_user_file() {
        let package_id = format!("read-bytes-test-{}", js_sys::Date::now() as u64);
        let payload = b"bounded browser bytes";
        let mut host = BrowserFileEffectHost::open(
            &package_id,
            Rc::new(|| {}),
            BrowserFileEffectLimits::default(),
        )
        .await
        .unwrap();
        let selected = host.register_source(test_file(payload)).unwrap();
        let read = test_invocation(
            BrowserFileEffectOperation::ReadBytes,
            45,
            Value::Record(BTreeMap::from([
                ("file".to_owned(), selected),
                (
                    "max_bytes".to_owned(),
                    Value::integer(payload.len() as i64).unwrap(),
                ),
            ])),
        );
        host.submit(BrowserFileEffectOperation::ReadBytes, read)
            .unwrap();
        let completion = next_notification(&mut host).await;
        assert!(completion.terminal);
        assert_eq!(completion.result_sequence, None);
        assert_eq!(outcome_tag(&completion.outcome), "BytesRead");
        let Value::Record(fields) = completion.outcome else {
            unreachable!();
        };
        let Value::Bytes(bytes) = &fields["bytes"] else {
            panic!("BytesRead must contain Bytes");
        };
        assert_eq!(bytes.as_ref(), payload);
        assert_eq!(host.active_count(), 0);

        let database_name = host.shared.content.database_name().to_owned();
        drop(host);
        Factory::new()
            .unwrap()
            .delete(&database_name)
            .unwrap()
            .await
            .unwrap();
    }

    #[wasm_bindgen_test(async)]
    async fn write_bytes_and_content_save_commit_through_browser_writables() {
        let package_id = format!("write-save-test-{}", js_sys::Date::now() as u64);
        let mut host = BrowserFileEffectHost::open(
            &package_id,
            Rc::new(|| {}),
            BrowserFileEffectLimits::default(),
        )
        .await
        .unwrap();

        let write_payload = b"atomic browser write";
        let write_target = test_writable_target();
        let bound_target = host.register_target(write_target.handle.clone()).unwrap();
        let write = test_invocation(
            BrowserFileEffectOperation::WriteBytes,
            46,
            Value::Record(BTreeMap::from([
                (
                    "bytes".to_owned(),
                    Value::Bytes(write_payload.as_slice().into()),
                ),
                ("file".to_owned(), bound_target),
            ])),
        );
        host.submit(BrowserFileEffectOperation::WriteBytes, write)
            .unwrap();
        let written = next_notification(&mut host).await;
        assert!(written.terminal);
        assert_eq!(
            outcome_tag(&written.outcome),
            "BytesWritten",
            "unexpected browser write outcome: {:?}",
            written.outcome
        );
        assert_eq!(
            write_target.writes.borrow().as_slice(),
            &[write_payload.to_vec()]
        );

        let content_payload = b"durable browser content";
        let selected = host.register_source(test_file(content_payload)).unwrap();
        let import = test_invocation(
            BrowserFileEffectOperation::ContentImport,
            47,
            Value::Record(BTreeMap::from([("file".to_owned(), selected)])),
        );
        host.submit(BrowserFileEffectOperation::ContentImport, import)
            .unwrap();
        assert_eq!(
            outcome_tag(&next_notification(&mut host).await.outcome),
            "Started"
        );
        assert_eq!(
            outcome_tag(&next_notification(&mut host).await.outcome),
            "Progress"
        );
        let imported = next_notification(&mut host).await;
        assert!(imported.terminal);
        assert_eq!(outcome_tag(&imported.outcome), "Imported");
        let Value::Record(imported_fields) = imported.outcome else {
            unreachable!();
        };
        let content = imported_fields["content"].clone();

        let save_target = test_writable_target();
        let bound_target = host.register_target(save_target.handle.clone()).unwrap();
        let save = test_invocation(
            BrowserFileEffectOperation::ContentSave,
            48,
            Value::Record(BTreeMap::from([
                ("content".to_owned(), content),
                ("file".to_owned(), bound_target),
            ])),
        );
        host.submit(BrowserFileEffectOperation::ContentSave, save)
            .unwrap();
        assert_eq!(
            outcome_tag(&next_notification(&mut host).await.outcome),
            "Started"
        );
        assert_eq!(
            outcome_tag(&next_notification(&mut host).await.outcome),
            "Progress"
        );
        let saved = next_notification(&mut host).await;
        assert!(saved.terminal);
        assert_eq!(outcome_tag(&saved.outcome), "Saved");
        assert_eq!(
            save_target.writes.borrow().as_slice(),
            &[content_payload.to_vec()]
        );
        assert!(host.shared.calls.borrow().writer_busy.is_none());

        let database_name = host.shared.content.database_name().to_owned();
        drop(host);
        Factory::new()
            .unwrap()
            .delete(&database_name)
            .unwrap()
            .await
            .unwrap();
    }

    #[wasm_bindgen_test(async)]
    async fn writer_timeout_keeps_busy_ownership_until_abort_settles() {
        let package_id = format!("writer-timeout-test-{}", js_sys::Date::now() as u64);
        let mut host = BrowserFileEffectHost::open(
            &package_id,
            Rc::new(|| {}),
            BrowserFileEffectLimits::default(),
        )
        .await
        .unwrap();
        let call_id = test_call_id(51);
        let nonce = 7;
        let writer = WritableStream::new()
            .unwrap()
            .unchecked_into::<FileSystemWritableFileStream>();
        host.shared.calls.borrow_mut().active.insert(
            call_id,
            ActiveCall {
                operation: BrowserFileEffectOperation::WriteBytes,
                nonce,
                permit: EffectCommitPermit::new(),
                commit_guard: None,
                next_result_sequence: 0,
                credits: 0,
                max_in_flight: 0,
                pending_tasks: 0,
                cancelled: false,
                abort_started: false,
                validator: None,
                terminal_pending: None,
                state: ActiveOperationState::WriteBytes(WriteBytesState {
                    target: None,
                    bytes: Rc::from(&b"pending"[..]),
                    writer: Some(writer),
                    phase: WritePhase::Ready,
                }),
            },
        );
        host.shared.calls.borrow_mut().writer_busy = Some(call_id);

        timeout_call(&host.shared, call_id, nonce);
        {
            let calls = host.shared.calls.borrow();
            let active = &calls.active[&call_id];
            assert_eq!(calls.writer_busy, Some(call_id));
            assert_eq!(active.pending_tasks, 1);
            assert!(active.abort_started);
        }

        let replacement = test_invocation(BrowserFileEffectOperation::WriteBytes, 52, Value::Null);
        host.submit(BrowserFileEffectOperation::WriteBytes, replacement)
            .unwrap();
        let busy = next_notification(&mut host).await;
        assert!(busy.terminal);
        assert_eq!(outcome_tag(&busy.outcome), "Busy");
        assert_eq!(host.shared.calls.borrow().writer_busy, Some(call_id));

        let timed_out = next_notification(&mut host).await;
        assert_eq!(timed_out.call_id, call_id);
        assert!(timed_out.terminal);
        assert_eq!(outcome_tag(&timed_out.outcome), "Failed");
        let Value::Record(fields) = timed_out.outcome else {
            unreachable!();
        };
        assert_eq!(fields["code"], Value::Text("timeout".to_owned()));
        assert!(host.shared.calls.borrow().writer_busy.is_none());
        assert_eq!(host.active_count(), 0);

        let database_name = host.shared.content.database_name().to_owned();
        drop(host);
        Factory::new()
            .unwrap()
            .delete(&database_name)
            .unwrap()
            .await
            .unwrap();
    }

    #[wasm_bindgen_test(async)]
    async fn content_import_and_retained_stream_share_one_bounded_durable_path() {
        let package_id = format!("file-content-test-{}", js_sys::Date::now() as u64);
        let payload = (0..(FILE_STREAM_DEFAULT_CHUNK_BYTES as usize * 4 + 17))
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let expected_digest = <[u8; 32]>::from(Sha256::digest(&payload));
        let mut host = BrowserFileEffectHost::open(
            &package_id,
            Rc::new(|| {}),
            BrowserFileEffectLimits::default(),
        )
        .await
        .unwrap();
        let selected = host.register_source(test_file(&payload)).unwrap();

        let import = test_invocation(
            BrowserFileEffectOperation::ContentImport,
            1,
            Value::Record(BTreeMap::from([("file".to_owned(), selected.clone())])),
        );
        let import_call = import.call_id;
        host.submit(BrowserFileEffectOperation::ContentImport, import)
            .unwrap();
        let started = next_notification(&mut host).await;
        assert_eq!(started.result_sequence, Some(0));
        assert_eq!(outcome_tag(&started.outcome), "Started");

        for sequence in 1..=4 {
            let progress = next_notification(&mut host).await;
            assert_eq!(progress.result_sequence, Some(sequence));
            assert_eq!(outcome_tag(&progress.outcome), "Progress");
        }
        for _ in 0..4 {
            browser_tick().await;
        }
        assert_eq!(host.queued_count(), 0);
        assert_eq!(host.active_count(), 1);
        host.grant_credits(TransientEffectCreditGrant {
            call_id: import_call,
            credits: 1,
        })
        .unwrap();
        let final_progress = next_notification(&mut host).await;
        assert_eq!(final_progress.result_sequence, Some(5));
        assert_eq!(outcome_tag(&final_progress.outcome), "Progress");
        let imported = next_notification(&mut host).await;
        assert_eq!(imported.result_sequence, Some(6));
        assert!(imported.terminal);
        assert_eq!(outcome_tag(&imported.outcome), "Imported");
        let Value::Record(imported_fields) = &imported.outcome else {
            unreachable!();
        };
        let content = ContentRef::from_value(&imported_fields["content"]).unwrap();
        assert_eq!(content.size(), payload.len() as u64);
        assert_eq!(content.digest(), expected_digest);
        assert_eq!(
            host.shared
                .content
                .read_range(&content, payload.len() as u64 - 17, 17)
                .await
                .unwrap(),
            payload[payload.len() - 17..]
        );

        let stream = test_invocation(
            BrowserFileEffectOperation::ReadStream,
            2,
            Value::Record(BTreeMap::from([
                (
                    "chunk_bytes".to_owned(),
                    Value::integer(FILE_STREAM_DEFAULT_CHUNK_BYTES as i64).unwrap(),
                ),
                ("file".to_owned(), selected),
                ("retain_content".to_owned(), Value::Bool(true)),
            ])),
        );
        let stream_call = stream.call_id;
        host.submit(BrowserFileEffectOperation::ReadStream, stream)
            .unwrap();
        let opened = next_notification(&mut host).await;
        assert_eq!(opened.result_sequence, Some(0));
        assert_eq!(outcome_tag(&opened.outcome), "Opened");
        for sequence in 1..=4 {
            let chunk = next_notification(&mut host).await;
            assert_eq!(chunk.result_sequence, Some(sequence));
            assert_eq!(outcome_tag(&chunk.outcome), "Chunk");
        }
        host.grant_credits(TransientEffectCreditGrant {
            call_id: stream_call,
            credits: 1,
        })
        .unwrap();
        let final_chunk = next_notification(&mut host).await;
        assert_eq!(final_chunk.result_sequence, Some(5));
        assert_eq!(outcome_tag(&final_chunk.outcome), "Chunk");
        let finished = next_notification(&mut host).await;
        assert_eq!(finished.result_sequence, Some(6));
        assert!(finished.terminal);
        assert_eq!(outcome_tag(&finished.outcome), "Finished");
        let Value::Record(finished_fields) = &finished.outcome else {
            unreachable!();
        };
        let Value::Record(retained_fields) = &finished_fields["retained"] else {
            panic!("retained result must be a tagged record");
        };
        assert_eq!(retained_fields["$tag"], Value::Text("Retained".to_owned()));
        assert_eq!(
            ContentRef::from_value(&retained_fields["content"]).unwrap(),
            content
        );
        assert_eq!(host.active_count(), 0);

        let database_name = host.shared.content.database_name().to_owned();
        drop(host);
        Factory::new()
            .unwrap()
            .delete(&database_name)
            .unwrap()
            .await
            .unwrap();
    }

    #[wasm_bindgen_test(async)]
    async fn cancelling_an_in_flight_import_releases_its_durable_staging_owner() {
        let package_id = format!("file-cancel-test-{}", js_sys::Date::now() as u64);
        let payload = vec![0x5a; FILE_STREAM_DEFAULT_CHUNK_BYTES as usize * 2 + 1];
        let mut limits = BrowserFileEffectLimits::default();
        limits.max_active = 1;
        let mut host = BrowserFileEffectHost::open(&package_id, Rc::new(|| {}), limits)
            .await
            .unwrap();
        let selected = host.register_source(test_file(&payload)).unwrap();
        let import = test_invocation(
            BrowserFileEffectOperation::ContentImport,
            21,
            Value::Record(BTreeMap::from([("file".to_owned(), selected)])),
        );
        let call_id = import.call_id;
        host.submit(BrowserFileEffectOperation::ContentImport, import)
            .unwrap();
        assert_eq!(
            outcome_tag(&next_notification(&mut host).await.outcome),
            "Started"
        );
        assert_eq!(
            outcome_tag(&next_notification(&mut host).await.outcome),
            "Progress"
        );
        assert!(host.cancel(call_id));

        for _ in 0..200 {
            if host.active_count() == 0 {
                break;
            }
            browser_tick().await;
        }
        assert_eq!(host.active_count(), 0);
        assert_eq!(host.queued_count(), 0);

        let mut replacement = None;
        for _ in 0..200 {
            match host.shared.content.begin_import(1, DEFAULT_MEDIA).await {
                Ok(import) => {
                    replacement = Some(import);
                    break;
                }
                Err(BrowserContentStoreError::LimitExceeded {
                    resource: "active content staging imports",
                    ..
                }) => browser_tick().await,
                Err(error) => panic!("unexpected content cleanup failure: {error}"),
            }
        }
        assert!(
            replacement
                .expect("cancelled import must release durable staging")
                .abort()
                .await
                .unwrap()
        );

        let database_name = host.shared.content.database_name().to_owned();
        drop(host);
        Factory::new()
            .unwrap()
            .delete(&database_name)
            .unwrap()
            .await
            .unwrap();
    }

    #[wasm_bindgen_test(async)]
    async fn package_asset_resolution_rejects_urls_outside_the_exact_allowlist() {
        let package_id = format!("file-effect-test-{}", js_sys::Date::now() as u64);
        let descriptor = BrowserPackageAssetDescriptor {
            url: format!("asset://{package_id}/files/example.bin"),
            fetch_path: "/files/example.bin".to_owned(),
            bytes_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                .to_owned(),
            bytes_len: 3,
            media_type: DEFAULT_MEDIA.to_owned(),
        };
        let mut host = BrowserFileEffectHost::open(
            &package_id,
            Rc::new(|| {}),
            BrowserFileEffectLimits::default(),
        )
        .await
        .unwrap();
        host.register_package_assets(std::slice::from_ref(&descriptor))
            .unwrap();

        let allowed = tagged(
            "PackageAsset",
            BTreeMap::from([("url".to_owned(), Value::Text(descriptor.url.clone()))]),
        );
        assert!(matches!(
            host.resolve_source(&allowed).unwrap(),
            ResolvedFileSource::Package(_)
        ));

        let rejected = tagged(
            "PackageAsset",
            BTreeMap::from([(
                "url".to_owned(),
                Value::Text(format!("asset://{package_id}/files/not-listed.bin")),
            )]),
        );
        let failure = match host.resolve_source(&rejected) {
            Err(failure) => failure,
            Ok(_) => panic!("unlisted package asset unexpectedly resolved"),
        };
        assert_eq!(failure.code, "unknown_package_asset");
    }

    #[wasm_bindgen_test]
    fn package_stream_rechunks_with_bounded_buffer_and_verifies_digest() {
        let bytes = b"abcdefghij";
        let digest = <[u8; 32]>::from(Sha256::digest(bytes));
        let mut stream = PackageStreamState::new(10, digest, 4, 8).unwrap();

        stream.push_transport_chunk(b"abc").unwrap();
        assert!(!stream.has_output());
        stream.push_transport_chunk(b"defg").unwrap();
        assert_eq!(stream.take_output().unwrap(), b"abcd");
        stream.push_transport_chunk(b"hij").unwrap();
        assert_eq!(stream.take_output().unwrap(), b"efgh");
        stream.finish_transport().unwrap();
        assert_eq!(stream.take_output().unwrap(), b"ij");
        assert!(stream.is_complete());

        let mut wrong_digest = PackageStreamState::new(3, [0; 32], 2, 4).unwrap();
        wrong_digest.push_transport_chunk(b"abc").unwrap();
        assert_eq!(
            wrong_digest.finish_transport().unwrap_err().code,
            "package_asset_digest_mismatch"
        );

        let mut oversized = PackageStreamState::new(
            MAX_PACKAGE_TRANSPORT_CHUNK_BYTES + 1,
            [0; 32],
            4,
            MAX_PACKAGE_TRANSPORT_CHUNK_BYTES + 4,
        )
        .unwrap();
        assert_eq!(
            oversized
                .push_transport_chunk(&vec![0; MAX_PACKAGE_TRANSPORT_CHUNK_BYTES + 1])
                .unwrap_err()
                .code,
            "transport_chunk_too_large"
        );
    }
}
