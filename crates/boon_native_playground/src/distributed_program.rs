use crate::protocol::{ApplicationIdentity, SourceUnit};
use boon_plan::ProgramRole;
use boon_runtime::{
    DistributedProgramBundle, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeResult,
    RuntimeSourceUnit, compile_distributed_program_bundle,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramSource {
    pub role: ProgramRole,
    pub entry_path: String,
    pub units: Vec<SourceUnit>,
    pub application: ApplicationIdentity,
}

pub(crate) fn compile_distributed_program(
    mut sources: Vec<ProgramSource>,
) -> RuntimeResult<DistributedProgramBundle> {
    sources.sort_by_key(|source| role_rank(source.role));
    let requests = sources
        .into_iter()
        .map(program_compile_request)
        .collect::<Vec<_>>();
    compile_distributed_program_bundle(&requests).map_err(Into::into)
}

fn program_compile_request(source: ProgramSource) -> ProgramCompileRequest {
    let capability_profile = match source.role {
        ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
        ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
        ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
    };
    let mut units = source.units;
    if let Some(index) = units.iter().position(|unit| unit.path == source.entry_path) {
        let entry = units.remove(index);
        units.insert(0, entry);
    }
    ProgramCompileRequest {
        revision: 1,
        role: source.role,
        entry_path: source.entry_path.clone(),
        units: units
            .into_iter()
            .map(|unit| RuntimeSourceUnit {
                path: unit.path,
                source: unit.source,
            })
            .collect(),
        application: source.application,
        capability_profile,
    }
}

fn role_rank(role: ProgramRole) -> u8 {
    match role {
        ProgramRole::Client => 0,
        ProgramRole::Session => 1,
        ProgramRole::Server => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_runtime::{ProgramArtifact, SourcePayload, Value};

    const SHARED_PATH: &str = "distributed_fixture/Shared/DistributedContract.bn";
    const CLIENT_PATH: &str = "distributed_fixture/Client/RUN.bn";
    const SESSION_PATH: &str = "distributed_fixture/Session/RUN.bn";
    const SERVER_PATH: &str = "distributed_fixture/Server/RUN.bn";
    const SHARED_SOURCE: &str =
        include_str!("../testdata/distributed_fixture/Shared/DistributedContract.bn");
    const CLIENT_SOURCE: &str = include_str!("../testdata/distributed_fixture/Client/RUN.bn");
    const SESSION_SOURCE: &str = include_str!("../testdata/distributed_fixture/Session/RUN.bn");
    const SERVER_SOURCE: &str = include_str!("../testdata/distributed_fixture/Server/RUN.bn");

    #[test]
    fn unrelated_distributed_program_compiles_and_runs_as_isolated_deterministic_sessions() {
        let bundle =
            compile_distributed_program(fixture_sources()).expect("compile distributed fixture");
        assert_eq!(bundle.artifacts().len(), 3);
        let client = bundle.artifact(ProgramRole::Client).expect("client");
        let session = bundle.artifact(ProgramRole::Session).expect("session");
        let server = bundle.artifact(ProgramRole::Server).expect("server");
        assert_eq!(client.application().state_namespace, "fixture-client");
        assert_eq!(session.application().state_namespace, "fixture-session");
        assert_eq!(server.application().state_namespace, "fixture-server");
        assert_eq!(
            client.capability_profile(),
            ProgramCapabilityProfile::PublicClient
        );
        assert_eq!(
            session.capability_profile(),
            ProgramCapabilityProfile::TrustedSession
        );
        assert_eq!(
            server.capability_profile(),
            ProgramCapabilityProfile::TrustedServer
        );
        assert_eq!(
            client.source_digest(),
            boon_runtime::sha256_bytes(CLIENT_SOURCE.as_bytes())
        );
        assert_eq!(
            session.source_digest(),
            boon_runtime::sha256_bytes(SESSION_SOURCE.as_bytes())
        );
        assert_eq!(
            server.source_digest(),
            boon_runtime::sha256_bytes(SERVER_SOURCE.as_bytes())
        );
        assert_ne!(client.plan_digest(), session.plan_digest());
        assert_ne!(session.plan_digest(), server.plan_digest());

        let stable_document_session = bundle
            .start_distributed()
            .expect("first distributed lifecycle")
            .session_id(ProgramRole::Client)
            .expect("document session")
            .clone();
        let mut lifecycle = bundle
            .start_distributed()
            .expect("start distributed fixture");
        assert_eq!(lifecycle.session_count(), 3);
        assert_eq!(
            lifecycle.session_id(ProgramRole::Client),
            Some(&stable_document_session)
        );
        assert_ne!(
            lifecycle.session_id(ProgramRole::Client),
            lifecycle.session_id(ProgramRole::Session)
        );
        assert_ne!(
            lifecycle.session_id(ProgramRole::Session),
            lifecycle.session_id(ProgramRole::Server)
        );
        let expected_session_id = lifecycle
            .session_id(ProgramRole::Session)
            .expect("session identity")
            .0
            .clone();
        assert!(matches!(
            lifecycle
                .output_value_current(ProgramRole::Session, "status")
                .expect("session status"),
            Value::Record(fields)
                if fields.get("$tag") == Some(&Value::Text("Active".to_owned()))
                    && fields.get("role") == Some(&Value::Text("Session".to_owned()))
                    && fields.get("session_id") == Some(&Value::Text(expected_session_id))
        ));
        assert_eq!(
            lifecycle
                .output_value_current(ProgramRole::Session, "principal")
                .expect("session principal"),
            Value::Text("Anonymous".to_owned())
        );
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Client, "store.client_count")
                .expect("client count"),
            Value::integer(0).unwrap()
        );
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Server, "store.server_count")
                .expect("server count"),
            Value::integer(0).unwrap()
        );

        let server_turn = lifecycle
            .dispatch(
                ProgramRole::Server,
                "store.request_received",
                None,
                SourcePayload::default(),
            )
            .expect("server turn");
        assert_eq!(server_turn.turn.lifecycle_sequence, 1);
        assert_eq!(server_turn.turn.source_sequence, 1);
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Server, "store.server_count")
                .expect("updated server count"),
            Value::integer(1).unwrap()
        );
        let response = lifecycle
            .output_value_current(ProgramRole::Server, "api_response")
            .expect("server output");
        assert!(matches!(response, Value::Record(fields)
            if fields.get("count") == Some(&Value::integer(1).unwrap())));
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Client, "store.client_count")
                .expect("isolated client count"),
            Value::integer(0).unwrap()
        );

        let client_turn = lifecycle
            .dispatch(
                ProgramRole::Client,
                "store.increment",
                None,
                SourcePayload::default(),
            )
            .expect("client turn");
        assert_eq!(client_turn.turn.lifecycle_sequence, 2);
        assert_eq!(client_turn.turn.source_sequence, 1);
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Client, "store.client_count")
                .expect("updated client count"),
            Value::integer(1).unwrap()
        );
        assert_eq!(
            lifecycle
                .turn_log()
                .iter()
                .map(|turn| (turn.lifecycle_sequence, turn.role, turn.source_sequence))
                .collect::<Vec<_>>(),
            [(1, ProgramRole::Server, 1), (2, ProgramRole::Client, 1)]
        );
    }

    #[test]
    fn distributed_fixture_uses_runtime_content_artifacts_for_all_roles() {
        let bundle =
            compile_distributed_program(fixture_sources()).expect("compile distributed fixture");
        for (role, profile) in [
            (ProgramRole::Client, ProgramCapabilityProfile::PublicClient),
            (
                ProgramRole::Session,
                ProgramCapabilityProfile::TrustedSession,
            ),
            (ProgramRole::Server, ProgramCapabilityProfile::TrustedServer),
        ] {
            let artifact = bundle.artifact(role).expect("role artifact");
            let restored =
                ProgramArtifact::from_content_artifact(9, profile, artifact.to_content_artifact())
                    .expect("load serialized artifact");
            assert_eq!(restored.id(), artifact.id());
            assert_eq!(restored.role(), role);
            assert_eq!(restored.plan_digest(), artifact.plan_digest());
        }
    }

    #[test]
    fn declared_role_is_validated_against_the_compiled_output_boundary() {
        let mut sources = fixture_sources();
        sources
            .iter_mut()
            .find(|source| source.role == ProgramRole::Server)
            .expect("server source")
            .units = vec![
            SourceUnit {
                path: SHARED_PATH.to_owned(),
                source: SHARED_SOURCE.to_owned(),
            },
            SourceUnit {
                path: SERVER_PATH.to_owned(),
                source: CLIENT_SOURCE.to_owned(),
            },
        ];
        let error = compile_distributed_program(sources).expect_err("role mismatch");
        assert!(
            error
                .to_string()
                .contains("server programs cannot contain retained document or scene roots"),
            "unexpected role mismatch diagnostic: {error}"
        );
    }

    #[test]
    fn distributed_program_rejects_a_shared_state_namespace() {
        let mut sources = fixture_sources();
        for source in &mut sources {
            source.application.state_namespace = "shared-state".to_owned();
        }
        let error = compile_distributed_program(sources).expect_err("shared namespace");
        assert!(
            error
                .to_string()
                .contains("distributed roles must use distinct state namespaces"),
            "unexpected shared-namespace diagnostic: {error}"
        );
    }

    fn fixture_sources() -> Vec<ProgramSource> {
        vec![
            ProgramSource {
                role: ProgramRole::Client,
                entry_path: CLIENT_PATH.to_owned(),
                units: vec![
                    SourceUnit {
                        path: SHARED_PATH.to_owned(),
                        source: SHARED_SOURCE.to_owned(),
                    },
                    SourceUnit {
                        path: CLIENT_PATH.to_owned(),
                        source: CLIENT_SOURCE.to_owned(),
                    },
                ],
                application: ApplicationIdentity::new(
                    "dev.boon.distributed-fixture",
                    "fixture-client",
                    "test",
                ),
            },
            ProgramSource {
                role: ProgramRole::Session,
                entry_path: SESSION_PATH.to_owned(),
                units: vec![
                    SourceUnit {
                        path: SHARED_PATH.to_owned(),
                        source: SHARED_SOURCE.to_owned(),
                    },
                    SourceUnit {
                        path: SESSION_PATH.to_owned(),
                        source: SESSION_SOURCE.to_owned(),
                    },
                ],
                application: ApplicationIdentity::new(
                    "dev.boon.distributed-fixture",
                    "fixture-session",
                    "test",
                ),
            },
            ProgramSource {
                role: ProgramRole::Server,
                entry_path: SERVER_PATH.to_owned(),
                units: vec![
                    SourceUnit {
                        path: SHARED_PATH.to_owned(),
                        source: SHARED_SOURCE.to_owned(),
                    },
                    SourceUnit {
                        path: SERVER_PATH.to_owned(),
                        source: SERVER_SOURCE.to_owned(),
                    },
                ],
                application: ApplicationIdentity::new(
                    "dev.boon.distributed-fixture",
                    "fixture-server",
                    "test",
                ),
            },
        ]
    }
}
