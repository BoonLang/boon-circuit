use crate::protocol::ProgramSource;
use boon_editor::language::{LanguageProjectSnapshot, project_checked_language};
use boon_plan::ProgramRole;
use boon_program_runtime::{
    DistributedProgramBundle, DistributedProgramBundleWithClientProjection, ProgramCompileRequest,
    compile_distributed_program_bundle_with_client_projection,
};
use boon_runtime::{ProgramCapabilityProfile, RuntimeResult, RuntimeSourceUnit};

#[derive(Debug)]
pub(crate) struct CompiledDistributedProgram {
    pub bundle: DistributedProgramBundle,
    pub client_projection: LanguageProjectSnapshot,
}

pub(crate) fn compile_distributed_program(
    mut sources: Vec<ProgramSource>,
    language_revision: u64,
) -> RuntimeResult<CompiledDistributedProgram> {
    sources.sort_by_key(|source| role_rank(source.role));
    let client_units = sources
        .iter()
        .find(|source| source.role == ProgramRole::Client)
        .map(|source| source.units.clone())
        .ok_or_else(|| boon_plan::PlanError::new("distributed source has no Client role"))?;
    let requests = sources
        .into_iter()
        .map(program_compile_request)
        .collect::<Vec<_>>();
    let DistributedProgramBundleWithClientProjection {
        bundle,
        client_projection,
    } = compile_distributed_program_bundle_with_client_projection(&requests)?;
    let client_projection = project_checked_language(
        language_revision,
        &client_units,
        &client_projection.parsed,
        &client_projection.output,
    )
    .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
    Ok(CompiledDistributedProgram {
        bundle,
        client_projection,
    })
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
pub(crate) fn distributed_fixture_sources() -> Vec<ProgramSource> {
    use crate::protocol::{ApplicationIdentity, SourceUnit};

    const SHARED_PATH: &str = "distributed_fixture/Shared/DistributedContract.bn";
    const CLIENT_PATH: &str = "distributed_fixture/Client/RUN.bn";
    const SESSION_PATH: &str = "distributed_fixture/Session/RUN.bn";
    const SERVER_PATH: &str = "distributed_fixture/Server/RUN.bn";
    const SHARED_SOURCE: &str =
        include_str!("../testdata/distributed_fixture/Shared/DistributedContract.bn");
    const CLIENT_SOURCE: &str = include_str!("../testdata/distributed_fixture/Client/RUN.bn");
    const SESSION_SOURCE: &str = include_str!("../testdata/distributed_fixture/Session/RUN.bn");
    const SERVER_SOURCE: &str = include_str!("../testdata/distributed_fixture/Server/RUN.bn");

    [
        (
            ProgramRole::Client,
            CLIENT_PATH,
            CLIENT_SOURCE,
            "fixture-client",
        ),
        (
            ProgramRole::Session,
            SESSION_PATH,
            SESSION_SOURCE,
            "fixture-session",
        ),
        (
            ProgramRole::Server,
            SERVER_PATH,
            SERVER_SOURCE,
            "fixture-server",
        ),
    ]
    .into_iter()
    .map(
        |(role, entry_path, source, state_namespace)| ProgramSource {
            role,
            entry_path: entry_path.to_owned(),
            units: vec![
                SourceUnit {
                    path: SHARED_PATH.to_owned(),
                    source: SHARED_SOURCE.to_owned(),
                },
                SourceUnit {
                    path: entry_path.to_owned(),
                    source: source.to_owned(),
                },
            ],
            application: ApplicationIdentity::new(
                "dev.boon.distributed-fixture",
                state_namespace,
                "test",
            ),
        },
    )
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SourceUnit;
    use boon_contract::{SourceBundleDigestV1, SourceBundleUnit};
    use boon_program_runtime::ProgramArtifact;

    const SHARED_PATH: &str = "distributed_fixture/Shared/DistributedContract.bn";
    const CLIENT_PATH: &str = "distributed_fixture/Client/RUN.bn";
    const SESSION_PATH: &str = "distributed_fixture/Session/RUN.bn";
    const SERVER_PATH: &str = "distributed_fixture/Server/RUN.bn";
    const SHARED_SOURCE: &str =
        include_str!("../testdata/distributed_fixture/Shared/DistributedContract.bn");
    const CLIENT_SOURCE: &str = include_str!("../testdata/distributed_fixture/Client/RUN.bn");
    const SESSION_SOURCE: &str = include_str!("../testdata/distributed_fixture/Session/RUN.bn");
    const SERVER_SOURCE: &str = include_str!("../testdata/distributed_fixture/Server/RUN.bn");

    fn expected_source_bundle_digest_v1(entry_path: &str, source: &str) -> SourceBundleDigestV1 {
        SourceBundleDigestV1::new(
            entry_path,
            [
                SourceBundleUnit::new(SHARED_PATH, SHARED_SOURCE),
                SourceBundleUnit::new(entry_path, source),
            ],
        )
        .unwrap()
    }

    #[test]
    fn distributed_compile_preserves_role_artifacts_and_client_projection_identity() {
        let sources = distributed_fixture_sources();
        let client_units = sources
            .iter()
            .find(|source| source.role == ProgramRole::Client)
            .expect("client source")
            .units
            .clone();
        let CompiledDistributedProgram {
            bundle,
            client_projection,
        } = compile_distributed_program(sources, 41).expect("compile distributed fixture");
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
            client.source_bundle_digest_v1(),
            expected_source_bundle_digest_v1(CLIENT_PATH, CLIENT_SOURCE)
        );
        assert_eq!(
            session.source_bundle_digest_v1(),
            expected_source_bundle_digest_v1(SESSION_PATH, SESSION_SOURCE)
        );
        assert_eq!(
            server.source_bundle_digest_v1(),
            expected_source_bundle_digest_v1(SERVER_PATH, SERVER_SOURCE)
        );
        assert_ne!(client.plan_digest(), session.plan_digest());
        assert_ne!(session.plan_digest(), server.plan_digest());
        assert_eq!(client_projection.revision, 41);
        assert_eq!(client_projection.entrypoint, CLIENT_PATH);
        assert_eq!(
            client_projection.source_bundle_digest_v1,
            client.source_bundle_digest_v1()
        );
        assert!(client_projection.matches_source_units(&client_units));
        assert_eq!(
            client_projection
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [SHARED_PATH, CLIENT_PATH]
        );
        assert!(client_projection.diagnostics.is_empty());
        assert!(
            client_projection
                .files
                .iter()
                .any(|file| !file.inspector_hints.is_empty())
        );
        assert!(!client_projection.semantics.is_empty());
        assert!(client_projection.semantics.iter().all(|item| {
            matches!(item.location.path.as_str(), SHARED_PATH | CLIENT_PATH)
                && item.location.path != SESSION_PATH
                && item.location.path != SERVER_PATH
        }));
        let client_file = client_projection
            .files
            .iter()
            .position(|file| file.path == CLIENT_PATH)
            .expect("Client projection entry file");
        let materialized = client_projection
            .materialize_file(client_file, &client_units[client_file])
            .expect("materialize Client source from local bytes");
        assert_eq!(materialized.path, CLIENT_PATH);
        assert!(!materialized.lines.is_empty());

        let wire_hash = client
            .plan()
            .distributed_endpoint
            .as_ref()
            .expect("client distributed endpoint")
            .wire_schema_hash;
        assert_eq!(
            wire_hash,
            session
                .plan()
                .distributed_endpoint
                .as_ref()
                .expect("session distributed endpoint")
                .wire_schema_hash
        );
        assert_eq!(
            wire_hash,
            server
                .plan()
                .distributed_endpoint
                .as_ref()
                .expect("server distributed endpoint")
                .wire_schema_hash
        );
    }

    #[test]
    fn distributed_fixture_uses_runtime_content_artifacts_for_all_roles() {
        let bundle = compile_distributed_program(distributed_fixture_sources(), 1)
            .expect("compile distributed fixture")
            .bundle;
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
        let mut sources = distributed_fixture_sources();
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
        let error = compile_distributed_program(sources, 1).expect_err("role mismatch");
        assert!(
            error
                .to_string()
                .contains("server programs cannot contain retained document or scene roots"),
            "unexpected role mismatch diagnostic: {error}"
        );
    }

    #[test]
    fn distributed_program_rejects_a_shared_state_namespace() {
        let mut sources = distributed_fixture_sources();
        for source in &mut sources {
            source.application.state_namespace = "shared-state".to_owned();
        }
        let error = compile_distributed_program(sources, 1).expect_err("shared namespace");
        assert!(
            error
                .to_string()
                .contains("distributed roles must use distinct state namespaces"),
            "unexpected shared-namespace diagnostic: {error}"
        );
    }
}
