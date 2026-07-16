use boon_host_runtime::{
    HostServiceEffectAdapter, NamedSecret, apply_completion, apply_submission,
};
use boon_host_services::{HostServiceConfig, HostServices, SecretMaterial};
use boon_plan::{ApplicationIdentity, FiniteReal};
use boon_runtime::{
    ProgramCapabilityProfile, ProgramCompileRequest, ProgramSession, RuntimeSourceUnit,
    SourcePayload, TransientEffectInvocation, Value, compile_program_artifact,
};
use std::time::Duration;

fn number(value: i64) -> Value {
    Value::Number(FiniteReal::from_i64_exact(value).unwrap())
}

fn program() -> ProgramSession {
    let artifact = compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: "host_service_effects.bn".to_owned(),
        units: vec![RuntimeSourceUnit {
            path: "host_service_effects.bn".to_owned(),
            source: include_str!("../../../examples/host_service_effects.bn").to_owned(),
        }],
        application: ApplicationIdentity::new("dev.boon.host-services", "test", "local"),
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .unwrap();
    ProgramSession::start(artifact).unwrap()
}

fn adapter(max_active_deadlines: usize) -> HostServiceEffectAdapter {
    HostServiceEffectAdapter::new(
        HostServices::new(HostServiceConfig::default()),
        [NamedSecret::new(
            "session",
            SecretMaterial::new(b"correct horse battery staple".to_vec()),
        )],
        max_active_deadlines,
    )
    .unwrap()
}

fn invocation(
    program: &mut ProgramSession,
    source: &str,
    fields: impl IntoIterator<Item = (&'static str, Value)>,
) -> TransientEffectInvocation {
    let dispatched = program
        .dispatch(
            source,
            None,
            SourcePayload {
                fields: fields
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), value))
                    .collect(),
                ..SourcePayload::default()
            },
        )
        .unwrap();
    assert!(dispatched.runtime_turn.outbox_changes.is_empty());
    let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
        panic!("source must emit exactly one typed host-service effect");
    };
    invocation.clone()
}

fn apply_immediate(
    program: &mut ProgramSession,
    adapter: &mut HostServiceEffectAdapter,
    invocation: TransientEffectInvocation,
) {
    let submission = adapter.submit(invocation).unwrap();
    assert!(submission.immediate_completion.is_some());
    assert!(apply_submission(program, submission).unwrap().is_some());
}

#[tokio::test]
async fn compiled_boon_uses_typed_clock_random_secret_hmac_and_deadline_services() {
    let mut program = program();
    let mut adapter = adapter(4);

    let read_clock = invocation(&mut program, "store.read_clock", []);
    assert!(adapter.owns(read_clock.effect_id));
    apply_immediate(&mut program, &mut adapter, read_clock);
    let Value::Number(clock_seconds) = program.output_value_current("clock_seconds").unwrap()
    else {
        panic!("clock output must be Number");
    };
    assert!(clock_seconds.to_i64_exact().unwrap() > 1_700_000_000);

    let random = invocation(
        &mut program,
        "store.request_random",
        [("byte_count", number(24))],
    );
    apply_immediate(&mut program, &mut adapter, random);
    assert_eq!(
        program.output_value_current("random_byte_count").unwrap(),
        number(24)
    );

    let verify_secret = invocation(
        &mut program,
        "store.verify_secret",
        [
            ("secret", Value::Text("session".to_owned())),
            (
                "candidate",
                Value::Bytes(b"correct horse battery staple".to_vec()),
            ),
        ],
    );
    apply_immediate(&mut program, &mut adapter, verify_secret);
    assert_eq!(
        program.output_value_current("secret_match").unwrap(),
        Value::Bool(true)
    );

    let message = b"session-id.expiry".to_vec();
    let sign = invocation(
        &mut program,
        "store.sign_message",
        [
            ("secret", Value::Text("session".to_owned())),
            ("message", Value::Bytes(message.clone())),
        ],
    );
    apply_immediate(&mut program, &mut adapter, sign);
    let Value::Bytes(tag) = program.output_value_current("hmac_tag").unwrap() else {
        panic!("HMAC output must be Bytes");
    };
    assert_eq!(tag.len(), 32);

    let verify_hmac = invocation(
        &mut program,
        "store.verify_signature",
        [
            ("secret", Value::Text("session".to_owned())),
            ("message", Value::Bytes(message)),
            ("tag", Value::Bytes(tag)),
        ],
    );
    apply_immediate(&mut program, &mut adapter, verify_hmac);
    assert_eq!(
        program.output_value_current("hmac_match").unwrap(),
        Value::Bool(true)
    );

    let deadline = invocation(
        &mut program,
        "store.schedule_deadline",
        [("delay_ms", number(10))],
    );
    let submission = adapter.submit(deadline).unwrap();
    assert!(submission.immediate_completion.is_none());
    assert_eq!(adapter.active_deadline_count(), 1);
    let completion = tokio::time::timeout(Duration::from_secs(1), adapter.next_completion())
        .await
        .unwrap()
        .unwrap();
    apply_completion(&mut program, completion).unwrap();
    assert_eq!(
        program.output_value_current("timer_delay_ms").unwrap(),
        number(10)
    );
    assert_eq!(program.pending_transient_effect_count(), 0);
}

#[tokio::test]
async fn unknown_secrets_and_deadline_capacity_fail_as_typed_results() {
    let mut program = program();
    let mut adapter = adapter(1);

    let unknown = invocation(
        &mut program,
        "store.verify_secret",
        [
            ("secret", Value::Text("absent".to_owned())),
            ("candidate", Value::Bytes(Vec::new())),
        ],
    );
    apply_immediate(&mut program, &mut adapter, unknown);
    assert_eq!(
        program.output_value_current("error_code").unwrap(),
        Value::Text("unknown_secret".to_owned())
    );

    let first = invocation(
        &mut program,
        "store.schedule_deadline",
        [("delay_ms", number(200))],
    );
    let first_call = first.call_id;
    assert!(
        adapter
            .submit(first)
            .unwrap()
            .immediate_completion
            .is_none()
    );
    let second = invocation(
        &mut program,
        "store.schedule_deadline",
        [("delay_ms", number(200))],
    );
    let second_submission = adapter.submit(second).unwrap();
    assert!(second_submission.immediate_completion.is_some());
    apply_submission(&mut program, second_submission).unwrap();
    assert_eq!(
        program.output_value_current("error_code").unwrap(),
        Value::Text("host_busy".to_owned())
    );

    assert!(adapter.cancel(first_call));
    program.cancel_transient_effect(first_call).unwrap();
    assert_eq!(adapter.active_deadline_count(), 0);
    assert_eq!(program.pending_transient_effect_count(), 0);
}
