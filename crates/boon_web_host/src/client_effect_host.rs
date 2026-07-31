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
}

fn invalid_policy(reason: impl ToString) -> WebHostError {
    WebHostError::InvalidInput {
        field: "browser Client effect policy".to_owned(),
        reason: reason.to_string(),
    }
}
