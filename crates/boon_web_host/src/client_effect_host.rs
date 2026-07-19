#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use crate::{WebHostError, WebHostResult};
use boon_app_package::CapabilityProfileDescriptor;
use boon_effect_schema::{
    SECURE_RANDOM_BYTES_OPERATION, TIMER_DEADLINE_OPERATION, WALL_CLOCK_READ_OPERATION,
};
use boon_plan::{
    EffectBarrier, EffectContract, EffectDeliveryCardinality, EffectId, EffectReplay,
    EffectResultPolicy, ProgramRole,
};
use boon_runtime::{
    RuntimeTurn, TransientEffectCallId, TransientEffectCreditGrant, TransientEffectInvocation,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const DEFAULT_BROWSER_ACTIVE_EFFECT_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserClientEffectKind {
    WallClock,
    SecureRandom,
    Deadline,
}

impl BrowserClientEffectKind {
    fn policy(operation: &str) -> Option<(Self, &'static str)> {
        match operation {
            WALL_CLOCK_READ_OPERATION => Some((Self::WallClock, "host.clock")),
            SECURE_RANDOM_BYTES_OPERATION => Some((Self::SecureRandom, "host.secure-random")),
            TIMER_DEADLINE_OPERATION => Some((Self::Deadline, "host.timers")),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum BrowserClientEffectCommand {
    Submit {
        kind: BrowserClientEffectKind,
        invocation: TransientEffectInvocation,
    },
    Cancel {
        kind: BrowserClientEffectKind,
        call_id: TransientEffectCallId,
    },
}

/// Exact-call ownership and capability policy for browser-owned Client effects.
///
/// Platform adapters execute the returned commands, but only this core may
/// admit, cancel, or complete a runtime call ID.
#[derive(Debug)]
pub(crate) struct BrowserClientEffectHostCore {
    authorized: BTreeMap<EffectId, BrowserClientEffectKind>,
    owners: BTreeMap<TransientEffectCallId, BrowserClientEffectKind>,
    max_active: usize,
}

impl BrowserClientEffectHostCore {
    pub(crate) fn new(
        profile: &CapabilityProfileDescriptor,
        contracts: &[EffectContract],
        max_active: usize,
    ) -> WebHostResult<Self> {
        if profile.role != ProgramRole::Client {
            return Err(invalid_policy(
                "browser effect profile belongs to a non-Client role",
            ));
        }
        if max_active == 0 {
            return Err(WebHostError::InvalidInput {
                field: "browser active effect limit".to_owned(),
                reason: "must be non-zero".to_owned(),
            });
        }

        let grants = profile
            .grants
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut authorized = BTreeMap::new();
        for contract in contracts {
            let (kind, grant) = BrowserClientEffectKind::policy(&contract.host_operation)
                .ok_or_else(|| WebHostError::Unsupported {
                    feature: format!("Client host effect `{}`", contract.host_operation),
                    reason: "the browser host has no generic platform adapter".to_owned(),
                })?;
            let canonical_id =
                EffectId::from_host_operation(&contract.host_operation).map_err(invalid_policy)?;
            if contract.effect_id != canonical_id
                || !matches!(
                    contract.replay,
                    EffectReplay::ReadOnly | EffectReplay::ProcessScoped
                )
                || contract.barrier != EffectBarrier::None
                || contract.result_policy != EffectResultPolicy::ReturnValue
                || contract.delivery != EffectDeliveryCardinality::Single
                || contract.schema.is_none()
            {
                return Err(invalid_policy(format!(
                    "Client host effect `{}` differs from its canonical transient contract",
                    contract.host_operation
                )));
            }
            if !grants.contains(grant) {
                return Err(WebHostError::CapabilityDenied {
                    capability: grant.to_owned(),
                    reason: format!(
                        "Client host effect `{}` is not granted by profile `{}`",
                        contract.host_operation, profile.id
                    ),
                });
            }
            if authorized.insert(contract.effect_id, kind).is_some() {
                return Err(invalid_policy(format!(
                    "Client effect plan repeats `{}`",
                    contract.host_operation
                )));
            }
        }

        Ok(Self {
            authorized,
            owners: BTreeMap::new(),
            max_active,
        })
    }

    pub(crate) fn route_turns(
        &mut self,
        turns: &[RuntimeTurn],
    ) -> WebHostResult<Vec<BrowserClientEffectCommand>> {
        let mut candidate = self.owners.clone();
        let mut commands = Vec::new();
        for turn in turns {
            Self::route_batch_into(
                &self.authorized,
                self.max_active,
                &mut candidate,
                &turn.cancelled_transient_effects,
                &turn.transient_effect_credit_grants,
                &turn.transient_effects,
                &mut commands,
            )?;
        }
        self.owners = candidate;
        Ok(commands)
    }

    fn route_batch_into(
        authorized: &BTreeMap<EffectId, BrowserClientEffectKind>,
        max_active: usize,
        owners: &mut BTreeMap<TransientEffectCallId, BrowserClientEffectKind>,
        cancelled: &[TransientEffectCallId],
        credits: &[TransientEffectCreditGrant],
        invocations: &[TransientEffectInvocation],
        commands: &mut Vec<BrowserClientEffectCommand>,
    ) -> WebHostResult<()> {
        for call_id in cancelled {
            if let Some(kind) = owners.remove(call_id) {
                commands.push(BrowserClientEffectCommand::Cancel {
                    kind,
                    call_id: *call_id,
                });
            }
        }
        if let Some(grant) = credits.first() {
            return Err(invalid_policy(format!(
                "single-result browser effect call {} received stream credit",
                grant.call_id
            )));
        }

        let mut batch = BTreeSet::new();
        for invocation in invocations {
            if owners.contains_key(&invocation.call_id) || !batch.insert(invocation.call_id) {
                return Err(invalid_policy(format!(
                    "browser effect host received duplicate active call {}",
                    invocation.call_id
                )));
            }
            if invocation.delivery != EffectDeliveryCardinality::Single {
                return Err(invalid_policy(format!(
                    "browser effect call {} requested unsupported stream delivery",
                    invocation.call_id
                )));
            }
            let kind = authorized
                .get(&invocation.effect_id)
                .copied()
                .ok_or_else(|| WebHostError::CapabilityDenied {
                    capability: invocation.effect_id.to_string(),
                    reason: "effect was not authorized during browser startup".to_owned(),
                })?;
            if owners.len() >= max_active {
                return Err(WebHostError::QueueOverflow {
                    queue: "browser active effects".to_owned(),
                    capacity: max_active,
                });
            }
            owners.insert(invocation.call_id, kind);
            commands.push(BrowserClientEffectCommand::Submit {
                kind,
                invocation: invocation.clone(),
            });
        }
        Ok(())
    }

    pub(crate) fn accept_completion(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> WebHostResult<BrowserClientEffectKind> {
        self.owners
            .remove(&call_id)
            .ok_or_else(|| WebHostError::InvalidInput {
                field: "browser effect completion".to_owned(),
                reason: format!("call {call_id} is stale or belongs to another host"),
            })
    }

    pub(crate) fn cancel_all(&mut self) -> Vec<BrowserClientEffectCommand> {
        let commands = self
            .owners
            .iter()
            .map(|(call_id, kind)| BrowserClientEffectCommand::Cancel {
                kind: *kind,
                call_id: *call_id,
            })
            .collect();
        self.owners.clear();
        commands
    }

    #[cfg(test)]
    pub(crate) fn pending_count(&self) -> usize {
        self.owners.len()
    }
}

fn invalid_policy(reason: impl ToString) -> WebHostError {
    WebHostError::InvalidInput {
        field: "browser Client effect policy".to_owned(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::{EffectInvocationId, builtin_effect_contract};
    use boon_runtime::{
        ApplicationIdentity, ClientSessionQueueLimits, DistributedClientRuntime,
        ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit, SourcePayload, Value,
        compile_distributed_program_bundle,
    };
    use boon_wire::SessionId;
    use std::collections::BTreeMap;

    fn profile(grants: &[&str]) -> CapabilityProfileDescriptor {
        let mut grants = grants
            .iter()
            .map(|grant| (*grant).to_owned())
            .collect::<Vec<_>>();
        grants.sort();
        CapabilityProfileDescriptor {
            id: "browser-test-v1".to_owned(),
            role: ProgramRole::Client,
            grants,
        }
    }

    fn call_id(sequence: u64) -> TransientEffectCallId {
        serde_json::from_value(serde_json::json!({
            "launch_epoch": 7,
            "sequence": sequence,
        }))
        .unwrap()
    }

    fn invocation(operation: &str, sequence: u64) -> TransientEffectInvocation {
        let effect_id = EffectId::from_host_operation(operation).unwrap();
        TransientEffectInvocation {
            call_id: call_id(sequence),
            invocation_id: EffectInvocationId::from_result_owner(effect_id, "store.result")
                .unwrap(),
            effect_id,
            trigger_source_sequence: sequence,
            authority_turn_sequence: sequence,
            target: None,
            intent: Value::Record(BTreeMap::from([(
                "byte_count".to_owned(),
                Value::integer(4).unwrap(),
            )])),
            delivery: EffectDeliveryCardinality::Single,
        }
    }

    #[test]
    fn policy_requires_canonical_supported_operations_and_selected_grants() {
        let random = builtin_effect_contract(SECURE_RANDOM_BYTES_OPERATION)
            .unwrap()
            .unwrap();
        let host = BrowserClientEffectHostCore::new(
            &profile(&["host.secure-random"]),
            std::slice::from_ref(&random),
            2,
        )
        .unwrap();
        assert_eq!(host.pending_count(), 0);

        let denied = BrowserClientEffectHostCore::new(&profile(&[]), &[random], 2).unwrap_err();
        assert!(matches!(denied, WebHostError::CapabilityDenied { .. }));

        let http = builtin_effect_contract(boon_effect_schema::OUTBOUND_HTTP_REQUEST_OPERATION)
            .unwrap()
            .unwrap();
        let unsupported =
            BrowserClientEffectHostCore::new(&profile(&["browser.same-origin-http"]), &[http], 2)
                .unwrap_err();
        assert!(matches!(unsupported, WebHostError::Unsupported { .. }));
    }

    #[test]
    fn exact_call_completion_cancellation_and_credit_paths_fail_closed() {
        let random = builtin_effect_contract(SECURE_RANDOM_BYTES_OPERATION)
            .unwrap()
            .unwrap();
        let mut host =
            BrowserClientEffectHostCore::new(&profile(&["host.secure-random"]), &[random], 2)
                .unwrap();
        let first = invocation(SECURE_RANDOM_BYTES_OPERATION, 1);
        let second = invocation(SECURE_RANDOM_BYTES_OPERATION, 2);
        let mut owners = host.owners.clone();
        let mut commands = Vec::new();
        BrowserClientEffectHostCore::route_batch_into(
            &host.authorized,
            host.max_active,
            &mut owners,
            &[],
            &[],
            std::slice::from_ref(&first),
            &mut commands,
        )
        .unwrap();
        host.owners = owners;
        assert_eq!(host.pending_count(), 1);
        assert!(matches!(
            commands.as_slice(),
            [BrowserClientEffectCommand::Submit { invocation, .. }]
                if invocation.call_id == first.call_id
        ));
        assert_eq!(
            host.accept_completion(first.call_id).unwrap(),
            BrowserClientEffectKind::SecureRandom
        );
        assert!(host.accept_completion(first.call_id).is_err());

        let mut owners = host.owners.clone();
        let mut commands = Vec::new();
        BrowserClientEffectHostCore::route_batch_into(
            &host.authorized,
            host.max_active,
            &mut owners,
            &[],
            &[],
            std::slice::from_ref(&second),
            &mut commands,
        )
        .unwrap();
        BrowserClientEffectHostCore::route_batch_into(
            &host.authorized,
            host.max_active,
            &mut owners,
            &[second.call_id],
            &[],
            &[],
            &mut commands,
        )
        .unwrap();
        host.owners = owners;
        assert_eq!(host.pending_count(), 0);
        assert!(matches!(
            commands.last(),
            Some(BrowserClientEffectCommand::Cancel { call_id, .. })
                if *call_id == second.call_id
        ));

        let mut owners = host.owners.clone();
        let error = BrowserClientEffectHostCore::route_batch_into(
            &host.authorized,
            host.max_active,
            &mut owners,
            &[],
            &[TransientEffectCreditGrant {
                call_id: second.call_id,
                credits: 1,
            }],
            &[],
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("stream credit"));
        assert!(host.cancel_all().is_empty());
    }

    #[test]
    fn real_distributed_client_effect_round_trips_through_exact_browser_owner() {
        let request = |role, source: &str| ProgramCompileRequest {
            revision: 1,
            role,
            entry_path: "RUN.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.browser-effect-host-test",
                format!("{}-state", role.as_str()),
                "test",
            ),
            capability_profile: match role {
                ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
                ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
                ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
            },
        };
        let client_source = r#"
store: [
    randomize: SOURCE
    random:
        RandomNotRead |> HOLD random {
            randomize |> THEN { Random/bytes(byte_count: 1) }
        }
    random_size:
        random |> WHEN {
            RandomBytesReady => random.bytes |> Bytes/length()
            __ => 0
        }
]

scene: Scene/Element/text(
    element: [events: [press: store.randomize]]
    style: [width: Fill]
    text: TEXT { Browser effect }
)
"#;
        let passive = "store: [ready: True]";
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, client_source),
            request(ProgramRole::Session, passive),
            request(ProgramRole::Server, passive),
        ])
        .unwrap();
        let artifact = bundle.artifact(ProgramRole::Client).unwrap();
        let mut runtime =
            DistributedClientRuntime::start(artifact, ClientSessionQueueLimits::default()).unwrap();
        runtime.bind(SessionId::from_bytes([9; 32]), 1, 0).unwrap();
        runtime.mark_current().unwrap();
        let update = runtime
            .dispatch("store.randomize", SourcePayload::default())
            .unwrap();
        let invocation = update
            .turns
            .iter()
            .flat_map(|turn| &turn.transient_effects)
            .next()
            .cloned()
            .expect("Client random invocation");
        let mut host = BrowserClientEffectHostCore::new(
            &profile(&["host.secure-random"]),
            &artifact.plan().effects,
            4,
        )
        .unwrap();
        let commands = host.route_turns(&update.turns).unwrap();
        assert!(matches!(
            commands.as_slice(),
            [BrowserClientEffectCommand::Submit { invocation: routed, .. }]
                if routed.call_id == invocation.call_id
        ));

        host.accept_completion(invocation.call_id).unwrap();
        runtime
            .complete_transient_effect(
                invocation.call_id,
                Value::Record(BTreeMap::from([
                    (
                        "$tag".to_owned(),
                        Value::Text("RandomBytesReady".to_owned()),
                    ),
                    ("bytes".to_owned(), Value::Bytes(vec![42].into())),
                ])),
            )
            .unwrap();
        assert_eq!(
            runtime.root_value_current("store.random_size").unwrap(),
            Value::integer(1).unwrap()
        );
        assert_eq!(runtime.pending_transient_effect_count(), 0);
        assert_eq!(host.pending_count(), 0);
    }
}
