use super::{MachinePlan, OutputContractKind, OutputRootId, ProgramRole, SourceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostPortPlan {
    HttpServer {
        request_source: SourceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disconnect_source: Option<SourceId>,
        response_output: OutputRootId,
    },
    WebSocketServer {
        open_source: SourceId,
        message_source: SourceId,
        close_source: SourceId,
        error_source: SourceId,
        actions_output: OutputRootId,
    },
}

impl HostPortPlan {
    pub fn source_ids(&self) -> impl Iterator<Item = SourceId> + '_ {
        let mut ids = Vec::with_capacity(4);
        match self {
            Self::HttpServer {
                request_source,
                disconnect_source,
                ..
            } => {
                ids.push(*request_source);
                ids.extend(*disconnect_source);
            }
            Self::WebSocketServer {
                open_source,
                message_source,
                close_source,
                error_source,
                ..
            } => ids.extend([*open_source, *message_source, *close_source, *error_source]),
        }
        ids.into_iter()
    }

    pub fn output_id(&self) -> OutputRootId {
        match self {
            Self::HttpServer {
                response_output, ..
            } => *response_output,
            Self::WebSocketServer { actions_output, .. } => *actions_output,
        }
    }
}

pub(crate) fn host_ports_failure(plan: &MachinePlan) -> Option<String> {
    if plan.host_ports.is_empty() {
        return None;
    }
    if plan.program_role != ProgramRole::Server {
        return Some("host ports require a server program role".to_owned());
    }

    let source_ids = plan
        .source_routes
        .iter()
        .map(|route| route.source_id)
        .collect::<BTreeSet<_>>();
    let mut kinds = BTreeSet::new();
    for port in &plan.host_ports {
        let kind = match port {
            HostPortPlan::HttpServer { .. } => "http_server",
            HostPortPlan::WebSocketServer { .. } => "websocket_server",
        };
        if !kinds.insert(kind) {
            return Some(format!(
                "host port kind `{kind}` is declared more than once"
            ));
        }
        if let Some(source_id) = port.source_ids().find(|id| !source_ids.contains(id)) {
            return Some(format!(
                "host port `{kind}` references missing source ID {}",
                source_id.0
            ));
        }
        let Some(output) = plan
            .outputs
            .iter()
            .find(|output| output.id == port.output_id())
        else {
            return Some(format!(
                "host port `{kind}` references a missing output root"
            ));
        };
        if !matches!(output.contract, OutputContractKind::HostValue { .. }) {
            return Some(format!(
                "host port `{kind}` output `{}` is not a host value",
                output.name
            ));
        }
    }
    None
}
