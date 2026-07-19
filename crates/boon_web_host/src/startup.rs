use crate::{DistributedSessionIdentity, WebHostError, WebHostResult};
use boon_app_package::BrowserAppConfig;
use boon_plan::EffectContract;
use boon_runtime::{ClientSessionQueueLimits, DistributedClientRuntime};

/// A verified public Client artifact mounted into the distributed Client
/// endpoint. The browser adapter must move these parts into the resumable
/// Client/Session socket owner; there is no local-session product fallback.
pub struct BrowserAppStartup {
    config: BrowserAppConfig,
    identity: DistributedSessionIdentity,
    runtime: DistributedClientRuntime,
    effect_contracts: Vec<EffectContract>,
}

impl BrowserAppStartup {
    pub fn from_artifact_bytes(
        config: BrowserAppConfig,
        artifact_bytes: Vec<u8>,
    ) -> WebHostResult<Self> {
        let artifact = config
            .decode_client_artifact(artifact_bytes)
            .map_err(package_input_error)?;
        let endpoint = artifact
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or_else(|| WebHostError::InvalidInput {
                field: "browser client artifact".to_owned(),
                reason: "Client artifact has no distributed graph endpoint".to_owned(),
            })?;
        let identity = DistributedSessionIdentity::new(
            &config.package_id,
            *endpoint.graph.graph_id.as_bytes(),
            endpoint.graph.revision,
            endpoint.wire_schema_hash,
        )
        .map_err(|error| WebHostError::InvalidInput {
            field: "browser distributed Session identity".to_owned(),
            reason: error.to_string(),
        })?;
        let effect_contracts = artifact.plan().effects.clone();
        let runtime =
            DistributedClientRuntime::start(&artifact, ClientSessionQueueLimits::default())
                .map_err(|error| WebHostError::InvalidInput {
                    field: "browser client artifact".to_owned(),
                    reason: error.to_string(),
                })?;
        Ok(Self {
            config,
            identity,
            runtime,
            effect_contracts,
        })
    }

    pub fn config(&self) -> &BrowserAppConfig {
        &self.config
    }

    pub fn runtime(&self) -> &DistributedClientRuntime {
        &self.runtime
    }

    pub fn into_distributed_parts(
        self,
    ) -> (
        BrowserAppConfig,
        DistributedSessionIdentity,
        DistributedClientRuntime,
        Vec<EffectContract>,
    ) {
        (
            self.config,
            self.identity,
            self.runtime,
            self.effect_contracts,
        )
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
