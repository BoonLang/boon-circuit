use std::time::Duration;

use boon_host_services::testing::{DeterministicHostServices, DeterministicProviderConfig};
use boon_host_services::{
    CancellationOutcome, HostCapabilities, HostLimit, HostServiceConfig, HostServiceError,
    HostServiceLimits, HostServices, ScheduleEventKind, SecretMaterial, TimerReceiveError,
    TimerState, WallClockSnapshot,
};

fn deterministic_host(
    host_instance_id: u64,
    config: HostServiceConfig,
) -> DeterministicHostServices {
    DeterministicHostServices::new(
        config,
        DeterministicProviderConfig::new(
            host_instance_id,
            WallClockSnapshot::from_unix_parts(1_800_000_000, 250_000_000).unwrap(),
            [0x5a; 32],
        ),
    )
}

#[test]
fn cancellation_is_idempotent_and_prevents_future_interval_events() {
    let mut host = deterministic_host(10, HostServiceConfig::default());
    let timer = host.schedule_interval(Duration::from_millis(10)).unwrap();
    let cancellation = timer.cancellation_handle();

    assert_eq!(cancellation.cancel(), CancellationOutcome::Cancelled);
    assert_eq!(cancellation.cancel(), CancellationOutcome::AlreadyCancelled);
    host.advance(Duration::from_millis(100)).unwrap();

    assert_eq!(timer.state(), TimerState::Cancelled);
    assert_eq!(timer.try_recv(), Err(TimerReceiveError::Cancelled));
}

#[test]
fn configured_limits_cover_bytes_timers_and_nanoseconds() {
    let limits = HostServiceLimits::default()
        .with_max_random_bytes_per_request(4)
        .with_max_concurrent_scheduled_timers(1)
        .with_interval_bounds(Duration::from_millis(5), Duration::from_millis(10))
        .with_max_deadline_horizon(Duration::from_millis(20))
        .with_max_configured_secrets(1)
        .with_max_secret_bytes(4)
        .with_max_verification_candidate_bytes(4)
        .with_max_hmac_message_bytes(4);
    let config = HostServiceConfig::new(HostCapabilities::ALL, limits).unwrap();
    let mut host = deterministic_host(11, config);

    assert!(matches!(
        host.secure_random(5),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::RandomBytesPerRequest,
            requested: 5,
            maximum: 4,
        })
    ));
    assert!(matches!(
        host.schedule_interval(Duration::from_millis(4)),
        Err(HostServiceError::BelowMinimum {
            limit: HostLimit::MinimumInterval,
            ..
        })
    ));
    assert!(matches!(
        host.schedule_interval(Duration::from_millis(11)),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::MaximumInterval,
            ..
        })
    ));
    assert!(matches!(
        host.schedule_deadline_after(Duration::from_millis(21)),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::DeadlineHorizon,
            ..
        })
    ));

    let timer = host
        .schedule_deadline_after(Duration::from_millis(10))
        .unwrap();
    assert!(matches!(
        host.schedule_deadline_after(Duration::from_millis(10)),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::ConcurrentScheduledTimers,
            requested: 2,
            maximum: 1,
        })
    ));
    assert_eq!(timer.cancel(), CancellationOutcome::Cancelled);

    assert!(matches!(
        host.configure_secret(SecretMaterial::new(b"12345".to_vec())),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::SecretBytes,
            requested: 5,
            maximum: 4,
        })
    ));
    let secret_ref = host
        .configure_secret(SecretMaterial::new(b"1234".to_vec()))
        .unwrap();
    assert!(matches!(
        host.configure_secret(SecretMaterial::new(b"next".to_vec())),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::ConfiguredSecrets,
            requested: 2,
            maximum: 1,
        })
    ));
    assert!(matches!(
        host.verify_configured_secret(secret_ref, b"12345"),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::VerificationCandidateBytes,
            requested: 5,
            maximum: 4,
        })
    ));
    assert!(matches!(
        host.hmac_sha256_sign(secret_ref, b"12345"),
        Err(HostServiceError::LimitExceeded {
            limit: HostLimit::HmacMessageBytes,
            requested: 5,
            maximum: 4,
        })
    ));
}

#[test]
fn secret_material_is_redacted_from_all_debug_surfaces() {
    let mut host = deterministic_host(12, HostServiceConfig::default());
    let secret_text = "NEVER_PRINT_THIS_SECRET";
    let secret_ref = host
        .configure_secret(SecretMaterial::new(secret_text.as_bytes().to_vec()))
        .unwrap();
    let tag = host.hmac_sha256_sign(secret_ref, b"message").unwrap();

    let host_debug = format!("{host:?}");
    let reference_debug = format!("{secret_ref:?}");
    let tag_debug = format!("{tag:?}");
    assert!(host_debug.contains("[REDACTED]"));
    assert!(tag_debug.contains("[REDACTED]"));
    assert!(!host_debug.contains(secret_text));
    assert!(!reference_debug.contains(secret_text));
    assert!(!tag_debug.contains(secret_text));
    assert_eq!(secret_ref.store_id().get(), 12);
    assert_eq!(secret_ref.secret_id().get(), 1);
}

#[test]
fn configured_secret_verification_has_fixed_tag_semantics_for_mismatch_shapes() {
    let mut host = deterministic_host(13, HostServiceConfig::default());
    let secret_ref = host
        .configure_secret(SecretMaterial::new(b"constant-time-token".to_vec()))
        .unwrap();

    assert!(
        host.verify_configured_secret(secret_ref, b"constant-time-token")
            .unwrap()
            .is_verified()
    );
    for mismatch in [
        b"constant-time-tokee".as_slice(),
        b"constant-time-toke".as_slice(),
        b"constant-time-token-extra".as_slice(),
        b"".as_slice(),
    ] {
        assert!(
            !host
                .verify_configured_secret(secret_ref, mismatch)
                .unwrap()
                .is_verified()
        );
    }
}

#[test]
fn hmac_sha256_rejects_message_and_tag_tampering() {
    let mut host = deterministic_host(14, HostServiceConfig::default());
    let secret_ref = host
        .configure_secret(SecretMaterial::new(b"hmac-key".to_vec()))
        .unwrap();
    let message = b"authenticated message";
    let tag = host.hmac_sha256_sign(secret_ref, message).unwrap();

    assert!(
        host.hmac_sha256_verify(secret_ref, message, &tag)
            .unwrap()
            .is_verified()
    );
    assert!(
        !host
            .hmac_sha256_verify(secret_ref, b"tampered message", &tag)
            .unwrap()
            .is_verified()
    );

    let mut tampered_bytes = tag.into_bytes();
    tampered_bytes[7] ^= 0x80;
    let tampered_tag = boon_host_services::HmacSha256Tag::from_bytes(tampered_bytes);
    assert!(
        !host
            .hmac_sha256_verify(secret_ref, message, &tampered_tag)
            .unwrap()
            .is_verified()
    );
}

#[test]
fn deterministic_time_and_interval_events_preserve_units_and_ownership() {
    let mut host = deterministic_host(15, HostServiceConfig::default());
    let start = host.monotonic_now().unwrap();
    let wall_start = host.wall_clock_now().unwrap();
    let timer = host.schedule_interval(Duration::from_millis(10)).unwrap();

    let now = host.advance(Duration::from_millis(35)).unwrap();
    let wall_now = host.wall_clock_now().unwrap();
    let event = timer.try_recv().unwrap();

    assert_eq!(start.clock_id(), now.clock_id());
    assert_eq!(now.nanoseconds_since_clock_origin(), 35_000_000);
    assert_eq!(
        now.duration_since(start).unwrap(),
        Duration::from_millis(35)
    );
    assert_eq!(
        wall_now.unix_epoch_seconds(),
        wall_start.unix_epoch_seconds()
    );
    assert_eq!(wall_now.nanoseconds_within_second(), 285_000_000);
    assert_eq!(event.scheduled_for().clock_id(), start.clock_id());
    assert_eq!(event.observed_at(), now);
    assert_eq!(
        event.kind(),
        ScheduleEventKind::Interval {
            sequence: 1,
            skipped_intervals: 2,
        }
    );
}

#[test]
fn monotonic_deadlines_reject_snapshots_from_another_owner() {
    let first = deterministic_host(16, HostServiceConfig::default());
    let second = deterministic_host(17, HostServiceConfig::default());
    let foreign_deadline = first.monotonic_now().unwrap();

    assert!(matches!(
        second.schedule_deadline_at(foreign_deadline),
        Err(HostServiceError::Time(
            boon_host_services::TimeError::ClockOwnerMismatch { .. }
        ))
    ));
}

#[test]
fn deterministic_random_provider_replays_the_same_byte_stream() {
    let first = deterministic_host(18, HostServiceConfig::default());
    let second = deterministic_host(18, HostServiceConfig::default());

    assert_eq!(
        first.secure_random(64).unwrap().as_bytes(),
        second.secure_random(64).unwrap().as_bytes()
    );
}

#[test]
fn shutdown_is_idempotent_and_closes_deterministic_timers() {
    let host = deterministic_host(19, HostServiceConfig::default());
    let timer = host.schedule_interval(Duration::from_secs(1)).unwrap();

    host.shutdown();
    host.shutdown();

    assert!(host.is_shutdown());
    assert_eq!(timer.state(), TimerState::SchedulerShutdown);
    assert_eq!(timer.recv(), Err(TimerReceiveError::SchedulerShutdown));
    assert!(matches!(
        host.monotonic_now(),
        Err(HostServiceError::Shutdown)
    ));
}

#[test]
fn production_shutdown_joins_worker_and_marks_pending_timers() {
    let host = HostServices::default();
    let timer = host
        .schedule_deadline_after(Duration::from_secs(60))
        .unwrap();

    host.shutdown();

    assert!(host.is_shutdown());
    assert_eq!(timer.state(), TimerState::SchedulerShutdown);
    assert_eq!(timer.try_recv(), Err(TimerReceiveError::SchedulerShutdown));
}
