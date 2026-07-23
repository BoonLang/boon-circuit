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

pub(crate) struct PackageAsset<'a> {
    pub url: &'a str,
    pub media: &'a str,
    pub bytes: &'a [u8],
}

pub(crate) enum TransientHostCompletion {
    Single {
        call_id: TransientEffectCallId,
        outcome: Value,
    },
    File(FileEffectEvent),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeHostLane {
    File,
    Http,
    Services,
    Waveform,
}

struct QueuedSingleCompletion {
    lane: NativeHostLane,
    call_id: TransientEffectCallId,
    outcome: Value,
}

pub(crate) struct NativeTransientHost {
    file_streams: FileEffectAdapter,
    http: OutboundHttpEffectAdapter,
    services: HostServiceEffectAdapter,
    waveforms: WaveformEffectWorker,
    calls: ExactCallHostCore<NativeHostLane>,
    ready: VecDeque<QueuedSingleCompletion>,
    async_runtime: tokio::runtime::Runtime,
}

impl NativeTransientHost {
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
            .map_err(|error| format!("cannot start native effect runtime: {error}"))?;
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
                lanes.push(NativeHostLane::File);
            }
            if effect_id == http.effect_id() {
                lanes.push(NativeHostLane::Http);
            }
            if services.owns(effect_id) {
                lanes.push(NativeHostLane::Services);
            }
            if waveforms.owns(effect_id) {
                lanes.push(NativeHostLane::Waveform);
            }
            let lane = match lanes.as_slice() {
                [lane] => *lane,
                [] => {
                    return Err(format!(
                        "native preview has no adapter for required effect {effect_id}"
                    ));
                }
                _ => {
                    return Err(format!(
                        "native preview adapters ambiguously own required effect {effect_id}"
                    ));
                }
            };
            if authorized_effects.insert(effect_id, lane).is_some() {
                return Err(format!(
                    "native preview plan repeats required effect {effect_id}"
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
            if lane != NativeHostLane::File {
                return Err(format!(
                    "native stream credit targets unowned call {}",
                    grant.call_id
                ));
            }
            if !self
                .file_streams
                .accept_credit_grant(grant)
                .map_err(|error| error.to_string())?
            {
                return Err(format!(
                    "native file lane rejected credit for active call {}",
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
                NativeHostLane::File => self
                    .file_streams
                    .submit(invocation.clone())
                    .map(|_| ())
                    .map_err(|error| error.to_string()),
                NativeHostLane::Http => self
                    .http
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string())
                    .map(|submission| {
                        if let Some(completion) = submission.immediate_completion {
                            self.ready.push_back(QueuedSingleCompletion {
                                lane: NativeHostLane::Http,
                                call_id: completion.call_id,
                                outcome: completion.outcome,
                            });
                        }
                    }),
                NativeHostLane::Services => self
                    .services
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string())
                    .map(|submission| {
                        if let Some(completion) = submission.immediate_completion {
                            self.ready.push_back(QueuedSingleCompletion {
                                lane: NativeHostLane::Services,
                                call_id: completion.call_id,
                                outcome: completion.outcome,
                            });
                        }
                    }),
                NativeHostLane::Waveform => self
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

    pub fn try_completion(&mut self) -> Result<Option<TransientHostCompletion>, String> {
        while let Some(completion) = self.ready.pop_front() {
            if self
                .calls
                .accept_result(completion.call_id, completion.lane, true)
                .is_err()
            {
                continue;
            }
            return Ok(Some(TransientHostCompletion::Single {
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
                .accept_result(event.call_id, NativeHostLane::File, event.is_terminal())
                .map_err(|error| error.to_string())?;
            return Ok(Some(TransientHostCompletion::File(event)));
        }
        if let Some(completion) = self
            .http
            .try_next_completion()
            .map_err(|error| error.to_string())?
        {
            self.calls
                .accept_result(completion.call_id, NativeHostLane::Http, true)
                .map_err(|error| error.to_string())?;
            return Ok(Some(TransientHostCompletion::Single {
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
                .accept_result(completion.call_id, NativeHostLane::Services, true)
                .map_err(|error| error.to_string())?;
            return Ok(Some(TransientHostCompletion::Single {
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
            .accept_result(completion.call_id, NativeHostLane::Waveform, true)
            .map_err(|error| error.to_string())?;
        Ok(Some(single_completion(completion)))
    }

    pub fn has_work(&self) -> bool {
        self.calls.active_count() != 0
    }

    #[cfg(test)]
    pub fn active_call_count(&self) -> usize {
        self.calls.active_count()
    }

    #[cfg(test)]
    pub fn file_stream_outstanding_credits(&self) -> Vec<u32> {
        self.calls
            .active_call_ids()
            .into_iter()
            .filter_map(|call_id| self.file_streams.outstanding_credits(call_id))
            .collect()
    }

    #[cfg(test)]
    pub fn file_stream_owned_call_count(&self) -> usize {
        self.file_streams.owned_call_count()
    }

    #[cfg(test)]
    pub fn pending_content_writer_count(&self) -> usize {
        self.file_streams.content_store().pending_writer_count()
    }

    fn cancel(&mut self, call_id: TransientEffectCallId) {
        for (lane, call_id) in self.calls.cancel_calls(&[call_id]) {
            self.cancel_adapter(lane, call_id);
        }
    }

    fn cancel_adapter(&mut self, lane: NativeHostLane, call_id: TransientEffectCallId) {
        match lane {
            NativeHostLane::File => {
                self.file_streams.cancel(call_id);
            }
            NativeHostLane::Http => {
                self.http.cancel(call_id);
            }
            NativeHostLane::Services => {
                self.services.cancel(call_id);
            }
            NativeHostLane::Waveform => {
                self.waveforms.cancel(call_id);
            }
        }
    }
}

impl Drop for NativeTransientHost {
    fn drop(&mut self) {
        let calls = self.calls.active_call_ids();
        for call_id in calls {
            self.cancel(call_id);
        }
    }
}

fn single_completion(completion: WaveformEffectCompletion) -> TransientHostCompletion {
    TransientHostCompletion::Single {
        call_id: completion.call_id,
        outcome: completion.outcome,
    }
}
