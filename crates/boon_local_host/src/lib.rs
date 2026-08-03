use boon_host_runtime::{
    ContentStore, ContentStoreLimits, FileCapabilityRegistry, FileEffectAdapter, FileEffectEvent,
    HostServiceEffectAdapter, NamedSecret,
};
use boon_host_services::{HostServiceConfig, HostServices};
use boon_http_client::{ClientConfig, HttpClient};
use boon_http_runtime::OutboundHttpEffectAdapter;
use boon_plan::EffectId;
use boon_runtime::{
    ExactCallHostCore, RuntimeTurn, TransientEffectCallId, TransientEffectCreditGrant,
    TransientEffectInvocation, Value,
};
use boon_wellen_host::{WaveformEffectCompletion, WaveformEffectLimits, WaveformEffectWorker};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

const FILE_CAPABILITY_LIMIT: usize = 64;
const ACTIVE_FILE_STREAM_LIMIT: usize = 8;
const ACTIVE_HTTP_REQUEST_LIMIT: usize = 8;
const ACTIVE_DEADLINE_LIMIT: usize = 32;
const WAVEFORM_CACHE_LIMIT: usize = 8;
const WAVEFORM_PENDING_LIMIT: usize = 32;
const CONTENT_SPARE_ENTRIES: usize = 64;
const CONTENT_SPARE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ACTIVE_TRANSIENT_EFFECTS: usize = ACTIVE_FILE_STREAM_LIMIT
    + ACTIVE_HTTP_REQUEST_LIMIT
    + ACTIVE_DEADLINE_LIMIT
    + WAVEFORM_PENDING_LIMIT;

pub struct PackageAsset<'a> {
    pub url: &'a str,
    pub media: &'a str,
    pub bytes: &'a [u8],
}

pub enum LocalTransientCompletion {
    Single {
        call_id: TransientEffectCallId,
        outcome: Value,
    },
    File(FileEffectEvent),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalHostLane {
    File,
    Http,
    Services,
    Waveform,
}

struct QueuedSingleCompletion {
    lane: LocalHostLane,
    call_id: TransientEffectCallId,
    outcome: Value,
}

pub struct LocalTransientHost {
    file_streams: FileEffectAdapter,
    http: OutboundHttpEffectAdapter,
    services: HostServiceEffectAdapter,
    waveforms: WaveformEffectWorker,
    calls: ExactCallHostCore<LocalHostLane>,
    ready: VecDeque<QueuedSingleCompletion>,
    async_runtime: tokio::runtime::Runtime,
}

impl LocalTransientHost {
    pub fn new<'a>(
        root: PathBuf,
        assets: impl IntoIterator<Item = PackageAsset<'a>>,
        required_effects: impl IntoIterator<Item = EffectId>,
    ) -> Result<Self, String> {
        let assets = assets.into_iter().collect::<Vec<_>>();
        let asset_bytes = assets.iter().try_fold(0_u64, |total, asset| {
            let bytes = u64::try_from(asset.bytes.len())
                .map_err(|_| "package asset byte length exceeds the host range".to_owned())?;
            total
                .checked_add(bytes)
                .ok_or_else(|| "package asset byte total overflow".to_owned())
        })?;
        let max_entries = assets
            .len()
            .checked_add(CONTENT_SPARE_ENTRIES)
            .ok_or_else(|| "package asset entry capacity overflow".to_owned())?;
        let max_bytes = asset_bytes
            .checked_add(CONTENT_SPARE_BYTES)
            .ok_or_else(|| "package asset content capacity overflow".to_owned())?;
        let content_store = ContentStore::new(
            root,
            ContentStoreLimits::new(max_entries.max(CONTENT_SPARE_ENTRIES), max_bytes),
        )
        .map_err(|error| error.to_string())?;
        let capabilities = FileCapabilityRegistry::new(FILE_CAPABILITY_LIMIT)
            .map_err(|error| error.to_string())?;
        let mut file_streams = FileEffectAdapter::new(
            capabilities,
            content_store.clone(),
            ACTIVE_FILE_STREAM_LIMIT,
        )
        .map_err(|error| error.to_string())?;
        for asset in assets {
            file_streams
                .register_package_asset(asset.url, asset.media, asset.bytes)
                .map_err(|error| error.to_string())?;
        }
        let waveforms = WaveformEffectWorker::start(
            content_store,
            WaveformEffectLimits::new(WAVEFORM_CACHE_LIMIT),
            WAVEFORM_PENDING_LIMIT,
        )
        .map_err(|error| error.to_string())?;
        let async_runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|error| format!("cannot start local effect runtime: {error}"))?;
        let http = OutboundHttpEffectAdapter::new(
            HttpClient::new(ClientConfig::new(Vec::new())).map_err(|error| error.to_string())?,
            ACTIVE_HTTP_REQUEST_LIMIT,
        )
        .map_err(|error| error.to_string())?;
        let services = HostServiceEffectAdapter::new(
            HostServices::new(HostServiceConfig::default()),
            Vec::<NamedSecret>::new(),
            ACTIVE_DEADLINE_LIMIT,
        )
        .map_err(|error| error.to_string())?;
        let mut authorized_effects = BTreeMap::new();
        for effect_id in required_effects {
            let mut lanes = Vec::with_capacity(4);
            if file_streams.owns_effect(effect_id) {
                lanes.push(LocalHostLane::File);
            }
            if effect_id == http.effect_id() {
                lanes.push(LocalHostLane::Http);
            }
            if services.owns(effect_id) {
                lanes.push(LocalHostLane::Services);
            }
            if waveforms.owns(effect_id) {
                lanes.push(LocalHostLane::Waveform);
            }
            let lane = match lanes.as_slice() {
                [lane] => *lane,
                [] => {
                    return Err(format!(
                        "local host has no adapter for required effect {effect_id}"
                    ));
                }
                _ => {
                    return Err(format!(
                        "local host adapters ambiguously own required effect {effect_id}"
                    ));
                }
            };
            if authorized_effects.insert(effect_id, lane).is_some() {
                return Err(format!(
                    "local host plan repeats required effect {effect_id}"
                ));
            }
        }
        let calls = ExactCallHostCore::new(authorized_effects, MAX_ACTIVE_TRANSIENT_EFFECTS)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            file_streams,
            http,
            services,
            waveforms,
            calls,
            ready: VecDeque::new(),
            async_runtime,
        })
    }

    pub fn route_turn(&mut self, turn: &RuntimeTurn) -> Result<(), String> {
        self.route_batch(
            &turn.cancelled_transient_effects,
            &turn.transient_effect_credit_grants,
            &turn.transient_effects,
        )
    }

    pub fn route_batch(
        &mut self,
        cancelled: &[TransientEffectCallId],
        credits: &[TransientEffectCreditGrant],
        invocations: &[TransientEffectInvocation],
    ) -> Result<(), String> {
        for (lane, call_id) in self.calls.cancel_calls(cancelled) {
            self.cancel_adapter(lane, call_id);
        }
        for (lane, grant) in self
            .calls
            .credit_lanes(credits)
            .map_err(|error| error.to_string())?
        {
            if lane != LocalHostLane::File {
                return Err(format!(
                    "local stream credit targets unowned call {}",
                    grant.call_id
                ));
            }
            if !self
                .file_streams
                .accept_credit_grant(grant)
                .map_err(|error| error.to_string())?
            {
                return Err(format!(
                    "local file lane rejected credit for active call {}",
                    grant.call_id
                ));
            }
        }
        let admitted = self
            .calls
            .admit(invocations.to_vec())
            .map_err(|error| error.to_string())?;
        let admitted_call_ids = admitted
            .iter()
            .map(|(_, invocation)| invocation.call_id)
            .collect::<Vec<_>>();
        let mut submitted = Vec::with_capacity(admitted.len());
        for (lane, invocation) in admitted {
            let _runtime = self.async_runtime.enter();
            let result = match lane {
                LocalHostLane::File => self
                    .file_streams
                    .submit(invocation.clone())
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                LocalHostLane::Http => self
                    .http
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string())
                    .map(|submission| {
                        if let Some(completion) = submission.immediate_completion {
                            self.ready.push_back(QueuedSingleCompletion {
                                lane: LocalHostLane::Http,
                                call_id: completion.call_id,
                                outcome: completion.outcome,
                            });
                        }
                    }),
                LocalHostLane::Services => self
                    .services
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string())
                    .map(|submission| {
                        if let Some(completion) = submission.immediate_completion {
                            self.ready.push_back(QueuedSingleCompletion {
                                lane: LocalHostLane::Services,
                                call_id: completion.call_id,
                                outcome: completion.outcome,
                            });
                        }
                    }),
                LocalHostLane::Waveform => self
                    .waveforms
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string()),
            };
            if let Err(error) = result {
                for (submitted_lane, call_id) in submitted {
                    self.cancel_adapter(submitted_lane, call_id);
                }
                self.calls.rollback_admitted(&admitted_call_ids);
                return Err(error);
            }
            submitted.push((lane, invocation.call_id));
        }
        Ok(())
    }

    pub fn try_completion(&mut self) -> Result<Option<LocalTransientCompletion>, String> {
        while let Some(completion) = self.ready.pop_front() {
            if self
                .calls
                .accept_result(completion.call_id, completion.lane, true)
                .is_err()
            {
                continue;
            }
            return Ok(Some(LocalTransientCompletion::Single {
                call_id: completion.call_id,
                outcome: completion.outcome,
            }));
        }
        if let Some(event) = self
            .file_streams
            .try_next_event()
            .map_err(|error| error.to_string())?
        {
            self.calls
                .accept_result(event.call_id, LocalHostLane::File, event.is_terminal())
                .map_err(|error| error.to_string())?;
            return Ok(Some(LocalTransientCompletion::File(event)));
        }
        if let Some(completion) = self
            .http
            .try_next_completion()
            .map_err(|error| error.to_string())?
        {
            self.calls
                .accept_result(completion.call_id, LocalHostLane::Http, true)
                .map_err(|error| error.to_string())?;
            return Ok(Some(LocalTransientCompletion::Single {
                call_id: completion.call_id,
                outcome: completion.outcome,
            }));
        }
        if let Some(completion) = self
            .services
            .try_next_completion()
            .map_err(|error| error.to_string())?
        {
            self.calls
                .accept_result(completion.call_id, LocalHostLane::Services, true)
                .map_err(|error| error.to_string())?;
            return Ok(Some(LocalTransientCompletion::Single {
                call_id: completion.call_id,
                outcome: completion.outcome,
            }));
        }
        let completion = self
            .waveforms
            .try_completion()
            .map_err(|error| error.to_string())?;
        let Some(completion) = completion else {
            return Ok(None);
        };
        self.calls
            .accept_result(completion.call_id, LocalHostLane::Waveform, true)
            .map_err(|error| error.to_string())?;
        Ok(Some(single_completion(completion)))
    }

    pub fn has_work(&self) -> bool {
        self.calls.active_count() != 0
    }

    fn cancel(&mut self, call_id: TransientEffectCallId) {
        for (lane, call_id) in self.calls.cancel_calls(&[call_id]) {
            self.cancel_adapter(lane, call_id);
        }
    }

    fn cancel_adapter(&mut self, lane: LocalHostLane, call_id: TransientEffectCallId) {
        match lane {
            LocalHostLane::File => {
                self.file_streams.cancel(call_id);
            }
            LocalHostLane::Http => {
                self.http.cancel(call_id);
            }
            LocalHostLane::Services => {
                self.services.cancel(call_id);
            }
            LocalHostLane::Waveform => {
                self.waveforms.cancel(call_id);
            }
        }
    }
}

impl Drop for LocalTransientHost {
    fn drop(&mut self) {
        let calls = self.calls.active_call_ids();
        for call_id in calls {
            self.cancel(call_id);
        }
    }
}

fn single_completion(completion: WaveformEffectCompletion) -> LocalTransientCompletion {
    LocalTransientCompletion::Single {
        call_id: completion.call_id,
        outcome: completion.outcome,
    }
}
