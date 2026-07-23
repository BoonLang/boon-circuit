#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use crate::{WebHostError, WebHostResult};
use boon_app_package::CapabilityProfileDescriptor;
use boon_effect_schema::{
    CONTENT_IMPORT_OPERATION, CONTENT_SAVE_OPERATION, FILE_READ_BYTES_OPERATION,
    FILE_READ_STREAM_OPERATION, FILE_WRITE_BYTES_OPERATION, SECURE_RANDOM_BYTES_OPERATION,
    TIMER_DEADLINE_OPERATION, WALL_CLOCK_READ_OPERATION,
};
use boon_plan::{EffectContract, ProgramRole, builtin_effect_contract};
use boon_runtime::{
    ExactCallHostCore, RuntimeTurn, TransientEffectCallId, TransientEffectCreditGrant,
    TransientEffectInvocation,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BrowserClientEffectKind {
    WallClock,
    SecureRandom,
    Deadline,
    FileReadBytes,
    FileWriteBytes,
    FileReadStream,
    ContentImport,
    ContentSave,
}

impl BrowserClientEffectKind {
    fn policy(operation: &str) -> Option<(Self, &'static str)> {
        match operation {
            WALL_CLOCK_READ_OPERATION => Some((Self::WallClock, "host.clock")),
            SECURE_RANDOM_BYTES_OPERATION => Some((Self::SecureRandom, "host.secure-random")),
            TIMER_DEADLINE_OPERATION => Some((Self::Deadline, "host.timers")),
            FILE_READ_BYTES_OPERATION => Some((Self::FileReadBytes, "host.file-read")),
            FILE_WRITE_BYTES_OPERATION => Some((Self::FileWriteBytes, "host.file-write")),
            FILE_READ_STREAM_OPERATION => Some((Self::FileReadStream, "host.file-read")),
            CONTENT_IMPORT_OPERATION => Some((Self::ContentImport, "host.content-import")),
            CONTENT_SAVE_OPERATION => Some((Self::ContentSave, "host.content-save")),
            _ => None,
        }
    }

    pub(crate) fn operation(self) -> &'static str {
        match self {
            Self::WallClock => WALL_CLOCK_READ_OPERATION,
            Self::SecureRandom => SECURE_RANDOM_BYTES_OPERATION,
            Self::Deadline => TIMER_DEADLINE_OPERATION,
            Self::FileReadBytes => FILE_READ_BYTES_OPERATION,
            Self::FileWriteBytes => FILE_WRITE_BYTES_OPERATION,
            Self::FileReadStream => FILE_READ_STREAM_OPERATION,
            Self::ContentImport => CONTENT_IMPORT_OPERATION,
            Self::ContentSave => CONTENT_SAVE_OPERATION,
        }
    }

    fn is_stream(self) -> bool {
        matches!(
            self,
            Self::FileReadStream | Self::ContentImport | Self::ContentSave
        )
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
    GrantCredits {
        kind: BrowserClientEffectKind,
        grant: TransientEffectCreditGrant,
    },
}

/// Exact-call ownership and capability policy for browser-owned Client effects.
///
/// Platform adapters execute the returned commands, but only this core may
/// admit, cancel, or complete a runtime call ID.
#[derive(Clone, Debug)]
pub(crate) struct BrowserClientEffectHostCore {
    calls: ExactCallHostCore<BrowserClientEffectKind>,
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
            let canonical = builtin_effect_contract(&contract.host_operation)
                .map_err(invalid_policy)?
                .ok_or_else(|| invalid_policy("browser effect has no canonical contract"))?;
            if contract != &canonical {
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

        let calls = ExactCallHostCore::new(authorized, max_active).map_err(invalid_policy)?;
        Ok(Self { calls })
    }

    pub(crate) fn route_turns(
        &mut self,
        turns: &[RuntimeTurn],
    ) -> WebHostResult<Vec<BrowserClientEffectCommand>> {
        let mut candidate = self.calls.clone();
        let mut commands = Vec::new();
        for turn in turns {
            Self::route_batch_into(
                &mut candidate,
                &turn.cancelled_transient_effects,
                &turn.transient_effect_credit_grants,
                &turn.transient_effects,
                &mut commands,
            )?;
        }
        self.calls = candidate;
        Ok(commands)
    }

    #[cfg(test)]
    fn route_batch(
        &mut self,
        cancelled: &[TransientEffectCallId],
        credits: &[TransientEffectCreditGrant],
        invocations: &[TransientEffectInvocation],
    ) -> WebHostResult<Vec<BrowserClientEffectCommand>> {
        let mut candidate = self.calls.clone();
        let mut commands = Vec::new();
        Self::route_batch_into(
            &mut candidate,
            cancelled,
            credits,
            invocations,
            &mut commands,
        )?;
        self.calls = candidate;
        Ok(commands)
    }

    fn route_batch_into(
        calls: &mut ExactCallHostCore<BrowserClientEffectKind>,
        cancelled: &[TransientEffectCallId],
        credits: &[TransientEffectCreditGrant],
        invocations: &[TransientEffectInvocation],
        commands: &mut Vec<BrowserClientEffectCommand>,
    ) -> WebHostResult<()> {
        for (kind, call_id) in calls.cancel_calls(cancelled) {
            commands.push(BrowserClientEffectCommand::Cancel { kind, call_id });
        }
        for (kind, grant) in calls.credit_lanes(credits).map_err(invalid_policy)? {
            if !kind.is_stream() {
                return Err(invalid_policy(format!(
                    "single-result browser effect call {} received stream credit",
                    grant.call_id
                )));
            }
            commands.push(BrowserClientEffectCommand::GrantCredits { kind, grant });
        }

        for invocation in invocations {
            let kind = calls.authorized_lane(invocation.effect_id).ok_or_else(|| {
                invalid_policy(format!(
                    "browser host is not authorized for effect {}",
                    invocation.effect_id
                ))
            })?;
            let expected = builtin_effect_contract(kind.operation())
                .map_err(invalid_policy)?
                .expect("browser effect kinds have canonical contracts")
                .delivery;
            if invocation.delivery != expected {
                return Err(invalid_policy(format!(
                    "browser effect call {} differs from its canonical delivery",
                    invocation.call_id
                )));
            }
        }
        let admitted = calls.admit(invocations.to_vec()).map_err(invalid_policy)?;
        for (kind, invocation) in admitted {
            commands.push(BrowserClientEffectCommand::Submit { kind, invocation });
        }
        Ok(())
    }

    pub(crate) fn accept_result(
        &mut self,
        call_id: TransientEffectCallId,
        kind: BrowserClientEffectKind,
        terminal: bool,
    ) -> WebHostResult<()> {
        self.calls
            .accept_result(call_id, kind, terminal)
            .map_err(|error| WebHostError::InvalidInput {
                field: "browser effect result".to_owned(),
                reason: error.to_string(),
            })
    }

    pub(crate) fn cancel_all(&mut self) -> Vec<BrowserClientEffectCommand> {
        let calls = self.calls.active_call_ids();
        self.calls
            .cancel_calls(&calls)
            .into_iter()
            .map(|(kind, call_id)| BrowserClientEffectCommand::Cancel { kind, call_id })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn pending_count(&self) -> usize {
        self.calls.active_count()
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
    use boon_plan::{
        EffectDeliveryCardinality, EffectId, EffectInvocationId, OwnerInstanceId,
        builtin_effect_contract,
    };
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
        TransientEffectCallId::from_host_parts(7, sequence)
    }

    fn invocation(operation: &str, sequence: u64) -> TransientEffectInvocation {
        let effect_id = EffectId::from_host_operation(operation).unwrap();
        TransientEffectInvocation {
            call_id: call_id(sequence),
            invocation_id: EffectInvocationId::from_result_owner(effect_id, "store.result")
                .unwrap(),
            effect_id,
            trigger_sequence: sequence,
            authority_turn_sequence: sequence,
            owner: OwnerInstanceId::root(),
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
        let commands = host
            .route_batch(&[], &[], std::slice::from_ref(&first))
            .unwrap();
        assert_eq!(host.pending_count(), 1);
        assert!(matches!(
            commands.as_slice(),
            [BrowserClientEffectCommand::Submit { invocation, .. }]
                if invocation.call_id == first.call_id
        ));
        host.accept_result(first.call_id, BrowserClientEffectKind::SecureRandom, true)
            .unwrap();
        assert!(
            host.accept_result(first.call_id, BrowserClientEffectKind::SecureRandom, true)
                .is_err()
        );

        let mut commands = host
            .route_batch(&[], &[], std::slice::from_ref(&second))
            .unwrap();
        commands.extend(host.route_batch(&[second.call_id], &[], &[]).unwrap());
        assert_eq!(host.pending_count(), 0);
        assert!(matches!(
            commands.last(),
            Some(BrowserClientEffectCommand::Cancel { call_id, .. })
                if *call_id == second.call_id
        ));

        let error = host
            .route_batch(
                &[],
                &[TransientEffectCreditGrant {
                    call_id: second.call_id,
                    credits: 1,
                }],
                &[],
            )
            .unwrap_err();
        assert!(error.to_string().contains("stream credit"));
        assert!(host.cancel_all().is_empty());
    }

    #[test]
    fn file_stream_ownership_survives_results_and_routes_bounded_credits() {
        let contract = builtin_effect_contract(FILE_READ_STREAM_OPERATION)
            .unwrap()
            .unwrap();
        let mut host = BrowserClientEffectHostCore::new(
            &profile(&["host.file-read"]),
            std::slice::from_ref(&contract),
            1,
        )
        .unwrap();
        let mut stream = invocation(FILE_READ_STREAM_OPERATION, 1);
        stream.delivery = contract.delivery;
        let commands = host
            .route_batch(&[], &[], std::slice::from_ref(&stream))
            .unwrap();
        assert!(matches!(
            commands.as_slice(),
            [BrowserClientEffectCommand::Submit {
                kind: BrowserClientEffectKind::FileReadStream,
                invocation,
            }] if invocation.call_id == stream.call_id
        ));

        host.accept_result(
            stream.call_id,
            BrowserClientEffectKind::FileReadStream,
            false,
        )
        .unwrap();
        assert_eq!(host.pending_count(), 1);
        let grant = TransientEffectCreditGrant {
            call_id: stream.call_id,
            credits: 1,
        };
        let commands = host.route_batch(&[], &[grant], &[]).unwrap();
        assert!(matches!(
            commands.as_slice(),
            [BrowserClientEffectCommand::GrantCredits {
                kind: BrowserClientEffectKind::FileReadStream,
                grant: routed,
            }] if *routed == grant
        ));
        assert!(
            host.accept_result(stream.call_id, BrowserClientEffectKind::ContentImport, true)
                .is_err()
        );
        host.accept_result(
            stream.call_id,
            BrowserClientEffectKind::FileReadStream,
            true,
        )
        .unwrap();
        assert_eq!(host.pending_count(), 0);
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

        host.accept_result(
            invocation.call_id,
            BrowserClientEffectKind::SecureRandom,
            true,
        )
        .unwrap();
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
