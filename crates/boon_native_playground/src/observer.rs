use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const OBSERVER_SOCKET_ENV: &str = "BOON_VERIFY_OBSERVER_SOCKET";
pub const PROOF_MODE_ENV: &str = "BOON_VERIFY_PROOF_MODE";
pub const PROOF_ARTIFACT_DIR_ENV: &str = "BOON_VERIFY_PROOF_ARTIFACT_DIR";
pub const PROOF_SAMPLE_ORDINAL_ENV: &str = "BOON_VERIFY_PROOF_SAMPLE_ORDINAL";

const MAGIC: [u8; 4] = *b"BNVO";
const VERSION: u16 = 2;
const HEADER_BYTES: usize = 7;
const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_STRING_BYTES: usize = 8 * 1024;
const CLIENT_QUEUE_DEPTH: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum ObserverRole {
    Preview = 1,
    Dev = 2,
}

impl ObserverRole {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::Preview),
            2 => Ok(Self::Dev),
            _ => Err(ObserverError::InvalidEnum("role", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum InputKind {
    PointerMove = 1,
    PointerButton = 2,
    Wheel = 3,
    Keyboard = 4,
    Text = 5,
    Ime = 6,
    Focus = 7,
    Resize = 8,
    Accessibility = 9,
    Close = 10,
}

impl InputKind {
    fn decode(value: u8) -> Result<Self, ObserverError> {
        match value {
            1 => Ok(Self::PointerMove),
            2 => Ok(Self::PointerButton),
            3 => Ok(Self::Wheel),
            4 => Ok(Self::Keyboard),
            5 => Ok(Self::Text),
            6 => Ok(Self::Ime),
            7 => Ok(Self::Focus),
            8 => Ok(Self::Resize),
            9 => Ok(Self::Accessibility),
            10 => Ok(Self::Close),
            _ => Err(ObserverError::InvalidEnum("input kind", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FrameEvidenceKey {
    pub frame_id: u64,
    pub input_id: u64,
    pub content_id: u64,
    pub layout_id: u64,
    pub render_id: u64,
    pub surface_epoch: u64,
    pub present_id: u64,
    pub proof_id: u64,
}

impl FrameEvidenceKey {
    pub fn is_complete(&self) -> bool {
        self.frame_id != 0
            && self.input_id != 0
            && self.content_id != 0
            && self.layout_id != 0
            && self.render_id != 0
            && self.surface_epoch != 0
            && self.present_id != 0
            && self.proof_id != 0
    }

    fn encode(&self, out: &mut Encoder) {
        out.u64(self.frame_id);
        out.u64(self.input_id);
        out.u64(self.content_id);
        out.u64(self.layout_id);
        out.u64(self.render_id);
        out.u64(self.surface_epoch);
        out.u64(self.present_id);
        out.u64(self.proof_id);
    }

    fn decode(input: &mut Decoder<'_>) -> Result<Self, ObserverError> {
        Ok(Self {
            frame_id: input.u64()?,
            input_id: input.u64()?,
            content_id: input.u64()?,
            layout_id: input.u64()?,
            render_id: input.u64()?,
            surface_epoch: input.u64()?,
            present_id: input.u64()?,
            proof_id: input.u64()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoleMetadata {
    pub role: ObserverRole,
    pub pid: u32,
    pub surface_id: String,
    pub surface_epoch: u64,
    pub logical_width: f32,
    pub logical_height: f32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale: f64,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub adapter_device_type: String,
    pub software_adapter: bool,
    pub surface_format: String,
    pub present_mode: String,
    pub window_backend: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputAccepted {
    pub role: ObserverRole,
    pub event_sequence: u64,
    pub real_os: bool,
    pub callback_to_host_ns: u64,
    pub surface_epoch: u64,
    pub kind: InputKind,
    pub pointer_button_pressed: Option<bool>,
    pub pointer_x: Option<f32>,
    pub pointer_y: Option<f32>,
    pub target: Option<String>,
    pub target_source_path: Option<String>,
    pub visible_change: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FramePresented {
    pub role: ObserverRole,
    pub key: FrameEvidenceKey,
    pub event_sequence: Option<u64>,
    pub input_kind: Option<InputKind>,
    pub callback_to_host_ns: u64,
    pub input_to_present_us: u64,
    pub event_dispatch_us: u64,
    pub executor_us: u64,
    pub runtime_document_us: u64,
    pub document_update_us: u64,
    pub render_us: u64,
    pub document_scene_convert_us: u64,
    pub scene_key_us: u64,
    pub rect_vertices_us: u64,
    pub asset_prepare_us: u64,
    pub quad_batch_key_us: u64,
    pub quad_upload_us: u64,
    pub draw_pass_us: u64,
    pub retained_metrics_us: u64,
    pub text_render_us: u64,
    pub submit_us: u64,
    pub present_us: u64,
    pub frame_us: u64,
    pub observer_drop_count: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProofArtifact {
    pub path: String,
    pub sha256: String,
    pub byte_len: u64,
    pub capture_method: String,
    pub nonblank_samples: u64,
    pub unique_rgba_values: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObserverEvent {
    RoleMetadata(RoleMetadata),
    InputAccepted(InputAccepted),
    FramePresented(FramePresented),
    SourceSwitchAcknowledged {
        revision: u64,
        elapsed_us: u64,
    },
    SourceSwitchFinal {
        revision: u64,
        elapsed_us: u64,
        key: FrameEvidenceKey,
    },
    TestTarget {
        request_id: u64,
        node: String,
        source_path: String,
        x: f32,
        y: f32,
    },
    TestCompleted {
        request_id: u64,
        passed: bool,
        completed_steps: u32,
        message: String,
    },
    ProofRequested {
        key: FrameEvidenceKey,
        snapshot_prepare_us: u64,
    },
    ProofCompleted {
        key: FrameEvidenceKey,
        completed_after_frame_id: u64,
        elapsed_us: u64,
        replaced_count: u64,
        result_drop_count: u64,
        artifact: Option<ProofArtifact>,
        error: Option<String>,
    },
    RoleTarget {
        role: ObserverRole,
        node: String,
        x: f32,
        y: f32,
    },
    SourceFailed {
        revision: u64,
        stage: String,
        message: String,
    },
}

impl ObserverEvent {
    fn tag(&self) -> u8 {
        match self {
            Self::RoleMetadata(_) => 1,
            Self::InputAccepted(_) => 2,
            Self::FramePresented(_) => 3,
            Self::SourceSwitchAcknowledged { .. } => 4,
            Self::SourceSwitchFinal { .. } => 5,
            Self::TestTarget { .. } => 6,
            Self::TestCompleted { .. } => 7,
            Self::ProofRequested { .. } => 8,
            Self::ProofCompleted { .. } => 9,
            Self::RoleTarget { .. } => 10,
            Self::SourceFailed { .. } => 11,
        }
    }

    fn encode(&self, out: &mut Encoder) -> Result<(), ObserverError> {
        match self {
            Self::RoleMetadata(value) => {
                out.u8(value.role as u8);
                out.u32(value.pid);
                out.string(&value.surface_id)?;
                out.u64(value.surface_epoch);
                out.f32(value.logical_width);
                out.f32(value.logical_height);
                out.u32(value.physical_width);
                out.u32(value.physical_height);
                out.f64(value.scale);
                out.string(&value.adapter_name)?;
                out.string(&value.adapter_backend)?;
                out.string(&value.adapter_device_type)?;
                out.bool(value.software_adapter);
                out.string(&value.surface_format)?;
                out.string(&value.present_mode)?;
                out.string(&value.window_backend)?;
            }
            Self::InputAccepted(value) => {
                out.u8(value.role as u8);
                out.u64(value.event_sequence);
                out.bool(value.real_os);
                out.u64(value.callback_to_host_ns);
                out.u64(value.surface_epoch);
                out.u8(value.kind as u8);
                out.u8(match value.pointer_button_pressed {
                    None => 0,
                    Some(true) => 1,
                    Some(false) => 2,
                });
                out.optional_f32(value.pointer_x);
                out.optional_f32(value.pointer_y);
                out.optional_string(value.target.as_deref())?;
                out.optional_string(value.target_source_path.as_deref())?;
                out.bool(value.visible_change);
            }
            Self::FramePresented(value) => {
                out.u8(value.role as u8);
                value.key.encode(out);
                out.optional_u64(value.event_sequence);
                out.optional_u8(value.input_kind.map(|kind| kind as u8));
                out.u64(value.callback_to_host_ns);
                out.u64(value.input_to_present_us);
                out.u64(value.event_dispatch_us);
                out.u64(value.executor_us);
                out.u64(value.runtime_document_us);
                out.u64(value.document_update_us);
                out.u64(value.render_us);
                out.u64(value.document_scene_convert_us);
                out.u64(value.scene_key_us);
                out.u64(value.rect_vertices_us);
                out.u64(value.asset_prepare_us);
                out.u64(value.quad_batch_key_us);
                out.u64(value.quad_upload_us);
                out.u64(value.draw_pass_us);
                out.u64(value.retained_metrics_us);
                out.u64(value.text_render_us);
                out.u64(value.submit_us);
                out.u64(value.present_us);
                out.u64(value.frame_us);
                out.u64(value.observer_drop_count);
            }
            Self::SourceSwitchAcknowledged {
                revision,
                elapsed_us,
            } => {
                out.u64(*revision);
                out.u64(*elapsed_us);
            }
            Self::SourceSwitchFinal {
                revision,
                elapsed_us,
                key,
            } => {
                out.u64(*revision);
                out.u64(*elapsed_us);
                key.encode(out);
            }
            Self::TestTarget {
                request_id,
                node,
                source_path,
                x,
                y,
            } => {
                out.u64(*request_id);
                out.string(node)?;
                out.string(source_path)?;
                out.f32(*x);
                out.f32(*y);
            }
            Self::TestCompleted {
                request_id,
                passed,
                completed_steps,
                message,
            } => {
                out.u64(*request_id);
                out.bool(*passed);
                out.u32(*completed_steps);
                out.string(message)?;
            }
            Self::ProofRequested {
                key,
                snapshot_prepare_us,
            } => {
                key.encode(out);
                out.u64(*snapshot_prepare_us);
            }
            Self::ProofCompleted {
                key,
                completed_after_frame_id,
                elapsed_us,
                replaced_count,
                result_drop_count,
                artifact,
                error,
            } => {
                key.encode(out);
                out.u64(*completed_after_frame_id);
                out.u64(*elapsed_us);
                out.u64(*replaced_count);
                out.u64(*result_drop_count);
                out.bool(artifact.is_some());
                if let Some(artifact) = artifact {
                    out.string(&artifact.path)?;
                    out.string(&artifact.sha256)?;
                    out.u64(artifact.byte_len);
                    out.string(&artifact.capture_method)?;
                    out.u64(artifact.nonblank_samples);
                    out.u64(artifact.unique_rgba_values);
                }
                out.optional_string(error.as_deref())?;
            }
            Self::RoleTarget { role, node, x, y } => {
                out.u8(*role as u8);
                out.string(node)?;
                out.f32(*x);
                out.f32(*y);
            }
            Self::SourceFailed {
                revision,
                stage,
                message,
            } => {
                out.u64(*revision);
                out.string(stage)?;
                out.string(message)?;
            }
        }
        Ok(())
    }

    fn decode(tag: u8, input: &mut Decoder<'_>) -> Result<Self, ObserverError> {
        let event = match tag {
            1 => Self::RoleMetadata(RoleMetadata {
                role: ObserverRole::decode(input.u8()?)?,
                pid: input.u32()?,
                surface_id: input.string()?,
                surface_epoch: input.u64()?,
                logical_width: input.f32()?,
                logical_height: input.f32()?,
                physical_width: input.u32()?,
                physical_height: input.u32()?,
                scale: input.f64()?,
                adapter_name: input.string()?,
                adapter_backend: input.string()?,
                adapter_device_type: input.string()?,
                software_adapter: input.bool()?,
                surface_format: input.string()?,
                present_mode: input.string()?,
                window_backend: input.string()?,
            }),
            2 => Self::InputAccepted(InputAccepted {
                role: ObserverRole::decode(input.u8()?)?,
                event_sequence: input.u64()?,
                real_os: input.bool()?,
                callback_to_host_ns: input.u64()?,
                surface_epoch: input.u64()?,
                kind: InputKind::decode(input.u8()?)?,
                pointer_button_pressed: match input.u8()? {
                    0 => None,
                    1 => Some(true),
                    2 => Some(false),
                    value => return Err(ObserverError::InvalidEnum("pointer button state", value)),
                },
                pointer_x: input.optional_f32()?,
                pointer_y: input.optional_f32()?,
                target: input.optional_string()?,
                target_source_path: input.optional_string()?,
                visible_change: input.bool()?,
            }),
            3 => Self::FramePresented(FramePresented {
                role: ObserverRole::decode(input.u8()?)?,
                key: FrameEvidenceKey::decode(input)?,
                event_sequence: input.optional_u64()?,
                input_kind: input.optional_u8()?.map(InputKind::decode).transpose()?,
                callback_to_host_ns: input.u64()?,
                input_to_present_us: input.u64()?,
                event_dispatch_us: input.u64()?,
                executor_us: input.u64()?,
                runtime_document_us: input.u64()?,
                document_update_us: input.u64()?,
                render_us: input.u64()?,
                document_scene_convert_us: input.u64()?,
                scene_key_us: input.u64()?,
                rect_vertices_us: input.u64()?,
                asset_prepare_us: input.u64()?,
                quad_batch_key_us: input.u64()?,
                quad_upload_us: input.u64()?,
                draw_pass_us: input.u64()?,
                retained_metrics_us: input.u64()?,
                text_render_us: input.u64()?,
                submit_us: input.u64()?,
                present_us: input.u64()?,
                frame_us: input.u64()?,
                observer_drop_count: input.u64()?,
            }),
            4 => Self::SourceSwitchAcknowledged {
                revision: input.u64()?,
                elapsed_us: input.u64()?,
            },
            5 => Self::SourceSwitchFinal {
                revision: input.u64()?,
                elapsed_us: input.u64()?,
                key: FrameEvidenceKey::decode(input)?,
            },
            6 => Self::TestTarget {
                request_id: input.u64()?,
                node: input.string()?,
                source_path: input.string()?,
                x: input.f32()?,
                y: input.f32()?,
            },
            7 => Self::TestCompleted {
                request_id: input.u64()?,
                passed: input.bool()?,
                completed_steps: input.u32()?,
                message: input.string()?,
            },
            8 => Self::ProofRequested {
                key: FrameEvidenceKey::decode(input)?,
                snapshot_prepare_us: input.u64()?,
            },
            9 => {
                let key = FrameEvidenceKey::decode(input)?;
                let completed_after_frame_id = input.u64()?;
                let elapsed_us = input.u64()?;
                let replaced_count = input.u64()?;
                let result_drop_count = input.u64()?;
                let artifact = if input.bool()? {
                    Some(ProofArtifact {
                        path: input.string()?,
                        sha256: input.string()?,
                        byte_len: input.u64()?,
                        capture_method: input.string()?,
                        nonblank_samples: input.u64()?,
                        unique_rgba_values: input.u64()?,
                    })
                } else {
                    None
                };
                Self::ProofCompleted {
                    key,
                    completed_after_frame_id,
                    elapsed_us,
                    replaced_count,
                    result_drop_count,
                    artifact,
                    error: input.optional_string()?,
                }
            }
            10 => Self::RoleTarget {
                role: ObserverRole::decode(input.u8()?)?,
                node: input.string()?,
                x: input.f32()?,
                y: input.f32()?,
            },
            11 => Self::SourceFailed {
                revision: input.u64()?,
                stage: input.string()?,
                message: input.string()?,
            },
            _ => return Err(ObserverError::UnknownEvent(tag)),
        };
        input.finish()?;
        Ok(event)
    }
}

pub struct ObserverClient {
    sender: Option<mpsc::SyncSender<ObserverEvent>>,
    dropped: Arc<AtomicU64>,
    writer: Option<JoinHandle<()>>,
}

impl ObserverClient {
    pub fn from_env() -> Result<Option<Self>, ObserverError> {
        let Some(path) = std::env::var_os(OBSERVER_SOCKET_ENV) else {
            return Ok(None);
        };
        Self::connect(Path::new(&path)).map(Some)
    }

    pub fn connect(path: &Path) -> Result<Self, ObserverError> {
        let stream = UnixStream::connect(path)?;
        stream.set_write_timeout(Some(Duration::from_millis(250)))?;
        let (sender, receiver) = mpsc::sync_channel(CLIENT_QUEUE_DEPTH);
        let dropped = Arc::new(AtomicU64::new(0));
        let thread_dropped = Arc::clone(&dropped);
        let writer = thread::Builder::new()
            .name("boon-verifier-observer".to_owned())
            .spawn(move || observer_writer(stream, receiver, thread_dropped))?;
        Ok(Self {
            sender: Some(sender),
            dropped,
            writer: Some(writer),
        })
    }

    pub fn emit(&self, event: ObserverEvent) {
        let Some(sender) = &self.sender else {
            return;
        };
        if sender.try_send(event).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for ObserverClient {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn observer_writer(
    mut stream: UnixStream,
    receiver: mpsc::Receiver<ObserverEvent>,
    dropped: Arc<AtomicU64>,
) {
    for event in receiver {
        if write_event(&mut stream, &event).is_err() {
            dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
}

pub fn write_event(writer: &mut impl Write, event: &ObserverEvent) -> Result<(), ObserverError> {
    let mut encoded = Encoder::default();
    encoded.bytes.extend_from_slice(&MAGIC);
    encoded.u16(VERSION);
    encoded.u8(event.tag());
    event.encode(&mut encoded)?;
    if encoded.bytes.len() > MAX_EVENT_BYTES {
        return Err(ObserverError::FrameTooLarge(encoded.bytes.len()));
    }
    writer.write_all(&(encoded.bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&encoded.bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_event(reader: &mut impl Read) -> Result<Option<ObserverEvent>, ObserverError> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length[..1]) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if !(HEADER_BYTES..=MAX_EVENT_BYTES).contains(&length) {
        return Err(ObserverError::FrameTooLarge(length));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    if bytes[..4] != MAGIC {
        return Err(ObserverError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(ObserverError::UnsupportedVersion(version));
    }
    let mut input = Decoder::new(&bytes[HEADER_BYTES..]);
    ObserverEvent::decode(bytes[6], &mut input).map(Some)
}

#[derive(Debug)]
pub enum ObserverError {
    Io(io::Error),
    FrameTooLarge(usize),
    StringTooLarge(usize),
    InvalidMagic,
    UnsupportedVersion(u16),
    UnknownEvent(u8),
    InvalidEnum(&'static str, u8),
    InvalidBool(u8),
    InvalidOption(u8),
    InvalidUtf8(std::str::Utf8Error),
    Truncated,
    TrailingBytes(usize),
}

impl fmt::Display for ObserverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "observer I/O failed: {error}"),
            Self::FrameTooLarge(bytes) => write!(formatter, "observer frame is {bytes} bytes"),
            Self::StringTooLarge(bytes) => write!(formatter, "observer string is {bytes} bytes"),
            Self::InvalidMagic => formatter.write_str("observer frame magic is invalid"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "observer protocol version {version} is unsupported"
                )
            }
            Self::UnknownEvent(tag) => write!(formatter, "observer event tag {tag} is unknown"),
            Self::InvalidEnum(name, value) => {
                write!(formatter, "observer {name} value {value} is invalid")
            }
            Self::InvalidBool(value) => write!(formatter, "observer bool {value} is invalid"),
            Self::InvalidOption(value) => write!(formatter, "observer option {value} is invalid"),
            Self::InvalidUtf8(error) => write!(formatter, "observer UTF-8 is invalid: {error}"),
            Self::Truncated => formatter.write_str("observer frame is truncated"),
            Self::TrailingBytes(bytes) => {
                write!(formatter, "observer frame has {bytes} trailing bytes")
            }
        }
    }
}

impl std::error::Error for ObserverError {}

impl From<io::Error> for ObserverError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    fn optional_u8(&mut self, value: Option<u8>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.u8(value);
            }
            None => self.u8(0),
        }
    }

    fn optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.u64(value);
            }
            None => self.u8(0),
        }
    }

    fn optional_f32(&mut self, value: Option<f32>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.f32(value);
            }
            None => self.u8(0),
        }
    }

    fn optional_string(&mut self, value: Option<&str>) -> Result<(), ObserverError> {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value)?;
            }
            None => self.u8(0),
        }
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), ObserverError> {
        if value.len() > MAX_STRING_BYTES {
            return Err(ObserverError::StringTooLarge(value.len()));
        }
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
}

struct Decoder<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ObserverError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ObserverError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ObserverError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ObserverError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, ObserverError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(ObserverError::InvalidBool(value)),
        }
    }

    fn u32(&mut self) -> Result<u32, ObserverError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ObserverError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn f32(&mut self) -> Result<f32, ObserverError> {
        self.u32().map(f32::from_bits)
    }

    fn f64(&mut self) -> Result<f64, ObserverError> {
        self.u64().map(f64::from_bits)
    }

    fn optional_u8(&mut self) -> Result<Option<u8>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u8().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u64().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn optional_f32(&mut self) -> Result<Option<f32>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.f32().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn optional_string(&mut self) -> Result<Option<String>, ObserverError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            value => Err(ObserverError::InvalidOption(value)),
        }
    }

    fn string(&mut self) -> Result<String, ObserverError> {
        let length = self.u32()? as usize;
        if length > MAX_STRING_BYTES {
            return Err(ObserverError::StringTooLarge(length));
        }
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(ObserverError::InvalidUtf8)
    }

    fn finish(&self) -> Result<(), ObserverError> {
        let trailing = self.bytes.len().saturating_sub(self.offset);
        if trailing == 0 {
            Ok(())
        } else {
            Err(ObserverError::TrailingBytes(trailing))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(frame: u64) -> FrameEvidenceKey {
        FrameEvidenceKey {
            frame_id: frame,
            input_id: frame + 1,
            content_id: frame + 2,
            layout_id: frame + 3,
            render_id: frame + 4,
            surface_epoch: frame + 5,
            present_id: frame + 6,
            proof_id: frame + 7,
        }
    }

    fn roundtrip(event: ObserverEvent) {
        let mut bytes = Vec::new();
        write_event(&mut bytes, &event).expect("encode observer event");
        assert_eq!(read_event(&mut bytes.as_slice()).unwrap(), Some(event));
    }

    #[test]
    fn observer_codec_roundtrips_identity_timing_and_artifact_records() {
        roundtrip(ObserverEvent::InputAccepted(InputAccepted {
            role: ObserverRole::Dev,
            event_sequence: 3,
            real_os: true,
            callback_to_host_ns: 12,
            surface_epoch: 2,
            kind: InputKind::PointerButton,
            pointer_button_pressed: Some(true),
            pointer_x: Some(20.0),
            pointer_y: Some(30.0),
            target: Some("dev.test".to_owned()),
            target_source_path: None,
            visible_change: true,
        }));
        roundtrip(ObserverEvent::FramePresented(FramePresented {
            role: ObserverRole::Preview,
            key: key(10),
            event_sequence: Some(4),
            input_kind: Some(InputKind::Wheel),
            callback_to_host_ns: 123,
            input_to_present_us: 456,
            event_dispatch_us: 11,
            executor_us: 5,
            runtime_document_us: 6,
            document_update_us: 12,
            render_us: 7,
            document_scene_convert_us: 1,
            scene_key_us: 2,
            rect_vertices_us: 3,
            asset_prepare_us: 4,
            quad_batch_key_us: 5,
            quad_upload_us: 6,
            draw_pass_us: 7,
            retained_metrics_us: 8,
            text_render_us: 9,
            submit_us: 8,
            present_us: 9,
            frame_us: 10,
            observer_drop_count: 0,
        }));
        roundtrip(ObserverEvent::ProofCompleted {
            key: key(20),
            completed_after_frame_id: 22,
            elapsed_us: 1_234,
            replaced_count: 2,
            result_drop_count: 0,
            artifact: Some(ProofArtifact {
                path: "target/proof.png".to_owned(),
                sha256: "a".repeat(64),
                byte_len: 42,
                capture_method: "app-owned-wgpu".to_owned(),
                nonblank_samples: 10,
                unique_rgba_values: 3,
            }),
            error: None,
        });
        roundtrip(ObserverEvent::SourceFailed {
            revision: 9,
            stage: "runtime-mount".to_owned(),
            message: "invalid retained document".to_owned(),
        });
    }

    #[test]
    fn evidence_key_rejects_zero_identity_components() {
        assert!(key(1).is_complete());
        assert!(!key(0).is_complete());
    }

    #[test]
    fn decoder_rejects_unbounded_and_trailing_frames() {
        let mut oversized = (MAX_EVENT_BYTES as u32 + 1).to_le_bytes().to_vec();
        oversized.resize(8, 0);
        assert!(matches!(
            read_event(&mut oversized.as_slice()),
            Err(ObserverError::FrameTooLarge(_))
        ));

        let event = ObserverEvent::ProofRequested {
            key: key(3),
            snapshot_prepare_us: 42,
        };
        let mut bytes = Vec::new();
        write_event(&mut bytes, &event).unwrap();
        let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        bytes.extend_from_slice(&[9]);
        bytes[..4].copy_from_slice(&((length + 1) as u32).to_le_bytes());
        assert!(matches!(
            read_event(&mut bytes.as_slice()),
            Err(ObserverError::TrailingBytes(1))
        ));
    }
}
