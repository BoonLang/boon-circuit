use boon_host_runtime::{
    ContentStore, ContentStoreLimits, FileCapabilityRegistry, FileReadStreamEffectAdapter,
    FileReadStreamEvent,
};
use boon_runtime::{RuntimeTurn, TransientEffectCallId, Value};
use boon_wellen_host::{WaveformEffectCompletion, WaveformEffectLimits, WaveformEffectWorker};
use std::path::PathBuf;

const FILE_CAPABILITY_LIMIT: usize = 64;
const ACTIVE_FILE_STREAM_LIMIT: usize = 8;
const WAVEFORM_CACHE_LIMIT: usize = 8;
const WAVEFORM_PENDING_LIMIT: usize = 32;
const CONTENT_SPARE_ENTRIES: usize = 64;
const CONTENT_SPARE_BYTES: u64 = 512 * 1024 * 1024;

pub(crate) struct PackageAsset<'a> {
    pub url: &'a str,
    pub bytes: &'a [u8],
}

pub(crate) enum TransientHostCompletion {
    Single {
        call_id: TransientEffectCallId,
        outcome: Value,
    },
    Stream(FileReadStreamEvent),
}

pub(crate) struct NativeTransientHost {
    file_streams: FileReadStreamEffectAdapter,
    waveforms: WaveformEffectWorker,
}

impl NativeTransientHost {
    pub fn new<'a>(
        root: PathBuf,
        assets: impl IntoIterator<Item = PackageAsset<'a>>,
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
        let mut file_streams = FileReadStreamEffectAdapter::new(
            capabilities,
            content_store.clone(),
            ACTIVE_FILE_STREAM_LIMIT,
        )
        .map_err(|error| error.to_string())?;
        for asset in assets {
            file_streams
                .register_package_asset(asset.url, asset.bytes)
                .map_err(|error| error.to_string())?;
        }
        let waveforms = WaveformEffectWorker::start(
            content_store,
            WaveformEffectLimits::new(WAVEFORM_CACHE_LIMIT),
            WAVEFORM_PENDING_LIMIT,
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            file_streams,
            waveforms,
        })
    }

    pub fn route_turn(&mut self, turn: &RuntimeTurn) -> Result<(), String> {
        self.file_streams
            .route_runtime_turn(turn)
            .map_err(|error| error.to_string())?;
        for call_id in &turn.cancelled_transient_effects {
            self.waveforms.cancel(*call_id);
        }
        for invocation in &turn.transient_effects {
            if invocation.effect_id == self.file_streams.effect_id() {
                self.file_streams
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string())?;
            } else if self.waveforms.owns(invocation.effect_id) {
                self.waveforms
                    .submit(invocation.clone())
                    .map_err(|error| error.to_string())?;
            } else {
                return Err(format!(
                    "native transient host has no adapter for effect {}",
                    invocation.effect_id
                ));
            }
        }
        Ok(())
    }

    pub fn try_completion(&mut self) -> Result<Option<TransientHostCompletion>, String> {
        if let Some(event) = self
            .file_streams
            .try_next_event()
            .map_err(|error| error.to_string())?
        {
            return Ok(Some(TransientHostCompletion::Stream(event)));
        }
        self.waveforms
            .try_completion()
            .map(|completion| completion.map(single_completion))
            .map_err(|error| error.to_string())
    }

    pub fn has_work(&self) -> bool {
        self.file_streams.owned_call_count() > 0 || self.waveforms.is_busy()
    }
}

fn single_completion(completion: WaveformEffectCompletion) -> TransientHostCompletion {
    TransientHostCompletion::Single {
        call_id: completion.call_id,
        outcome: completion.outcome,
    }
}
