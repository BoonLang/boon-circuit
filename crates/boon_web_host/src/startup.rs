use crate::{WebHostError, WebHostResult};
use boon_app_package::BrowserAppConfig;
use boon_runtime::ProgramSession;

/// A verified public Client artifact with its initial Boon session mounted.
/// Browser platform adapters attach input, persistence, layout, and rendering
/// to this single session rather than decoding package metadata independently.
pub struct BrowserAppStartup {
    config: BrowserAppConfig,
    session: ProgramSession,
}

impl BrowserAppStartup {
    pub fn from_artifact_bytes(
        config: BrowserAppConfig,
        artifact_bytes: Vec<u8>,
    ) -> WebHostResult<Self> {
        let artifact = config
            .decode_client_artifact(artifact_bytes)
            .map_err(package_input_error)?;
        let session =
            ProgramSession::start(artifact).map_err(|error| WebHostError::InvalidInput {
                field: "browser client artifact".to_owned(),
                reason: error.to_string(),
            })?;
        Ok(Self { config, session })
    }

    pub fn config(&self) -> &BrowserAppConfig {
        &self.config
    }

    pub fn session(&self) -> &ProgramSession {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut ProgramSession {
        &mut self.session
    }
}

pub fn decode_browser_app_config(bytes: &[u8]) -> WebHostResult<BrowserAppConfig> {
    BrowserAppConfig::decode(bytes).map_err(package_input_error)
}

fn package_input_error(error: boon_app_package::PackageError) -> WebHostError {
    WebHostError::InvalidInput {
        field: "browser app package".to_owned(),
        reason: error.to_string(),
    }
}
