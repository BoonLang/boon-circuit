use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

const MAGIC: [u8; 4] = *b"BNIP";
const VERSION: u16 = 5;
const HEADER_BYTES: usize = MAGIC.len() + 2 + 1;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;
const MAX_SOURCE_UNITS: usize = 1_024;
const MAX_CATALOG_ENTRIES: usize = 1_024;
const MAX_TEST_STEPS: usize = 4_096;
const MAX_ASSET_BLOBS: usize = 1_024;
const MAX_ASSET_BLOB_BYTES: usize = 8 * 1024 * 1024;
pub const VERIFY_BOUNDED_WINDOWS_ENV: &str = "BOON_VERIFY_BOUNDED_WINDOWS";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Role {
    Preview = 1,
    Dev = 2,
}

impl Role {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Preview),
            2 => Ok(Self::Dev),
            _ => Err(ProtocolError::InvalidEnum("role", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetBlob {
    pub url: String,
    pub media_type: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogItem {
    pub id: String,
    pub label: String,
    pub custom: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestStep {
    pub source_path: String,
    pub action_kind: Option<String>,
    pub target_text: Option<String>,
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub target_occurrence: Option<u64>,
    pub pointer_x: Option<String>,
    pub pointer_y: Option<String>,
    pub pointer_width: Option<String>,
    pub pointer_height: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PreviewIntent {
    Replace = 1,
    Run = 2,
    Reset = 3,
    Test = 4,
}

impl PreviewIntent {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Replace),
            2 => Ok(Self::Run),
            3 => Ok(Self::Reset),
            4 => Ok(Self::Test),
            _ => Err(ProtocolError::InvalidEnum("preview intent", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum FrameMode {
    Idle = 1,
    Burst = 2,
    Probe = 3,
}

impl FrameMode {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Idle),
            2 => Ok(Self::Burst),
            3 => Ok(Self::Probe),
            _ => Err(ProtocolError::InvalidEnum("frame mode", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProofMode {
    Off = 1,
    Trace = 2,
    Readback = 3,
}

impl ProofMode {
    fn decode(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Off),
            2 => Ok(Self::Trace),
            3 => Ok(Self::Readback),
            _ => Err(ProtocolError::InvalidEnum("proof mode", value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewStats {
    pub frame_seq: u64,
    pub source_revision: u64,
    pub frame_mode: FrameMode,
    pub proof_mode: ProofMode,
    pub frames_per_second_milli: u32,
    pub input_to_present_micros: u32,
    pub render_micros: u32,
    pub present_micros: u32,
    pub missed_frames: u64,
    pub dropped_snapshots: u64,
    pub sample_age_millis: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    Hello {
        role: Role,
        pid: u32,
    },
    Ready {
        role: Role,
    },
    Catalog {
        entries: Vec<CatalogItem>,
        active_id: String,
    },
    OpenEditor {
        example_id: String,
        label: String,
        revision: u64,
        units: Vec<SourceUnit>,
    },
    DevSelectExample {
        example_id: String,
    },
    DevSourceChanged {
        revision: u64,
        units: Vec<SourceUnit>,
    },
    DevRun {
        revision: u64,
        units: Vec<SourceUnit>,
    },
    DevReset,
    DevTest {
        request_id: u64,
        revision: u64,
        units: Vec<SourceUnit>,
    },
    PreviewApply {
        intent: PreviewIntent,
        request_id: Option<u64>,
        revision: u64,
        units: Vec<SourceUnit>,
        test_steps: Vec<TestStep>,
    },
    PreviewAssets {
        assets: Vec<AssetBlob>,
    },
    PreviewStats(PreviewStats),
    PreviewStatus {
        revision: u64,
        ok: bool,
        message: String,
    },
    PreviewRuntimeChanged {
        revision: u64,
        runtime_sequence: u64,
    },
    PreviewTestResult {
        request_id: u64,
        passed: bool,
        message: String,
    },
    DevInspect {
        request_id: u64,
        revision: u64,
        path: String,
    },
    PreviewInspect {
        request_id: u64,
        revision: u64,
        path: String,
    },
    PreviewInspectResult {
        request_id: u64,
        revision: u64,
        runtime_sequence: u64,
        path: String,
        ok: bool,
        value: String,
    },
    Shutdown,
}

impl Message {
    fn tag(&self) -> u8 {
        match self {
            Self::Hello { .. } => 1,
            Self::Ready { .. } => 2,
            Self::Catalog { .. } => 3,
            Self::OpenEditor { .. } => 4,
            Self::DevSelectExample { .. } => 5,
            Self::DevSourceChanged { .. } => 6,
            Self::DevRun { .. } => 7,
            Self::DevReset => 8,
            Self::DevTest { .. } => 9,
            Self::PreviewApply { .. } => 10,
            Self::PreviewStats(_) => 11,
            Self::PreviewStatus { .. } => 12,
            Self::PreviewTestResult { .. } => 13,
            Self::Shutdown => 14,
            Self::DevInspect { .. } => 15,
            Self::PreviewInspect { .. } => 16,
            Self::PreviewInspectResult { .. } => 17,
            Self::PreviewRuntimeChanged { .. } => 18,
            Self::PreviewAssets { .. } => 19,
        }
    }

    fn encode_payload(&self, out: &mut Encoder) -> Result<(), ProtocolError> {
        match self {
            Self::Hello { role, pid } => {
                out.u8(*role as u8);
                out.u32(*pid);
            }
            Self::Ready { role } => out.u8(*role as u8),
            Self::Catalog { entries, active_id } => {
                out.catalog(entries)?;
                out.string(active_id)?;
            }
            Self::OpenEditor {
                example_id,
                label,
                revision,
                units,
            } => {
                out.string(example_id)?;
                out.string(label)?;
                out.u64(*revision);
                out.source_units(units)?;
            }
            Self::DevSelectExample { example_id } => out.string(example_id)?,
            Self::DevSourceChanged { revision, units } | Self::DevRun { revision, units } => {
                out.u64(*revision);
                out.source_units(units)?;
            }
            Self::DevReset => {}
            Self::DevTest {
                request_id,
                revision,
                units,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.source_units(units)?;
            }
            Self::PreviewApply {
                intent,
                request_id,
                revision,
                units,
                test_steps,
            } => {
                out.u8(*intent as u8);
                out.optional_u64(*request_id);
                out.u64(*revision);
                out.source_units(units)?;
                out.test_steps(test_steps)?;
            }
            Self::PreviewAssets { assets } => out.asset_blobs(assets)?,
            Self::PreviewStats(stats) => {
                out.u64(stats.frame_seq);
                out.u64(stats.source_revision);
                out.u8(stats.frame_mode as u8);
                out.u8(stats.proof_mode as u8);
                out.u32(stats.frames_per_second_milli);
                out.u32(stats.input_to_present_micros);
                out.u32(stats.render_micros);
                out.u32(stats.present_micros);
                out.u64(stats.missed_frames);
                out.u64(stats.dropped_snapshots);
                out.u32(stats.sample_age_millis);
            }
            Self::PreviewStatus {
                revision,
                ok,
                message,
            } => {
                out.u64(*revision);
                out.bool(*ok);
                out.string(message)?;
            }
            Self::PreviewRuntimeChanged {
                revision,
                runtime_sequence,
            } => {
                out.u64(*revision);
                out.u64(*runtime_sequence);
            }
            Self::PreviewTestResult {
                request_id,
                passed,
                message,
            } => {
                out.u64(*request_id);
                out.bool(*passed);
                out.string(message)?;
            }
            Self::DevInspect {
                request_id,
                revision,
                path,
            }
            | Self::PreviewInspect {
                request_id,
                revision,
                path,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.string(path)?;
            }
            Self::PreviewInspectResult {
                request_id,
                revision,
                runtime_sequence,
                path,
                ok,
                value,
            } => {
                out.u64(*request_id);
                out.u64(*revision);
                out.u64(*runtime_sequence);
                out.string(path)?;
                out.bool(*ok);
                out.string(value)?;
            }
            Self::Shutdown => {}
        }
        Ok(())
    }

    fn decode(tag: u8, input: &mut Decoder<'_>) -> Result<Self, ProtocolError> {
        let message = match tag {
            1 => Self::Hello {
                role: Role::decode(input.u8()?)?,
                pid: input.u32()?,
            },
            2 => Self::Ready {
                role: Role::decode(input.u8()?)?,
            },
            3 => Self::Catalog {
                entries: input.catalog()?,
                active_id: input.string()?,
            },
            4 => Self::OpenEditor {
                example_id: input.string()?,
                label: input.string()?,
                revision: input.u64()?,
                units: input.source_units()?,
            },
            5 => Self::DevSelectExample {
                example_id: input.string()?,
            },
            6 => Self::DevSourceChanged {
                revision: input.u64()?,
                units: input.source_units()?,
            },
            7 => Self::DevRun {
                revision: input.u64()?,
                units: input.source_units()?,
            },
            8 => Self::DevReset,
            9 => Self::DevTest {
                request_id: input.u64()?,
                revision: input.u64()?,
                units: input.source_units()?,
            },
            10 => Self::PreviewApply {
                intent: PreviewIntent::decode(input.u8()?)?,
                request_id: input.optional_u64()?,
                revision: input.u64()?,
                units: input.source_units()?,
                test_steps: input.test_steps()?,
            },
            11 => Self::PreviewStats(PreviewStats {
                frame_seq: input.u64()?,
                source_revision: input.u64()?,
                frame_mode: FrameMode::decode(input.u8()?)?,
                proof_mode: ProofMode::decode(input.u8()?)?,
                frames_per_second_milli: input.u32()?,
                input_to_present_micros: input.u32()?,
                render_micros: input.u32()?,
                present_micros: input.u32()?,
                missed_frames: input.u64()?,
                dropped_snapshots: input.u64()?,
                sample_age_millis: input.u32()?,
            }),
            12 => Self::PreviewStatus {
                revision: input.u64()?,
                ok: input.bool()?,
                message: input.string()?,
            },
            13 => Self::PreviewTestResult {
                request_id: input.u64()?,
                passed: input.bool()?,
                message: input.string()?,
            },
            14 => Self::Shutdown,
            15 => Self::DevInspect {
                request_id: input.u64()?,
                revision: input.u64()?,
                path: input.string()?,
            },
            16 => Self::PreviewInspect {
                request_id: input.u64()?,
                revision: input.u64()?,
                path: input.string()?,
            },
            17 => Self::PreviewInspectResult {
                request_id: input.u64()?,
                revision: input.u64()?,
                runtime_sequence: input.u64()?,
                path: input.string()?,
                ok: input.bool()?,
                value: input.string()?,
            },
            18 => Self::PreviewRuntimeChanged {
                revision: input.u64()?,
                runtime_sequence: input.u64()?,
            },
            19 => Self::PreviewAssets {
                assets: input.asset_blobs()?,
            },
            _ => return Err(ProtocolError::UnknownMessage(tag)),
        };
        input.finish()?;
        Ok(message)
    }
}

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    FrameTooLarge(usize),
    InvalidMagic,
    UnsupportedVersion(u16),
    UnknownMessage(u8),
    InvalidEnum(&'static str, u8),
    InvalidBool(u8),
    InvalidOption(u8),
    InvalidUtf8(std::str::Utf8Error),
    LimitExceeded(&'static str, usize),
    Truncated,
    TrailingBytes(usize),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "IPC I/O failed: {error}"),
            Self::FrameTooLarge(bytes) => write!(f, "IPC frame is too large: {bytes} bytes"),
            Self::InvalidMagic => f.write_str("IPC frame has invalid magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "IPC protocol version {version} is unsupported")
            }
            Self::UnknownMessage(tag) => write!(f, "IPC message tag {tag} is unknown"),
            Self::InvalidEnum(name, value) => write!(f, "IPC {name} value {value} is invalid"),
            Self::InvalidBool(value) => write!(f, "IPC bool value {value} is invalid"),
            Self::InvalidOption(value) => write!(f, "IPC option value {value} is invalid"),
            Self::InvalidUtf8(error) => write!(f, "IPC string is not UTF-8: {error}"),
            Self::LimitExceeded(name, value) => {
                write!(f, "IPC {name} exceeds its limit: {value}")
            }
            Self::Truncated => f.write_str("IPC frame is truncated"),
            Self::TrailingBytes(bytes) => write!(f, "IPC frame has {bytes} trailing bytes"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidUtf8(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    pub fn new(stream: UnixStream) -> Self {
        Self { stream }
    }

    pub fn connect(path: &Path, role: Role) -> Result<Self, ProtocolError> {
        let mut connection = Self::new(UnixStream::connect(path)?);
        connection.send(&Message::Hello {
            role,
            pid: std::process::id(),
        })?;
        Ok(connection)
    }

    pub fn try_clone(&self) -> Result<Self, ProtocolError> {
        Ok(Self::new(self.stream.try_clone()?))
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> Result<(), ProtocolError> {
        self.stream.set_read_timeout(timeout)?;
        Ok(())
    }

    pub fn send(&mut self, message: &Message) -> Result<(), ProtocolError> {
        write_message(&mut self.stream, message)
    }

    pub fn receive(&mut self) -> Result<Option<Message>, ProtocolError> {
        read_message(&mut self.stream)
    }
}

pub fn write_message(writer: &mut impl Write, message: &Message) -> Result<(), ProtocolError> {
    let mut body = Encoder::default();
    body.bytes.extend_from_slice(&MAGIC);
    body.u16(VERSION);
    body.u8(message.tag());
    message.encode_payload(&mut body)?;
    if body.bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(body.bytes.len()));
    }
    writer.write_all(&(body.bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&body.bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_message(reader: &mut impl Read) -> Result<Option<Message>, ProtocolError> {
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length[..1]) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if !(HEADER_BYTES..=MAX_FRAME_BYTES).contains(&length) {
        return Err(ProtocolError::FrameTooLarge(length));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    if body[..MAGIC.len()] != MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    let version = u16::from_le_bytes([body[4], body[5]]);
    if version != VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }
    let tag = body[6];
    let mut input = Decoder::new(&body[HEADER_BYTES..]);
    Message::decode(tag, &mut input).map(Some)
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

    fn optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.u8(1);
                self.u64(value);
            }
            None => self.u8(0),
        }
    }

    fn string(&mut self, value: &str) -> Result<(), ProtocolError> {
        if value.len() > MAX_STRING_BYTES {
            return Err(ProtocolError::LimitExceeded("string bytes", value.len()));
        }
        let projected = self
            .bytes
            .len()
            .checked_add(4)
            .and_then(|length| length.checked_add(value.len()))
            .ok_or(ProtocolError::FrameTooLarge(usize::MAX))?;
        if projected > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge(projected));
        }
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn source_units(&mut self, units: &[SourceUnit]) -> Result<(), ProtocolError> {
        if units.len() > MAX_SOURCE_UNITS {
            return Err(ProtocolError::LimitExceeded(
                "source unit count",
                units.len(),
            ));
        }
        self.u32(units.len() as u32);
        for unit in units {
            self.string(&unit.path)?;
            self.string(&unit.source)?;
        }
        Ok(())
    }

    fn asset_blobs(&mut self, assets: &[AssetBlob]) -> Result<(), ProtocolError> {
        if assets.len() > MAX_ASSET_BLOBS {
            return Err(ProtocolError::LimitExceeded("asset count", assets.len()));
        }
        self.u32(assets.len() as u32);
        for asset in assets {
            self.string(&asset.url)?;
            self.string(&asset.media_type)?;
            self.string(&asset.sha256)?;
            if asset.bytes.len() > MAX_ASSET_BLOB_BYTES {
                return Err(ProtocolError::LimitExceeded(
                    "asset blob bytes",
                    asset.bytes.len(),
                ));
            }
            self.u32(asset.bytes.len() as u32);
            self.bytes.extend_from_slice(&asset.bytes);
        }
        Ok(())
    }

    fn catalog(&mut self, entries: &[CatalogItem]) -> Result<(), ProtocolError> {
        if entries.len() > MAX_CATALOG_ENTRIES {
            return Err(ProtocolError::LimitExceeded(
                "catalog entry count",
                entries.len(),
            ));
        }
        self.u32(entries.len() as u32);
        for entry in entries {
            self.string(&entry.id)?;
            self.string(&entry.label)?;
            self.bool(entry.custom);
        }
        Ok(())
    }

    fn test_steps(&mut self, steps: &[TestStep]) -> Result<(), ProtocolError> {
        if steps.len() > MAX_TEST_STEPS {
            return Err(ProtocolError::LimitExceeded("test step count", steps.len()));
        }
        self.u32(steps.len() as u32);
        for step in steps {
            self.string(&step.source_path)?;
            self.optional_string(step.action_kind.as_deref())?;
            self.optional_string(step.target_text.as_deref())?;
            self.optional_string(step.text.as_deref())?;
            self.optional_string(step.key.as_deref())?;
            self.optional_string(step.address.as_deref())?;
            self.optional_u64(step.target_occurrence);
            self.optional_string(step.pointer_x.as_deref())?;
            self.optional_string(step.pointer_y.as_deref())?;
            self.optional_string(step.pointer_width.as_deref())?;
            self.optional_string(step.pointer_height.as_deref())?;
        }
        Ok(())
    }

    fn optional_string(&mut self, value: Option<&str>) -> Result<(), ProtocolError> {
        match value {
            Some(value) => {
                self.u8(1);
                self.string(value)?;
            }
            None => self.u8(0),
        }
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

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(ProtocolError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(ProtocolError::InvalidBool(value)),
        }
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.u64().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn string(&mut self) -> Result<String, ProtocolError> {
        let length = self.u32()? as usize;
        if length > MAX_STRING_BYTES {
            return Err(ProtocolError::LimitExceeded("string bytes", length));
        }
        let value = std::str::from_utf8(self.take(length)?).map_err(ProtocolError::InvalidUtf8)?;
        Ok(value.to_owned())
    }

    fn source_units(&mut self) -> Result<Vec<SourceUnit>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_SOURCE_UNITS {
            return Err(ProtocolError::LimitExceeded("source unit count", count));
        }
        (0..count)
            .map(|_| {
                Ok(SourceUnit {
                    path: self.string()?,
                    source: self.string()?,
                })
            })
            .collect()
    }

    fn asset_blobs(&mut self) -> Result<Vec<AssetBlob>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_ASSET_BLOBS {
            return Err(ProtocolError::LimitExceeded("asset count", count));
        }
        (0..count)
            .map(|_| {
                let url = self.string()?;
                let media_type = self.string()?;
                let sha256 = self.string()?;
                let length = self.u32()? as usize;
                if length > MAX_ASSET_BLOB_BYTES {
                    return Err(ProtocolError::LimitExceeded("asset blob bytes", length));
                }
                Ok(AssetBlob {
                    url,
                    media_type,
                    sha256,
                    bytes: self.take(length)?.to_vec(),
                })
            })
            .collect()
    }

    fn catalog(&mut self) -> Result<Vec<CatalogItem>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_CATALOG_ENTRIES {
            return Err(ProtocolError::LimitExceeded("catalog entry count", count));
        }
        (0..count)
            .map(|_| {
                Ok(CatalogItem {
                    id: self.string()?,
                    label: self.string()?,
                    custom: self.bool()?,
                })
            })
            .collect()
    }

    fn test_steps(&mut self) -> Result<Vec<TestStep>, ProtocolError> {
        let count = self.u32()? as usize;
        if count > MAX_TEST_STEPS {
            return Err(ProtocolError::LimitExceeded("test step count", count));
        }
        (0..count)
            .map(|_| {
                Ok(TestStep {
                    source_path: self.string()?,
                    action_kind: self.optional_string()?,
                    target_text: self.optional_string()?,
                    text: self.optional_string()?,
                    key: self.optional_string()?,
                    address: self.optional_string()?,
                    target_occurrence: self.optional_u64()?,
                    pointer_x: self.optional_string()?,
                    pointer_y: self.optional_string()?,
                    pointer_width: self.optional_string()?,
                    pointer_height: self.optional_string()?,
                })
            })
            .collect()
    }

    fn optional_string(&mut self) -> Result<Option<String>, ProtocolError> {
        match self.u8()? {
            0 => Ok(None),
            1 => self.string().map(Some),
            value => Err(ProtocolError::InvalidOption(value)),
        }
    }

    fn finish(&self) -> Result<(), ProtocolError> {
        let remaining = self.bytes.len() - self.offset;
        if remaining == 0 {
            Ok(())
        } else {
            Err(ProtocolError::TrailingBytes(remaining))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units() -> Vec<SourceUnit> {
        vec![
            SourceUnit {
                path: "examples/main.bn".to_owned(),
                source: "value: 42\n".to_owned(),
            },
            SourceUnit {
                path: "examples/view.bn".to_owned(),
                source: "view: Text[text: value]\n".to_owned(),
            },
        ]
    }

    fn roundtrip(message: Message) {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).expect("encode message");
        let decoded = read_message(&mut bytes.as_slice())
            .expect("decode message")
            .expect("message");
        assert_eq!(decoded, message);
    }

    #[test]
    fn roundtrips_control_and_source_messages() {
        let messages = [
            Message::Hello {
                role: Role::Preview,
                pid: 81,
            },
            Message::Catalog {
                entries: vec![CatalogItem {
                    id: "counter".to_owned(),
                    label: "Counter".to_owned(),
                    custom: false,
                }],
                active_id: "counter".to_owned(),
            },
            Message::OpenEditor {
                example_id: "counter".to_owned(),
                label: "Counter".to_owned(),
                revision: 7,
                units: units(),
            },
            Message::DevInspect {
                request_id: 9,
                revision: 7,
                path: "store.count".to_owned(),
            },
            Message::PreviewInspect {
                request_id: 9,
                revision: 7,
                path: "store.count".to_owned(),
            },
            Message::PreviewInspectResult {
                request_id: 9,
                revision: 7,
                runtime_sequence: 3,
                path: "store.count".to_owned(),
                ok: true,
                value: "3".to_owned(),
            },
            Message::DevSourceChanged {
                revision: 8,
                units: units(),
            },
            Message::DevTest {
                request_id: 91,
                revision: 9,
                units: units(),
            },
            Message::PreviewApply {
                intent: PreviewIntent::Test,
                request_id: Some(91),
                revision: 9,
                units: units(),
                test_steps: vec![TestStep {
                    source_path: "store.increment.press".to_owned(),
                    action_kind: Some("click".to_owned()),
                    target_text: Some("+".to_owned()),
                    text: None,
                    key: None,
                    address: None,
                    target_occurrence: None,
                    pointer_x: Some("216".to_owned()),
                    pointer_y: Some("0".to_owned()),
                    pointer_width: Some("360".to_owned()),
                    pointer_height: Some("1".to_owned()),
                }],
            },
            Message::PreviewAssets {
                assets: vec![AssetBlob {
                    url: "asset://portfolio/hero.webp".to_owned(),
                    media_type: "image/webp".to_owned(),
                    sha256: "abc123".to_owned(),
                    bytes: vec![1, 2, 3, 4],
                }],
            },
            Message::Shutdown,
        ];
        for message in messages {
            roundtrip(message);
        }
    }

    #[test]
    fn roundtrips_preview_feedback() {
        roundtrip(Message::PreviewStats(PreviewStats {
            frame_seq: 144,
            source_revision: 19,
            frame_mode: FrameMode::Burst,
            proof_mode: ProofMode::Off,
            frames_per_second_milli: 59_940,
            input_to_present_micros: 8_311,
            render_micros: 1_203,
            present_micros: 5_022,
            missed_frames: 2,
            dropped_snapshots: 1,
            sample_age_millis: 4,
        }));
        roundtrip(Message::PreviewStatus {
            revision: 19,
            ok: false,
            message: "compile failed on line 3".to_owned(),
        });
        roundtrip(Message::PreviewRuntimeChanged {
            revision: 19,
            runtime_sequence: 8,
        });
        roundtrip(Message::PreviewTestResult {
            request_id: 4,
            passed: true,
            message: "counter scenario passed".to_owned(),
        });
    }

    #[test]
    fn kavik_asset_bundle_roundtrips_inside_the_bounded_preview_frame() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("kavik_cz")
            .unwrap();
        let message = Message::PreviewAssets {
            assets: example.assets,
        };
        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).expect("portfolio assets should fit one IPC frame");
        assert!(bytes.len() <= MAX_FRAME_BYTES + std::mem::size_of::<u32>());
        assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), Some(message));
    }

    #[test]
    fn stream_roundtrip_preserves_frame_boundaries() {
        let (left, right) = UnixStream::pair().expect("socket pair");
        let sender = std::thread::spawn(move || {
            let mut channel = Connection::new(left);
            channel
                .send(&Message::Ready { role: Role::Dev })
                .expect("send ready");
            channel.send(&Message::DevReset).expect("send reset");
        });
        let mut receiver = Connection::new(right);
        assert_eq!(
            receiver.receive().expect("receive ready"),
            Some(Message::Ready { role: Role::Dev })
        );
        assert_eq!(
            receiver.receive().expect("receive reset"),
            Some(Message::DevReset)
        );
        assert_eq!(receiver.receive().expect("receive eof"), None);
        sender.join().expect("sender thread");
    }

    #[test]
    fn rejects_trailing_payload_bytes() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &Message::DevReset).expect("encode message");
        let length = u32::from_le_bytes(bytes[..4].try_into().expect("length"));
        bytes[..4].copy_from_slice(&(length + 1).to_le_bytes());
        bytes.push(0xff);
        assert!(matches!(
            read_message(&mut bytes.as_slice()),
            Err(ProtocolError::TrailingBytes(1))
        ));
    }
}
