use async_trait::async_trait;
use boon_app_package::{BundleFileDescriptor, BundleFileKind, BundleManifest, LoadedAppBundle};
use boon_effect_schema::{
    CONTENT_IMPORT_OPERATION, CONTENT_SAVE_OPERATION, FILE_READ_BYTES_OPERATION,
    FILE_READ_STREAM_OPERATION, FILE_WRITE_BYTES_OPERATION, HMAC_SHA256_SIGN_OPERATION,
    HMAC_SHA256_VERIFY_OPERATION, OUTBOUND_HTTP_REQUEST_OPERATION, SECRET_VERIFY_OPERATION,
    SECURE_RANDOM_BYTES_OPERATION, TIMER_DEADLINE_OPERATION, WALL_CLOCK_READ_OPERATION,
    WELLEN_CURSOR_VALUES_OPERATION, WELLEN_HIERARCHY_PAGE_OPERATION, WELLEN_OPEN_OPERATION,
    WELLEN_SIGNAL_PAGE_OPERATION,
};
use boon_host_runtime::{FileEffectAdapter, HostServiceEffectAdapter};
use boon_http_runtime::OutboundHttpEffectAdapter;
use boon_plan::{EffectId, ProgramRole};
use boon_program_runtime::ProgramArtifact;
use boon_runtime::ExactCallHostCore;
use boon_runtime::{TransientEffectCallId, TransientEffectCreditGrant, TransientEffectInvocation};
use boon_server_runtime::{
    TransientEffectHost, TransientEffectHostDelivery, TransientEffectHostError,
    TransientEffectHostEvent,
};
use boon_wellen_host::WaveformEffectWorker;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_PRODUCTION_ACTIVE_EFFECTS: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostLane {
    File,
    Http,
    Services,
    Waveform,
}

#[derive(Clone, Copy)]
struct HostOperationPolicy {
    operation: &'static str,
    grant: &'static str,
    lane: HostLane,
}

impl HostOperationPolicy {
    const fn new(operation: &'static str, grant: &'static str, lane: HostLane) -> Self {
        Self {
            operation,
            grant,
            lane,
        }
    }
}

const HOST_OPERATION_POLICIES: &[HostOperationPolicy] = &[
    HostOperationPolicy::new(
        OUTBOUND_HTTP_REQUEST_OPERATION,
        "server.outbound-https",
        HostLane::Http,
    ),
    HostOperationPolicy::new(WALL_CLOCK_READ_OPERATION, "host.clock", HostLane::Services),
    HostOperationPolicy::new(TIMER_DEADLINE_OPERATION, "host.timers", HostLane::Services),
    HostOperationPolicy::new(
        SECURE_RANDOM_BYTES_OPERATION,
        "host.secure-random",
        HostLane::Services,
    ),
    HostOperationPolicy::new(
        SECRET_VERIFY_OPERATION,
        "host.secret-store",
        HostLane::Services,
    ),
    HostOperationPolicy::new(
        HMAC_SHA256_SIGN_OPERATION,
        "host.hmac-sha256",
        HostLane::Services,
    ),
    HostOperationPolicy::new(
        HMAC_SHA256_VERIFY_OPERATION,
        "host.hmac-sha256",
        HostLane::Services,
    ),
    HostOperationPolicy::new(FILE_READ_BYTES_OPERATION, "host.file-read", HostLane::File),
    HostOperationPolicy::new(FILE_READ_STREAM_OPERATION, "host.file-read", HostLane::File),
    HostOperationPolicy::new(
        FILE_WRITE_BYTES_OPERATION,
        "host.file-write",
        HostLane::File,
    ),
    HostOperationPolicy::new(
        CONTENT_IMPORT_OPERATION,
        "host.content-import",
        HostLane::File,
    ),
    HostOperationPolicy::new(CONTENT_SAVE_OPERATION, "host.content-save", HostLane::File),
    HostOperationPolicy::new(
        WELLEN_OPEN_OPERATION,
        "host.waveform-read",
        HostLane::Waveform,
    ),
    HostOperationPolicy::new(
        WELLEN_HIERARCHY_PAGE_OPERATION,
        "host.waveform-read",
        HostLane::Waveform,
    ),
    HostOperationPolicy::new(
        WELLEN_SIGNAL_PAGE_OPERATION,
        "host.waveform-read",
        HostLane::Waveform,
    ),
    HostOperationPolicy::new(
        WELLEN_CURSOR_VALUES_OPERATION,
        "host.waveform-read",
        HostLane::Waveform,
    ),
];

struct ProductionHostPolicy {
    authorized_effects: BTreeMap<EffectId, HostLane>,
}

impl ProductionHostPolicy {
    fn from_bundle(bundle: &LoadedAppBundle) -> Result<Self, TransientEffectHostError> {
        Self::from_role_artifacts(
            bundle.manifest(),
            [
                (ProgramRole::Session, bundle.session_artifact()),
                (ProgramRole::Server, bundle.server_artifact()),
            ],
        )
    }

    fn from_role_artifacts<'a>(
        manifest: &BundleManifest,
        artifacts: impl IntoIterator<Item = (ProgramRole, &'a ProgramArtifact)>,
    ) -> Result<Self, TransientEffectHostError> {
        let mut authorized_effects = BTreeMap::new();
        for (role, artifact) in artifacts {
            if !matches!(role, ProgramRole::Session | ProgramRole::Server)
                || artifact.role() != role
            {
                return Err(TransientEffectHostError::new(
                    "production host policy accepts only matching Session and Server artifacts",
                ));
            }
            let descriptor = manifest.artifact(role).ok_or_else(|| {
                TransientEffectHostError::new(format_args!(
                    "bundle has no {} artifact descriptor for host policy",
                    role.as_str()
                ))
            })?;
            let profile = manifest
                .capability_profile(&descriptor.capability_profile_id)
                .ok_or_else(|| {
                    TransientEffectHostError::new(format_args!(
                        "{} artifact selects missing capability profile `{}`",
                        role.as_str(),
                        descriptor.capability_profile_id
                    ))
                })?;
            if profile.role != role {
                return Err(TransientEffectHostError::new(format_args!(
                    "{} artifact capability profile `{}` belongs to {}",
                    role.as_str(),
                    profile.id,
                    profile.role.as_str()
                )));
            }

            for contract in &artifact.plan().effects {
                let operation =
                    host_operation_policy(&contract.host_operation).ok_or_else(|| {
                        TransientEffectHostError::new(format_args!(
                            "{} host operation `{}` has no supported production policy mapping",
                            role.as_str(),
                            contract.host_operation
                        ))
                    })?;
                if contract.effect_id != effect_id(operation.operation)? {
                    return Err(TransientEffectHostError::new(format_args!(
                        "{} host operation `{}` has a non-canonical effect ID",
                        role.as_str(),
                        contract.host_operation
                    )));
                }
                if !profile.grants.iter().any(|grant| grant == operation.grant) {
                    return Err(TransientEffectHostError::new(format_args!(
                        "{} host operation `{}` requires grant `{}` in selected profile `{}`",
                        role.as_str(),
                        contract.host_operation,
                        operation.grant,
                        profile.id
                    )));
                }
                if let Some(previous) =
                    authorized_effects.insert(contract.effect_id, operation.lane)
                    && previous != operation.lane
                {
                    return Err(TransientEffectHostError::new(
                        "production host policy maps one effect ID to multiple adapters",
                    ));
                }
            }
        }
        Ok(Self { authorized_effects })
    }
}

fn host_operation_policy(operation: &str) -> Option<&'static HostOperationPolicy> {
    HOST_OPERATION_POLICIES
        .iter()
        .find(|policy| policy.operation == operation)
}

fn effect_id(operation: &str) -> Result<EffectId, TransientEffectHostError> {
    EffectId::from_host_operation(operation).map_err(TransientEffectHostError::new)
}

struct QueuedEvent {
    lane: HostLane,
    event: TransientEffectHostEvent,
}

/// One exact-call event lane for every generic production host adapter.
///
/// Runtime call IDs are the only ownership keys. The host never infers
/// replacement from application values, invocation IDs, paths, or example
/// identity.
pub struct ServerTransientEffectHost {
    file: FileEffectAdapter,
    http: OutboundHttpEffectAdapter,
    services: HostServiceEffectAdapter,
    waveform: WaveformEffectWorker,
    calls: ExactCallHostCore<HostLane>,
    ready: VecDeque<QueuedEvent>,
}

impl ServerTransientEffectHost {
    pub fn new_for_bundle(
        bundle: &LoadedAppBundle,
        mut file: FileEffectAdapter,
        http: OutboundHttpEffectAdapter,
        services: HostServiceEffectAdapter,
        waveform: WaveformEffectWorker,
    ) -> Result<Self, TransientEffectHostError> {
        let policy = ProductionHostPolicy::from_bundle(bundle)?;
        register_bundle_assets(&mut file, bundle)?;
        Self::new_with_policy(file, http, services, waveform, policy)
    }

    fn new_with_policy(
        file: FileEffectAdapter,
        http: OutboundHttpEffectAdapter,
        services: HostServiceEffectAdapter,
        waveform: WaveformEffectWorker,
        policy: ProductionHostPolicy,
    ) -> Result<Self, TransientEffectHostError> {
        let calls =
            ExactCallHostCore::new(policy.authorized_effects, MAX_PRODUCTION_ACTIVE_EFFECTS)?;
        let host = Self {
            file,
            http,
            services,
            waveform,
            calls,
            ready: VecDeque::new(),
        };
        host.validate_unique_effect_ownership()?;
        host.validate_policy_mappings()?;
        Ok(host)
    }

    fn validate_unique_effect_ownership(&self) -> Result<(), TransientEffectHostError> {
        let mut ids = BTreeSet::new();
        let all = self
            .file
            .effect_ids()
            .into_iter()
            .chain(std::iter::once(self.http.effect_id()))
            .chain(self.services.effect_ids())
            .chain(self.waveform.effect_ids());
        for id in all {
            if !ids.insert(id) {
                return Err(TransientEffectHostError::new(
                    "production host adapters claim the same effect ID",
                ));
            }
        }
        Ok(())
    }

    fn adapter_lane_for(&self, effect_id: EffectId) -> Option<HostLane> {
        if self.file.owns_effect(effect_id) {
            Some(HostLane::File)
        } else if effect_id == self.http.effect_id() {
            Some(HostLane::Http)
        } else if self.services.owns(effect_id) {
            Some(HostLane::Services)
        } else if self.waveform.owns(effect_id) {
            Some(HostLane::Waveform)
        } else {
            None
        }
    }

    fn validate_policy_mappings(&self) -> Result<(), TransientEffectHostError> {
        let mut mapped = BTreeSet::new();
        for policy in HOST_OPERATION_POLICIES {
            let id = effect_id(policy.operation)?;
            if !mapped.insert(id) {
                return Err(TransientEffectHostError::new(
                    "production host policy repeats a host operation effect ID",
                ));
            }
            if self.adapter_lane_for(id) != Some(policy.lane) {
                return Err(TransientEffectHostError::new(format_args!(
                    "production host policy operation `{}` is not owned by its declared adapter",
                    policy.operation
                )));
            }
        }
        for (id, lane) in self.calls.authorized_entries() {
            if self.adapter_lane_for(id) != Some(lane) {
                return Err(TransientEffectHostError::new(
                    "authorized production effect is unavailable from its declared adapter",
                ));
            }
        }
        Ok(())
    }

    fn lane_for(&self, effect_id: EffectId) -> Option<HostLane> {
        self.calls.authorized_lane(effect_id)
    }

    fn queue_single(
        &mut self,
        lane: HostLane,
        call_id: TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) {
        self.ready.push_back(QueuedEvent {
            lane,
            event: TransientEffectHostEvent::Result {
                call_id,
                delivery: TransientEffectHostDelivery::Single,
                outcome,
            },
        });
    }

    fn cancel_submitted(&mut self, calls: &[TransientEffectCallId]) {
        for call_id in calls {
            self.cancel(*call_id);
        }
    }

    fn pop_ready(&mut self) -> Option<TransientEffectHostEvent> {
        while let Some(queued) = self.ready.pop_front() {
            let call_id = event_call_id(&queued.event);
            if self
                .calls
                .accept_result(call_id, queued.lane, true)
                .is_err()
            {
                continue;
            }
            return Some(queued.event);
        }
        None
    }

    fn accepts_lane(&self, lane: HostLane) -> bool {
        self.calls.active_in_lane(lane)
    }

    fn accept_file_event(
        &mut self,
        event: boon_host_runtime::FileEffectEvent,
    ) -> Option<TransientEffectHostEvent> {
        if self
            .calls
            .accept_result(event.call_id, HostLane::File, event.is_terminal())
            .is_err()
        {
            return None;
        }
        Some(TransientEffectHostEvent::Result {
            call_id: event.call_id,
            delivery: if event.is_stream() {
                TransientEffectHostDelivery::Stream {
                    result_sequence: event.result_sequence,
                }
            } else {
                TransientEffectHostDelivery::Single
            },
            outcome: event.outcome,
        })
    }

    fn accept_single(
        &mut self,
        lane: HostLane,
        call_id: TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> Option<TransientEffectHostEvent> {
        if self.calls.accept_result(call_id, lane, true).is_err() {
            return None;
        }
        Some(TransientEffectHostEvent::Result {
            call_id,
            delivery: TransientEffectHostDelivery::Single,
            outcome,
        })
    }
}

#[async_trait]
impl TransientEffectHost for ServerTransientEffectHost {
    fn owns(&self, effect_id: EffectId) -> bool {
        self.lane_for(effect_id).is_some()
    }

    fn submit(
        &mut self,
        calls: Vec<TransientEffectInvocation>,
    ) -> Result<(), TransientEffectHostError> {
        let admitted = self.calls.admit(calls)?;
        let admitted_call_ids = admitted
            .iter()
            .map(|(_, call)| call.call_id)
            .collect::<Vec<_>>();
        let mut submitted = Vec::with_capacity(admitted.len());
        for (lane, call) in admitted {
            submitted.push(call.call_id);
            let result = match lane {
                HostLane::File => self
                    .file
                    .submit(call)
                    .map(|_| ())
                    .map_err(TransientEffectHostError::new),
                HostLane::Http => self
                    .http
                    .submit(call)
                    .map_err(TransientEffectHostError::new)
                    .map(|submission| {
                        if let Some(completion) = submission.immediate_completion {
                            self.queue_single(
                                HostLane::Http,
                                completion.call_id,
                                completion.outcome,
                            );
                        }
                    }),
                HostLane::Services => self
                    .services
                    .submit(call)
                    .map_err(TransientEffectHostError::new)
                    .map(|submission| {
                        if let Some(completion) = submission.immediate_completion {
                            self.queue_single(
                                HostLane::Services,
                                completion.call_id,
                                completion.outcome,
                            );
                        }
                    }),
                HostLane::Waveform => self
                    .waveform
                    .submit(call)
                    .map_err(TransientEffectHostError::new),
            };
            if let Err(error) = result {
                self.cancel_submitted(&submitted);
                self.calls.rollback_admitted(&admitted_call_ids);
                return Err(error);
            }
        }
        Ok(())
    }

    async fn next_event(&mut self) -> Result<TransientEffectHostEvent, TransientEffectHostError> {
        loop {
            if let Some(event) = self.pop_ready() {
                return Ok(event);
            }
            let wait_file = self.accepts_lane(HostLane::File);
            let wait_http = self.accepts_lane(HostLane::Http);
            let wait_services = self.accepts_lane(HostLane::Services);
            let wait_waveform = self.accepts_lane(HostLane::Waveform);
            if !(wait_file || wait_http || wait_services || wait_waveform) {
                return Err(TransientEffectHostError::new(
                    "production host has no active call to await",
                ));
            }

            enum Completion {
                File(Result<boon_host_runtime::FileEffectEvent, String>),
                Http(Result<boon_http_runtime::HttpEffectCompletion, String>),
                Services(Result<boon_host_runtime::HostServiceEffectCompletion, String>),
                Waveform(Result<boon_wellen_host::WaveformEffectCompletion, String>),
            }

            let completion = tokio::select! {
                result = self.file.next_event(), if wait_file => {
                    Completion::File(result.map_err(|error| error.to_string()))
                }
                result = self.http.next_completion(), if wait_http => {
                    Completion::Http(result.map_err(|error| error.to_string()))
                }
                result = self.services.next_completion(), if wait_services => {
                    Completion::Services(result.map_err(|error| error.to_string()))
                }
                result = self.waveform.next_completion(), if wait_waveform => {
                    Completion::Waveform(result.map_err(|error| error.to_string()))
                }
                else => return Err(TransientEffectHostError::new(
                    "production host event selection has no enabled lane",
                )),
            };

            let event = match completion {
                Completion::File(result) => {
                    self.accept_file_event(result.map_err(TransientEffectHostError::new)?)
                }
                Completion::Http(result) => {
                    let completion = result.map_err(TransientEffectHostError::new)?;
                    self.accept_single(HostLane::Http, completion.call_id, completion.outcome)
                }
                Completion::Services(result) => {
                    let completion = result.map_err(TransientEffectHostError::new)?;
                    self.accept_single(HostLane::Services, completion.call_id, completion.outcome)
                }
                Completion::Waveform(result) => {
                    let completion = result.map_err(TransientEffectHostError::new)?;
                    self.accept_single(HostLane::Waveform, completion.call_id, completion.outcome)
                }
            };
            if let Some(event) = event {
                return Ok(event);
            }
        }
    }

    fn grant_credits(
        &mut self,
        grants: &[TransientEffectCreditGrant],
    ) -> Result<(), TransientEffectHostError> {
        for (lane, grant) in self.calls.credit_lanes(grants)? {
            if lane != HostLane::File {
                return Err(TransientEffectHostError::new(
                    "stream credit targets a call not owned by the file lane",
                ));
            }
            let accepted = self
                .file
                .accept_credit_grant(grant)
                .map_err(TransientEffectHostError::new)?;
            if !accepted {
                return Err(TransientEffectHostError::new(
                    "file lane rejected credit for an active runtime call",
                ));
            }
        }
        Ok(())
    }

    fn cancel(&mut self, call_id: TransientEffectCallId) {
        for (lane, call_id) in self.calls.cancel_calls(&[call_id]) {
            match lane {
                HostLane::File => {
                    self.file.cancel(call_id);
                }
                HostLane::Http => {
                    self.http.cancel(call_id);
                }
                HostLane::Services => {
                    self.services.cancel(call_id);
                }
                HostLane::Waveform => {
                    self.waveform.cancel(call_id);
                }
            }
        }
    }

    fn shutdown(&mut self) {
        let calls = self.calls.active_call_ids();
        for call_id in calls {
            self.cancel(call_id);
        }
        self.ready.clear();
        let _ = self.waveform.shutdown();
    }
}

fn event_call_id(event: &TransientEffectHostEvent) -> TransientEffectCallId {
    match event {
        TransientEffectHostEvent::Result { call_id, .. }
        | TransientEffectHostEvent::Cancelled { call_id } => *call_id,
    }
}

fn register_bundle_assets(
    file: &mut FileEffectAdapter,
    bundle: &LoadedAppBundle,
) -> Result<(), TransientEffectHostError> {
    register_declared_assets(file, bundle.manifest(), |descriptor| {
        bundle
            .read_file(descriptor)
            .map_err(TransientEffectHostError::new)
    })
}

fn register_declared_assets(
    file: &mut FileEffectAdapter,
    manifest: &BundleManifest,
    mut read: impl FnMut(&BundleFileDescriptor) -> Result<Vec<u8>, TransientEffectHostError>,
) -> Result<(), TransientEffectHostError> {
    for descriptor in manifest
        .files
        .iter()
        .filter(|descriptor| descriptor.kind == BundleFileKind::Asset)
    {
        let bytes = read(descriptor)?;
        if bytes.len() != descriptor.bytes_len {
            return Err(TransientEffectHostError::new(format_args!(
                "package asset `{}` size differs from verified bundle metadata",
                descriptor.path
            )));
        }
        let url = package_asset_url(&manifest.package_id, &descriptor.path);
        let content = file
            .register_package_asset(url, package_asset_media_type(&descriptor.path), &bytes)
            .map_err(TransientEffectHostError::new)?;
        if digest_hex(content.digest()) != descriptor.bytes_sha256 {
            return Err(TransientEffectHostError::new(format_args!(
                "package asset `{}` digest differs from verified bundle metadata",
                descriptor.path
            )));
        }
    }
    Ok(())
}

fn package_asset_url(package_id: &str, path: &str) -> String {
    format!("asset://{package_id}/{path}")
}

fn package_asset_media_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "cbor" => "application/cbor",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "vcd" => "text/x-vcd; charset=utf-8",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn digest_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}
