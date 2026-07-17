//! Immutable client/session/server application packages for Boon production hosts.
//!
//! Package construction is available behind the `build` feature. Runtime
//! consumers only parse a closed manifest, verify every content digest, and
//! decode precompiled program artifacts.

mod browser;
mod bundle;
mod manifest;

#[cfg(feature = "build")]
mod build;

pub use browser::*;
pub use bundle::*;
pub use manifest::*;

#[cfg(feature = "build")]
pub use build::*;

use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub const APP_MANIFEST_FORMAT: u32 = 1;
pub const BUNDLE_FORMAT: u32 = 1;
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PACKAGE_FILE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_PACKAGE_FILES: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageError(String);

impl PackageError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn context(context: &str, error: impl Display) -> Self {
        Self(format!("{context}: {error}"))
    }
}

impl Display for PackageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for PackageError {}

impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<toml::de::Error> for PackageError {
    fn from(error: toml::de::Error) -> Self {
        Self(error.to_string())
    }
}
