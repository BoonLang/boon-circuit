use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::sensitive_input::SensitiveInputError;

#[derive(Debug)]
pub enum NativeHostError {
    InvalidConfig(&'static str),
    HostAlreadyOpen,
    WindowEventsAlreadyTaken,
    EventQueueOverflow,
    EventSourceClosed,
    MissingPointerPosition(&'static str),
    PointerButtonOutOfRange(u32),
    InvalidNumber {
        field: &'static str,
        value: f64,
    },
    CounterOverflow(&'static str),
    WrongRenderThread {
        operation: &'static str,
    },
    WrongWgpuThread {
        operation: &'static str,
        requirement: &'static str,
    },
    UnsupportedWgpuStrategy {
        operation: &'static str,
    },
    CreateSurface(String),
    RequestAdapter(String),
    SurfaceUnsupported,
    SurfaceCapabilitiesChanged,
    SensitiveInput(SensitiveInputError),
    UnknownWindowEvent,
}

impl Display for NativeHostError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(field) => {
                write!(formatter, "invalid native window config: {field}")
            }
            Self::HostAlreadyOpen => {
                formatter.write_str("a native surface host is already open in this process")
            }
            Self::WindowEventsAlreadyTaken => {
                formatter.write_str("app_window surface events were already taken")
            }
            Self::EventQueueOverflow => {
                formatter.write_str("app_window event queue overflowed; input order is lost")
            }
            Self::EventSourceClosed => formatter.write_str("app_window event source closed"),
            Self::MissingPointerPosition(kind) => {
                write!(formatter, "{kind} arrived before a pointer position")
            }
            Self::PointerButtonOutOfRange(button) => {
                write!(formatter, "pointer button {button} does not fit boon_host")
            }
            Self::InvalidNumber { field, value } => {
                write!(formatter, "invalid {field} value {value}")
            }
            Self::CounterOverflow(counter) => write!(formatter, "{counter} counter overflowed"),
            Self::WrongRenderThread { operation } => {
                write!(
                    formatter,
                    "{operation} ran outside the surface render thread"
                )
            }
            Self::WrongWgpuThread {
                operation,
                requirement,
            } => write!(formatter, "{operation} requires {requirement}"),
            Self::UnsupportedWgpuStrategy { operation } => {
                write!(
                    formatter,
                    "unsupported app_window WGPU strategy for {operation}"
                )
            }
            Self::CreateSurface(error) => {
                write!(formatter, "failed to create WGPU surface: {error}")
            }
            Self::RequestAdapter(error) => {
                write!(formatter, "failed to request WGPU adapter: {error}")
            }
            Self::SurfaceUnsupported => {
                formatter.write_str("adapter has no compatible surface configuration")
            }
            Self::SurfaceCapabilitiesChanged => formatter.write_str(
                "recreated surface no longer supports the configured format, mode, alpha, or usage",
            ),
            Self::SensitiveInput(error) => Display::fmt(error, formatter),
            Self::UnknownWindowEvent => {
                formatter.write_str("app_window produced an unknown window event")
            }
        }
    }
}

impl Error for NativeHostError {}

impl From<SensitiveInputError> for NativeHostError {
    fn from(error: SensitiveInputError) -> Self {
        Self::SensitiveInput(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceReconfigureReason {
    Outdated,
    Lost,
    Suboptimal,
}

#[derive(Debug)]
pub enum SurfaceAcquireError {
    Host(NativeHostError),
    Unconfigured,
    Suspended,
    Closing,
    FrameInFlight,
    Timeout,
    Occluded,
    Validation,
    Reconfigured {
        reason: SurfaceReconfigureReason,
        epoch: u64,
    },
}

impl Display for SurfaceAcquireError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => Display::fmt(error, formatter),
            Self::Unconfigured => formatter.write_str("surface is not configured"),
            Self::Suspended => formatter.write_str("zero-size surface is suspended"),
            Self::Closing => formatter.write_str("surface is closing"),
            Self::FrameInFlight => formatter.write_str("a surface frame is already in flight"),
            Self::Timeout => formatter.write_str("surface acquisition timed out"),
            Self::Occluded => formatter.write_str("surface is occluded"),
            Self::Validation => formatter.write_str("surface acquisition failed validation"),
            Self::Reconfigured { reason, epoch } => {
                write!(
                    formatter,
                    "surface was reconfigured after {reason:?}; epoch is {epoch}"
                )
            }
        }
    }
}

impl Error for SurfaceAcquireError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeHostError> for SurfaceAcquireError {
    fn from(error: NativeHostError) -> Self {
        Self::Host(error)
    }
}

#[derive(Debug)]
pub enum SurfacePresentError {
    Host(NativeHostError),
    Closing,
    StaleFrame {
        frame_epoch: u64,
        surface_epoch: u64,
    },
}

impl Display for SurfacePresentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => Display::fmt(error, formatter),
            Self::Closing => formatter.write_str("surface is closing"),
            Self::StaleFrame {
                frame_epoch,
                surface_epoch,
            } => write!(
                formatter,
                "frame epoch {frame_epoch} does not match surface epoch {surface_epoch}"
            ),
        }
    }
}

impl Error for SurfacePresentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NativeHostError> for SurfacePresentError {
    fn from(error: NativeHostError) -> Self {
        Self::Host(error)
    }
}
