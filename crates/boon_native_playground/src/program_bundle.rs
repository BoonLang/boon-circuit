use crate::protocol::{ApplicationIdentity, SourceUnit};
use boon_plan::ProgramRole;
use boon_runtime::{
    ProgramArtifact, ProgramBundle, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeResult,
    RuntimeSourceUnit, compile_program_artifact,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramSource {
    pub role: ProgramRole,
    pub entry_path: String,
    pub units: Vec<SourceUnit>,
    pub application: ApplicationIdentity,
}

pub(crate) fn compile_program_bundle(
    mut sources: Vec<ProgramSource>,
) -> RuntimeResult<ProgramBundle> {
    sources.sort_by_key(|source| role_rank(source.role));
    let artifacts = sources
        .into_iter()
        .map(compile_program_source)
        .collect::<RuntimeResult<Vec<_>>>()?;
    ProgramBundle::new(artifacts)
}

fn compile_program_source(source: ProgramSource) -> RuntimeResult<ProgramArtifact> {
    let capability_profile = match source.role {
        ProgramRole::Document => ProgramCapabilityProfile::PublicDocument,
        ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
    };
    let mut units = source.units;
    if let Some(index) = units.iter().position(|unit| unit.path == source.entry_path) {
        let entry = units.remove(index);
        units.insert(0, entry);
    }
    let request = ProgramCompileRequest {
        revision: 1,
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
    };
    compile_program_artifact(&request).map_err(|error| {
        format!(
            "compile {} program `{}`: {error}",
            source.role.as_str(),
            source.entry_path
        )
        .into()
    })
}

fn role_rank(role: ProgramRole) -> u8 {
    match role {
        ProgramRole::Document => 0,
        ProgramRole::Server => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_runtime::{SourcePayload, Value};

    const SHARED_PATH: &str = "paired_fixture/Shared/PairContract.bn";
    const CLIENT_PATH: &str = "paired_fixture/Client/RUN.bn";
    const SERVER_PATH: &str = "paired_fixture/Server/RUN.bn";
    const SHARED_SOURCE: &str = include_str!("../testdata/paired_fixture/Shared/PairContract.bn");
    const CLIENT_SOURCE: &str = include_str!("../testdata/paired_fixture/Client/RUN.bn");
    const SERVER_SOURCE: &str = include_str!("../testdata/paired_fixture/Server/RUN.bn");

    #[test]
    fn unrelated_pair_compiles_and_runs_as_isolated_deterministic_sessions() {
        let bundle = compile_program_bundle(fixture_sources()).expect("compile paired fixture");
        assert_eq!(bundle.artifacts().len(), 2);
        let document = bundle.artifact(ProgramRole::Document).expect("document");
        let server = bundle.artifact(ProgramRole::Server).expect("server");
        assert_eq!(document.application().state_namespace, "fixture-client");
        assert_eq!(server.application().state_namespace, "fixture-server");
        assert_eq!(
            document.capability_profile(),
            ProgramCapabilityProfile::PublicDocument
        );
        assert_eq!(
            server.capability_profile(),
            ProgramCapabilityProfile::TrustedServer
        );
        assert_eq!(
            document.source_digest(),
            boon_runtime::sha256_bytes(CLIENT_SOURCE.as_bytes())
        );
        assert_eq!(
            server.source_digest(),
            boon_runtime::sha256_bytes(SERVER_SOURCE.as_bytes())
        );
        assert_ne!(document.plan_digest(), server.plan_digest());

        let stable_document_session = bundle
            .start_paired()
            .expect("first paired lifecycle")
            .session_id(ProgramRole::Document)
            .expect("document session")
            .clone();
        let mut lifecycle = bundle.start_paired().expect("start paired fixture");
        assert_eq!(lifecycle.session_count(), 2);
        assert_eq!(
            lifecycle.session_id(ProgramRole::Document),
            Some(&stable_document_session)
        );
        assert_ne!(
            lifecycle.session_id(ProgramRole::Document),
            lifecycle.session_id(ProgramRole::Server)
        );
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Document, "store.client_count")
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
                .root_value_current(ProgramRole::Document, "store.client_count")
                .expect("isolated client count"),
            Value::integer(0).unwrap()
        );

        let client_turn = lifecycle
            .dispatch(
                ProgramRole::Document,
                "store.increment",
                None,
                SourcePayload::default(),
            )
            .expect("client turn");
        assert_eq!(client_turn.turn.lifecycle_sequence, 2);
        assert_eq!(client_turn.turn.source_sequence, 1);
        assert_eq!(
            lifecycle
                .root_value_current(ProgramRole::Document, "store.client_count")
                .expect("updated client count"),
            Value::integer(1).unwrap()
        );
        assert_eq!(
            lifecycle
                .turn_log()
                .iter()
                .map(|turn| (turn.lifecycle_sequence, turn.role, turn.source_sequence))
                .collect::<Vec<_>>(),
            [(1, ProgramRole::Server, 1), (2, ProgramRole::Document, 1)]
        );
    }

    #[test]
    fn paired_fixture_uses_runtime_content_artifacts_for_both_roles() {
        let bundle = compile_program_bundle(fixture_sources()).expect("compile paired fixture");
        for (role, profile) in [
            (
                ProgramRole::Document,
                ProgramCapabilityProfile::PublicDocument,
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
        let error = compile_program_bundle(sources).expect_err("role mismatch");
        assert!(error.to_string().contains("requires ProgramRole::Server"));
    }

    #[test]
    fn paired_bundle_rejects_a_shared_state_namespace() {
        let mut sources = fixture_sources();
        for source in &mut sources {
            source.application.state_namespace = "shared-state".to_owned();
        }
        let error = compile_program_bundle(sources).expect_err("shared namespace");
        assert!(
            error
                .to_string()
                .contains("repeats state namespace `shared-state`")
        );
    }

    fn fixture_sources() -> Vec<ProgramSource> {
        vec![
            ProgramSource {
                role: ProgramRole::Document,
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
                    "dev.boon.paired-fixture",
                    "fixture-client",
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
                    "dev.boon.paired-fixture",
                    "fixture-server",
                    "test",
                ),
            },
        ]
    }
}
