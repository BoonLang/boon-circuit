use crate::{
    AuthoritativeCallableSignature, AuthoritativeParameter, BuiltinSignatureRegistry,
    ContextualBuiltinKind, RenderContractRegistry, SourcePayloadPathLookup, TypecheckSyntaxProgram,
    checked_intrinsic_v1, function_param_requirements, host_effect_signature, host_port_table,
    merge_external_function_param_requirements, scene_root, session_info_intrinsic_type,
    source_payload_shape_table, syntax_source_sites,
};
use boon_checked::{
    CheckedCallContextKind, CheckedCallableKind, CheckedEffectSummary,
    CheckedExternalDeclarationIdentityV1, CheckedIntrinsicV1, CheckedParameterKind,
    CheckedParameterRequirement, ExternalTypeEnvironment, FlowType, ProgramRole, Type,
};
use boon_parser::ProjectSyntaxSnapshot;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const OWNER_ABI_ENVIRONMENT_DOMAIN_V1: &[u8] = b"boon.owner-abi-environment.v1\0";
const OWNER_CALLABLE_ABI_ENVIRONMENT_DOMAIN_V1: &[u8] = b"boon.owner-callable-abi-environment.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerAbiEnvironmentError {
    message: String,
}

impl OwnerAbiEnvironmentError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerAbiEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerAbiEnvironmentError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerAbiRenderRoot {
    Document,
    Scene,
}

impl OwnerAbiRenderRoot {
    const fn name(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Scene => "scene",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiPolicy {
    pub allow_unresolved_external: bool,
    pub require_resolved_external_identities: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerAbiEvaluationScope {
    Parent,
    Output { parameter_ordinal: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiParameterContract {
    pub name: String,
    pub kind: CheckedParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: OwnerAbiEvaluationScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiCallContextContract {
    pub name: String,
    pub kind: CheckedCallContextKind,
    pub provider_parameter_ordinal: u32,
    pub flow_type: FlowType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerAbiContextualOperation {
    Map {
        list: u32,
        row: u32,
        body: u32,
    },
    Filter {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Retain {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Remove {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Every {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Any {
        list: u32,
        row: u32,
        predicate: u32,
    },
    Find {
        list: u32,
        row: u32,
        predicate: u32,
    },
    SortBy {
        list: u32,
        row: u32,
        key: u32,
        direction: u32,
    },
    ThenBy {
        list: u32,
        row: u32,
        key: u32,
        direction: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiCallableContract {
    pub name: String,
    pub kind: CheckedCallableKind,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub parameters: Box<[OwnerAbiParameterContract]>,
    pub contexts: Box<[OwnerAbiCallContextContract]>,
    pub result: FlowType,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    pub contextual_operation: Option<OwnerAbiContextualOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiValueContract {
    pub canonical_path: String,
    pub flow_type: FlowType,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiSourcePayloadContract {
    pub canonical_path: String,
    pub payload_type: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiNamedTypeRequirement {
    pub name: String,
    pub ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerAbiLocalFunctionRequirement {
    pub function: String,
    pub parameters: Box<[OwnerAbiNamedTypeRequirement]>,
}

/// Frozen authoritative input shared by interface and owner-body requests.
///
/// This is the only owner path representation of builtin, render, host,
/// distributed/external, source-payload, and project policy contracts. It is
/// span-free, canonically ordered, and fingerprints complete contracts rather
/// than merely the authoritative names an owner happens to mention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerAbiEnvironment {
    pub role: ProgramRole,
    pub active_render_root: OwnerAbiRenderRoot,
    pub policy: OwnerAbiPolicy,
    pub callables: Box<[OwnerAbiCallableContract]>,
    pub values: Box<[OwnerAbiValueContract]>,
    pub source_payloads: Box<[OwnerAbiSourcePayloadContract]>,
    pub local_function_requirements: Box<[OwnerAbiLocalFunctionRequirement]>,
    fingerprint_v1: [u8; 32],
}

/// Backdatable projection consumed by call resolution and inference.
///
/// Project-local parameter requirements, values, and source payload shapes do
/// not invalidate call consumers until those consumers explicitly request the
/// corresponding ABI projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCallableAbiEnvironment {
    pub role: ProgramRole,
    pub active_render_root: OwnerAbiRenderRoot,
    pub policy: OwnerAbiPolicy,
    pub callables: Box<[OwnerAbiCallableContract]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerCallableAbiEnvironment {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn callable(&self, name: &str) -> Option<&OwnerAbiCallableContract> {
        self.callables
            .binary_search_by(|contract| contract.name.as_str().cmp(name))
            .ok()
            .and_then(|index| self.callables.get(index))
    }
}

impl OwnerAbiEnvironment {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn callable(&self, name: &str) -> Option<&OwnerAbiCallableContract> {
        self.callables
            .binary_search_by(|contract| contract.name.as_str().cmp(name))
            .ok()
            .and_then(|index| self.callables.get(index))
    }

    pub fn value(&self, canonical_path: &str) -> Option<&OwnerAbiValueContract> {
        self.values
            .binary_search_by(|contract| contract.canonical_path.as_str().cmp(canonical_path))
            .ok()
            .and_then(|index| self.values.get(index))
    }

    pub fn source_payload(&self, canonical_path: &str) -> Option<&OwnerAbiSourcePayloadContract> {
        self.source_payloads
            .binary_search_by(|contract| contract.canonical_path.as_str().cmp(canonical_path))
            .ok()
            .and_then(|index| self.source_payloads.get(index))
    }

    pub fn callable_environment(
        &self,
    ) -> Result<OwnerCallableAbiEnvironment, OwnerAbiEnvironmentError> {
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_CALLABLE_ABI_ENVIRONMENT_DOMAIN_V1,
            &(
                self.role,
                self.active_render_root,
                self.policy,
                &self.callables,
            ),
        )
        .map_err(|error| {
            OwnerAbiEnvironmentError::new(format!(
                "cannot fingerprint owner callable ABI environment: {error}"
            ))
        })?;
        Ok(OwnerCallableAbiEnvironment {
            role: self.role,
            active_render_root: self.active_render_root,
            policy: self.policy,
            callables: self.callables.clone(),
            fingerprint_v1,
        })
    }
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerAbiEnvironmentError> {
    u32::try_from(value)
        .map_err(|_| OwnerAbiEnvironmentError::new(format!("{context} exceeds u32")))
}

fn parameter_ordinal(
    parameters: &[OwnerAbiParameterContract],
    name: &str,
    callable: &str,
) -> Result<u32, OwnerAbiEnvironmentError> {
    parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .map(|parameter| parameter.ordinal)
        .ok_or_else(|| {
            OwnerAbiEnvironmentError::new(format!(
                "authoritative callable `{callable}` is missing `{name}`"
            ))
        })
}

fn contextual_operation(
    callable: &str,
    kind: ContextualBuiltinKind,
    parameters: &[OwnerAbiParameterContract],
) -> Result<OwnerAbiContextualOperation, OwnerAbiEnvironmentError> {
    let list = parameter_ordinal(parameters, "list", callable)?;
    let row = parameter_ordinal(parameters, "item", callable)?;
    Ok(match kind {
        ContextualBuiltinKind::Map => OwnerAbiContextualOperation::Map {
            list,
            row,
            body: parameter_ordinal(parameters, "new", callable)?,
        },
        ContextualBuiltinKind::Filter => OwnerAbiContextualOperation::Filter {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Retain => OwnerAbiContextualOperation::Retain {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Remove => OwnerAbiContextualOperation::Remove {
            list,
            row,
            predicate: parameter_ordinal(parameters, "when", callable)?,
        },
        ContextualBuiltinKind::Every => OwnerAbiContextualOperation::Every {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Any => OwnerAbiContextualOperation::Any {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::Find => OwnerAbiContextualOperation::Find {
            list,
            row,
            predicate: parameter_ordinal(parameters, "if", callable)?,
        },
        ContextualBuiltinKind::SortBy => OwnerAbiContextualOperation::SortBy {
            list,
            row,
            key: parameter_ordinal(parameters, "key", callable)?,
            direction: parameter_ordinal(parameters, "direction", callable)?,
        },
        ContextualBuiltinKind::ThenBy => OwnerAbiContextualOperation::ThenBy {
            list,
            row,
            key: parameter_ordinal(parameters, "key", callable)?,
            direction: parameter_ordinal(parameters, "direction", callable)?,
        },
    })
}

fn callable_contract(
    name: String,
    kind: CheckedCallableKind,
    signature: AuthoritativeCallableSignature,
    role: ProgramRole,
    external_identity: Option<CheckedExternalDeclarationIdentityV1>,
) -> Result<OwnerAbiCallableContract, OwnerAbiEnvironmentError> {
    let AuthoritativeCallableSignature {
        parameters,
        call_contexts,
        result,
        effect,
        contextual_builtin,
    } = signature;
    let output_ordinal = parameters
        .iter()
        .position(|parameter| parameter.kind == CheckedParameterKind::Out)
        .map(|ordinal| checked_u32(ordinal, "authoritative output parameter ordinal"))
        .transpose()?;
    let parameters = parameters
        .into_iter()
        .enumerate()
        .map(|(ordinal, parameter)| {
            let AuthoritativeParameter {
                name,
                kind,
                flow_type,
                requirement,
            } = parameter;
            let ordinal = checked_u32(ordinal, "authoritative parameter ordinal")?;
            let evaluation_scope = if kind == CheckedParameterKind::Value
                && matches!(name.as_str(), "new" | "if" | "when" | "key")
            {
                output_ordinal.map_or(OwnerAbiEvaluationScope::Parent, |parameter_ordinal| {
                    OwnerAbiEvaluationScope::Output { parameter_ordinal }
                })
            } else {
                OwnerAbiEvaluationScope::Parent
            };
            Ok(OwnerAbiParameterContract {
                name,
                kind,
                ordinal,
                flow_type,
                requirement,
                evaluation_scope,
            })
        })
        .collect::<Result<Vec<_>, OwnerAbiEnvironmentError>>()?;
    let contexts = call_contexts
        .into_iter()
        .map(|context| {
            Ok(OwnerAbiCallContextContract {
                provider_parameter_ordinal: parameter_ordinal(
                    &parameters,
                    &context.provider,
                    &name,
                )?,
                name: context.name,
                kind: context.kind,
                flow_type: context.flow_type,
            })
        })
        .collect::<Result<Vec<_>, OwnerAbiEnvironmentError>>()?;
    let contextual_operation = contextual_builtin
        .map(|kind| contextual_operation(&name, kind, &parameters))
        .transpose()?;
    Ok(OwnerAbiCallableContract {
        intrinsic: checked_intrinsic_v1(kind, &name),
        name,
        kind,
        external_identity,
        parameters: parameters.into_boxed_slice(),
        contexts: contexts.into_boxed_slice(),
        result,
        role,
        effect,
        contextual_operation,
    })
}

fn provisional_external_signatures(
    program: &TypecheckSyntaxProgram,
    external_types: &ExternalTypeEnvironment,
) -> BTreeMap<String, Vec<String>> {
    if !external_types.allow_unresolved {
        return BTreeMap::new();
    }
    let mut provisional = BTreeMap::new();
    for expression in program.expressions().iter() {
        let (function, arguments) = match &expression.kind {
            boon_syntax::AstExprKind::Call { function, args, .. }
            | boon_syntax::AstExprKind::Pipe {
                op: function, args, ..
            } => (function, args),
            _ => continue,
        };
        if crate::external_function_role(function).is_none()
            || external_types.functions.contains_key(function)
        {
            continue;
        }
        let Some(names) = arguments
            .iter()
            .map(|argument| argument.named_name().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        provisional.entry(function.clone()).or_insert(names);
    }
    provisional
}

fn field_projection_names(program: &TypecheckSyntaxProgram) -> BTreeSet<String> {
    program
        .expressions()
        .iter()
        .filter_map(|expression| match &expression.kind {
            boon_syntax::AstExprKind::Call { function, .. }
            | boon_syntax::AstExprKind::Pipe { op: function, .. } => {
                function.strip_prefix("Field/").map(str::to_owned)
            }
            _ => None,
        })
        .collect()
}

fn insert_callable(
    callables: &mut BTreeMap<String, OwnerAbiCallableContract>,
    name: &str,
    kind: CheckedCallableKind,
    signature: AuthoritativeCallableSignature,
    role: ProgramRole,
    external_identity: Option<CheckedExternalDeclarationIdentityV1>,
) -> Result<(), OwnerAbiEnvironmentError> {
    if callables.contains_key(name) {
        return Ok(());
    }
    let contract = callable_contract(name.to_owned(), kind, signature, role, external_identity)?;
    callables.insert(name.to_owned(), contract);
    Ok(())
}

pub fn project_owner_abi_environment(
    project: &ProjectSyntaxSnapshot,
    external_types: &ExternalTypeEnvironment,
) -> Result<OwnerAbiEnvironment, OwnerAbiEnvironmentError> {
    let program = TypecheckSyntaxProgram::UnitNative(project.clone());
    let active_render_root = if scene_root(&program).is_some() {
        OwnerAbiRenderRoot::Scene
    } else {
        OwnerAbiRenderRoot::Document
    };
    let render = RenderContractRegistry::default().with_active_root(active_render_root.name());
    let builtins = BuiltinSignatureRegistry::default();
    let role = external_types.current_role;
    let mut callables = BTreeMap::new();

    for (name, mut signature) in builtins.authoritative_signatures() {
        if host_effect_signature(name).is_some() {
            continue;
        }
        if boon_effect_schema::host_effect_spec(name).is_some() {
            signature.effect.invokes_host = true;
        }
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            signature,
            role,
            None,
        )?;
    }
    for name in ["Stream/pulses", "Stream/skip"] {
        if let Some(signature) = builtins.authoritative_signature(name) {
            insert_callable(
                &mut callables,
                name,
                CheckedCallableKind::Builtin,
                signature,
                role,
                None,
            )?;
        }
    }
    for name in ["SessionInfo/status", "SessionInfo/principal"] {
        let result = session_info_intrinsic_type(name).ok_or_else(|| {
            OwnerAbiEnvironmentError::new(format!("missing SessionInfo intrinsic `{name}`"))
        })?;
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            AuthoritativeCallableSignature {
                parameters: Vec::new(),
                call_contexts: Vec::new(),
                result: crate::continuous_flow_type(result),
                effect: CheckedEffectSummary::default(),
                contextual_builtin: None,
            },
            role,
            None,
        )?;
    }
    for (name, signature) in render.authoritative_signatures() {
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            signature,
            role,
            None,
        )?;
    }
    for name in boon_effect_schema::HOST_EFFECT_OPERATIONS {
        let Some(signature) = host_effect_signature(name) else {
            continue;
        };
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::Builtin,
            AuthoritativeCallableSignature {
                parameters: signature
                    .intent_fields
                    .into_iter()
                    .map(|field| AuthoritativeParameter {
                        name: field.name,
                        kind: CheckedParameterKind::Value,
                        flow_type: crate::continuous_flow_type(field.ty),
                        requirement: field
                            .default
                            .map_or(CheckedParameterRequirement::Required, |default| {
                                CheckedParameterRequirement::Optional { default }
                            }),
                    })
                    .collect(),
                call_contexts: Vec::new(),
                result: crate::continuous_flow_type(signature.result_type),
                effect: CheckedEffectSummary {
                    invokes_host: true,
                    ..CheckedEffectSummary::default()
                },
                contextual_builtin: None,
            },
            role,
            None,
        )?;
    }
    for field in field_projection_names(&program) {
        let name = format!("Field/{field}");
        insert_callable(
            &mut callables,
            &name,
            CheckedCallableKind::Builtin,
            AuthoritativeCallableSignature {
                parameters: vec![AuthoritativeParameter {
                    name: "input".to_owned(),
                    kind: CheckedParameterKind::Value,
                    flow_type: crate::unknown_flow_type(),
                    requirement: CheckedParameterRequirement::Required,
                }],
                call_contexts: Vec::new(),
                result: crate::unknown_flow_type(),
                effect: CheckedEffectSummary::default(),
                contextual_builtin: None,
            },
            role,
            None,
        )?;
    }
    for (name, function) in &external_types.functions {
        insert_callable(
            &mut callables,
            name,
            CheckedCallableKind::External,
            AuthoritativeCallableSignature {
                parameters: function
                    .args
                    .iter()
                    .map(|argument| AuthoritativeParameter {
                        name: argument.name.clone(),
                        kind: CheckedParameterKind::Value,
                        flow_type: argument.flow_type.clone(),
                        requirement: CheckedParameterRequirement::Required,
                    })
                    .collect(),
                call_contexts: Vec::new(),
                result: function.result.clone(),
                effect: function.effect,
                contextual_builtin: None,
            },
            role,
            external_types.external_identities.get(name).copied(),
        )?;
    }
    for (name, arguments) in provisional_external_signatures(&program, external_types) {
        insert_callable(
            &mut callables,
            &name,
            CheckedCallableKind::External,
            AuthoritativeCallableSignature {
                parameters: arguments
                    .into_iter()
                    .map(|name| AuthoritativeParameter {
                        name,
                        kind: CheckedParameterKind::Value,
                        flow_type: crate::unknown_flow_type(),
                        requirement: CheckedParameterRequirement::Required,
                    })
                    .collect(),
                call_contexts: Vec::new(),
                result: crate::unknown_flow_type(),
                effect: CheckedEffectSummary::default(),
                contextual_builtin: None,
            },
            role,
            external_types.external_identities.get(&name).copied(),
        )?;
    }

    let values = external_types
        .values
        .iter()
        .map(|(canonical_path, flow_type)| OwnerAbiValueContract {
            canonical_path: canonical_path.clone(),
            flow_type: flow_type.clone(),
            external_identity: external_types
                .external_identities
                .get(canonical_path)
                .copied(),
        })
        .collect::<Vec<_>>();

    let source_sites = syntax_source_sites(&program);
    let source_paths = source_sites
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    let source_payload_lookup = SourcePayloadPathLookup::new(&source_paths);
    let (host_ports, _) = host_port_table(&program, &source_payload_lookup);
    let source_payloads =
        source_payload_shape_table(&program, &source_sites, &source_payload_lookup, &host_ports)
            .into_iter()
            .map(|payload| OwnerAbiSourcePayloadContract {
                canonical_path: payload.source_path,
                payload_type: payload.payload_type,
            })
            .collect::<Vec<_>>();

    let mut local_requirements = function_param_requirements(&program);
    merge_external_function_param_requirements(
        &mut local_requirements,
        &external_types.local_function_requirements,
    );
    let local_function_requirements = local_requirements
        .into_iter()
        .map(|(function, parameters)| OwnerAbiLocalFunctionRequirement {
            function,
            parameters: parameters
                .into_iter()
                .map(|(name, ty)| OwnerAbiNamedTypeRequirement { name, ty })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>();

    let callables = callables.into_values().collect::<Vec<_>>();
    let policy = OwnerAbiPolicy {
        allow_unresolved_external: external_types.allow_unresolved,
        require_resolved_external_identities: external_types.require_resolved_identities,
    };
    let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
        OWNER_ABI_ENVIRONMENT_DOMAIN_V1,
        &(
            role,
            active_render_root,
            policy,
            &callables,
            &values,
            &source_payloads,
            &local_function_requirements,
        ),
    )
    .map_err(|error| {
        OwnerAbiEnvironmentError::new(format!("cannot fingerprint owner ABI environment: {error}"))
    })?;
    Ok(OwnerAbiEnvironment {
        role,
        active_render_root,
        policy,
        callables: callables.into_boxed_slice(),
        values: values.into_boxed_slice(),
        source_payloads: source_payloads.into_boxed_slice(),
        local_function_requirements: local_function_requirements.into_boxed_slice(),
        fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_parser::{parse_project_source_unit, project_unit_link_keys};
    use std::sync::Arc;

    fn project(source: &str) -> ProjectSyntaxSnapshot {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let source_unit_id = parsed.source_unit_id.clone();
        let link_key = project_unit_link_keys(
            "app/RUN.bn",
            [(source_unit_id.clone(), parsed.declared_functions.clone())],
        )
        .unwrap()
        .remove(&source_unit_id)
        .unwrap();
        let unit = parsed.into_unit_syntax_snapshot(link_key).unwrap();
        ProjectSyntaxSnapshot::from_unit_snapshots("app/RUN.bn", vec![Arc::new(unit)]).unwrap()
    }

    #[test]
    fn abi_environment_is_canonical_complete_and_role_sensitive() {
        let project = project("document: Document/new(title: TEXT { Demo })\n");
        let client = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let second = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let server = project_owner_abi_environment(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Server),
        )
        .unwrap();

        assert_eq!(client, second);
        assert_ne!(client.fingerprint_v1(), server.fingerprint_v1());
        assert_eq!(client.active_render_root, OwnerAbiRenderRoot::Document);
        let map = client.callable("List/map").unwrap();
        assert!(matches!(
            map.contextual_operation,
            Some(OwnerAbiContextualOperation::Map { .. })
        ));
        assert!(map.parameters.iter().any(|parameter| matches!(
            parameter.evaluation_scope,
            OwnerAbiEvaluationScope::Output { .. }
        )));
        assert!(client.callable("File/read_stream").is_some());
        assert!(client.callable("Document/new").is_some());
    }

    #[test]
    fn active_render_root_changes_render_contract_identity() {
        let document = project("document: Document/new(title: TEXT { Demo })\n");
        let scene = project("scene: Scene/new()\n");
        let document = project_owner_abi_environment(
            &document,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        let scene = project_owner_abi_environment(
            &scene,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        )
        .unwrap();
        assert_eq!(scene.active_render_root, OwnerAbiRenderRoot::Scene);
        assert_ne!(document.fingerprint_v1(), scene.fingerprint_v1());
    }

    #[test]
    fn external_contracts_and_policies_are_fingerprint_inputs() {
        let project = project("value: 1\n");
        let mut first = ExternalTypeEnvironment::empty(ProgramRole::Client);
        first.values.insert(
            "Server.value".to_owned(),
            FlowType {
                mode: boon_checked::FlowMode::Continuous,
                ty: Type::Number,
            },
        );
        let first = project_owner_abi_environment(&project, &first).unwrap();
        let mut second = ExternalTypeEnvironment::empty(ProgramRole::Client);
        second.allow_unresolved = true;
        second.values.insert(
            "Server.value".to_owned(),
            FlowType {
                mode: boon_checked::FlowMode::Continuous,
                ty: Type::Text,
            },
        );
        let second = project_owner_abi_environment(&project, &second).unwrap();
        assert_ne!(first.fingerprint_v1(), second.fingerprint_v1());
        assert_eq!(
            first.value("Server.value").unwrap().flow_type.ty,
            Type::Number
        );
        assert_eq!(
            second.value("Server.value").unwrap().flow_type.ty,
            Type::Text
        );
    }
}
