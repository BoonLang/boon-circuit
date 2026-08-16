use crate::{
    KernelCallableKind, KernelOwnerBuildError, KernelParameterEvaluationScope,
    receipt::alpha_normalize_flow_type,
};
use boon_checked::{
    CheckedCallContextKind, CheckedEffectSummary, CheckedExternalDeclarationIdentityV1,
    CheckedIntrinsicV1, CheckedParameterKind, CheckedParameterRequirement, FlowType, ProgramRole,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Immutable compiler/library ABI consumed by one kernel revision.
///
/// These rows are independent of parser arenas and the legacy owner checker.
/// Definition solving already consumes their normalized call equations; this
/// table retains the exact checked-image identities and metadata needed by the
/// one-pass linker.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct KernelAbiInput {
    role: ProgramRole,
    callables: Box<[KernelCallableAbiInput]>,
}

impl KernelAbiInput {
    pub fn new(
        role: ProgramRole,
        callables: impl IntoIterator<Item = KernelCallableAbiInput>,
    ) -> Result<Self, KernelOwnerBuildError> {
        let mut callables = callables.into_iter().collect::<Vec<_>>();
        // Every callable owns an independent generic scheme. External ABI
        // producers may allocate TypeVar ordinals from a process-wide arena;
        // carrying those ordinals into the checked linker both wastes the
        // dense namespace and accidentally correlates unrelated callables.
        // Canonicalize once at the permanent kernel boundary.
        for callable in &mut callables {
            let mut variables = BTreeMap::new();
            let mut next = 0;
            for parameter in &mut callable.parameters {
                parameter.flow_type =
                    alpha_normalize_flow_type(&parameter.flow_type, &mut variables, &mut next);
            }
            for context in &mut callable.contexts {
                context.flow_type =
                    alpha_normalize_flow_type(&context.flow_type, &mut variables, &mut next);
            }
            callable.result =
                alpha_normalize_flow_type(&callable.result, &mut variables, &mut next);
        }
        callables.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        let mut names = BTreeSet::new();
        for callable in &callables {
            validate_callable(role, callable)?;
            if !names.insert(callable.name.as_ref()) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel ABI repeats callable `{}`",
                    callable.name,
                )));
            }
        }
        Ok(Self {
            role,
            callables: callables.into_boxed_slice(),
        })
    }

    pub fn empty(role: ProgramRole) -> Self {
        Self {
            role,
            callables: Box::new([]),
        }
    }

    pub const fn role(&self) -> ProgramRole {
        self.role
    }

    pub fn callables(&self) -> &[KernelCallableAbiInput] {
        &self.callables
    }

    pub fn callable(&self, name: &str) -> Option<&KernelCallableAbiInput> {
        self.callables
            .binary_search_by(|callable| callable.name.as_ref().cmp(name))
            .ok()
            .map(|index| &self.callables[index])
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KernelCallableAbiInput {
    pub name: Box<str>,
    pub kind: KernelCallableKind,
    pub intrinsic: Option<CheckedIntrinsicV1>,
    pub external_identity: Option<CheckedExternalDeclarationIdentityV1>,
    pub parameters: Box<[KernelAbiParameterInput]>,
    pub contexts: Box<[KernelAbiCallContextInput]>,
    pub result: FlowType,
    pub result_specialization: KernelAbiResultSpecialization,
    pub role: ProgramRole,
    pub effect: CheckedEffectSummary,
    pub contextual_operation: Option<KernelAbiContextualOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KernelAbiParameterInput {
    pub name: Box<str>,
    pub kind: CheckedParameterKind,
    pub ordinal: u32,
    pub flow_type: FlowType,
    pub requirement: CheckedParameterRequirement,
    pub evaluation_scope: KernelParameterEvaluationScope,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KernelAbiCallContextInput {
    pub name: Box<str>,
    pub kind: CheckedCallContextKind,
    pub provider_parameter_ordinal: u32,
    pub flow_type: FlowType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum KernelAbiResultSpecialization {
    Fixed,
    RenderConstructor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum KernelAbiContextualOperation {
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

impl KernelAbiContextualOperation {
    fn parameter_ordinals(self) -> Box<[u32]> {
        match self {
            Self::Map { list, row, body } => Box::new([list, row, body]),
            Self::Filter {
                list,
                row,
                predicate,
            }
            | Self::Retain {
                list,
                row,
                predicate,
            }
            | Self::Remove {
                list,
                row,
                predicate,
            }
            | Self::Every {
                list,
                row,
                predicate,
            }
            | Self::Any {
                list,
                row,
                predicate,
            }
            | Self::Find {
                list,
                row,
                predicate,
            } => Box::new([list, row, predicate]),
            Self::SortBy {
                list,
                row,
                key,
                direction,
            }
            | Self::ThenBy {
                list,
                row,
                key,
                direction,
            } => Box::new([list, row, key, direction]),
        }
    }
}

fn validate_callable(
    role: ProgramRole,
    callable: &KernelCallableAbiInput,
) -> Result<(), KernelOwnerBuildError> {
    if callable.name.is_empty() {
        return Err(KernelOwnerBuildError::new(
            "kernel ABI contains an empty callable name",
        ));
    }
    if callable.kind == KernelCallableKind::User {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel ABI callable `{}` cannot use the user-definition kind",
            callable.name,
        )));
    }
    if callable.role != role {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel ABI callable `{}` belongs to {:?} inside a {:?} ABI",
            callable.name, callable.role, role,
        )));
    }
    let mut parameter_names = BTreeSet::new();
    for (expected, parameter) in callable.parameters.iter().enumerate() {
        if parameter.ordinal as usize != expected {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel ABI callable `{}` parameter `{}` has ordinal {} instead of {expected}",
                callable.name, parameter.name, parameter.ordinal,
            )));
        }
        if parameter.name.is_empty() || !parameter_names.insert(parameter.name.as_ref()) {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel ABI callable `{}` has an empty or repeated parameter `{}`",
                callable.name, parameter.name,
            )));
        }
        if let KernelParameterEvaluationScope::Output { parameter_ordinal } =
            parameter.evaluation_scope
        {
            let Some(output) = callable.parameters.get(parameter_ordinal as usize) else {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel ABI callable `{}` parameter `{}` targets missing OUT ordinal {parameter_ordinal}",
                    callable.name, parameter.name,
                )));
            };
            if output.kind != CheckedParameterKind::Out {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel ABI callable `{}` parameter `{}` targets non-OUT parameter `{}`",
                    callable.name, parameter.name, output.name,
                )));
            }
        }
    }
    let mut context_names = BTreeSet::new();
    for context in &callable.contexts {
        let Some(_) = callable
            .parameters
            .get(context.provider_parameter_ordinal as usize)
        else {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel ABI callable `{}` context `{}` targets missing provider ordinal {}",
                callable.name, context.name, context.provider_parameter_ordinal,
            )));
        };
        if context.name.is_empty() || !context_names.insert(context.name.as_ref()) {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel ABI callable `{}` has an empty or repeated context `{}`",
                callable.name, context.name,
            )));
        }
    }
    if let Some(operation) = callable.contextual_operation {
        for ordinal in operation.parameter_ordinals() {
            if ordinal as usize >= callable.parameters.len() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel ABI callable `{}` contextual operation references missing parameter ordinal {ordinal}",
                    callable.name,
                )));
            }
        }
    }
    Ok(())
}
