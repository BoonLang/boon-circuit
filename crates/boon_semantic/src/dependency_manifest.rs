//! Exhaustive callable dependency proof construction and compact sealing.
//!
//! The proof is derived only after every semantic graph is final. Checked IDs
//! are retained as source provenance; semantic IDs and explicit child ordinals
//! carry executable authority. Every checked and semantic record instance
//! receives exactly one primary callable/root owner and exactly one coverage
//! disposition. Once validated, the full record inventories are discarded and
//! `SemanticProgram` retains only their digests and exact owner/callable
//! implementation identities.

use crate::out_net::{OutCallProvenance, OutInputValue, OutPortId, PassedBinding};
use crate::*;
use boon_checked::{
    CheckedCallEntry, CheckedDeclarationKind, CheckedEvaluationScope, CheckedExpressionKind,
    CheckedParameterKind, CheckedProgram, CheckedStatementKind, DeclId, FlowMode, FlowType,
    LexicalScopeId, ProgramRole, Type, TypeVar,
};
use boon_compilation_db::{RequestGraphBuilder, RequestGraphDigestDomains};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::VecDeque;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

#[cfg(test)]
pub const CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V3: &str = "boon.callable-dependency-manifest.v3";
pub const CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V4: &str = "boon.callable-dependency-manifest.v4";
const CHECKED_PROGRAM_DIGEST_DOMAIN: &[u8] = b"boon.checked-program.v1\0";
const DEPENDENCY_COMPONENT_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-components.v1\0";
const DEPENDENCY_RECORD_PAYLOAD_DOMAIN: &[u8] = b"boon.callable-dependency-record-payload.v1\0";
const DEPENDENCY_OUT_TYPE_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-out-type.v1\0";
const DEPENDENCY_FLOW_TYPE_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-flow-type.v1\0";
#[cfg(test)]
const DEPENDENCY_RECORD_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-record.v1\0";
#[cfg(test)]
const DEPENDENCY_RECORD_SET_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-record-set.v2\0";
#[cfg(test)]
const DEPENDENCY_COVERAGE_SET_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-coverage-set.v1\0";
#[cfg(test)]
const DEPENDENCY_CALLABLE_SET_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-callable-set.v3\0";
const DEPENDENCY_PUBLIC_SHAPE_DOMAIN: &[u8] = b"boon.callable-dependency-public-shape.v1\0";
#[cfg(test)]
const DEPENDENCY_IMPLEMENTATION_OWNER_DOMAIN: &[u8] =
    b"boon.callable-dependency-implementation-owner.v2\0";
#[cfg(test)]
const DEPENDENCY_IMPLEMENTATION_COMPONENT_DOMAIN: &[u8] =
    b"boon.callable-dependency-implementation-component.v2\0";
#[cfg(test)]
const DEPENDENCY_IMPLEMENTATION_DOMAIN: &[u8] = b"boon.callable-dependency-implementation.v2\0";
#[cfg(test)]
const DEPENDENCY_MANIFEST_DIGEST_DOMAIN: &[u8] = b"boon.callable-dependency-manifest.v3\0";
#[cfg(test)]
const DEPENDENCY_PROGRAM_ROOT_ENTRY_DIGEST_DOMAIN: &[u8] =
    b"boon.callable-dependency-program-root-entry.v3\0";
#[cfg(test)]
const DEPENDENCY_SEALED_MANIFEST_DIGEST_DOMAIN: &[u8] =
    b"boon.callable-dependency-sealed-manifest.v1\0";
const DEPENDENCY_STABLE_OWNER_DOMAIN_V4: &[u8] = b"boon.callable-dependency-stable-owner.v4\0";
const DEPENDENCY_PROJECTION_KEY_DOMAIN_V4: &[u8] = b"boon.callable-dependency-projection-key.v4\0";
const DEPENDENCY_PROJECTION_LOCAL_ROW_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-projection-local-row.v4\0";
const DEPENDENCY_PROJECTION_ROW_DOMAIN_V4: &[u8] = b"boon.callable-dependency-projection-row.v4\0";
const DEPENDENCY_PROJECTION_RECEIPT_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-projection-receipt.v4\0";
const DEPENDENCY_PROJECTION_RECEIPT_SET_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-projection-receipt-set.v4\0";
const DEPENDENCY_ROW_RECEIPT_SET_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-row-receipt-set.v4\0";
const DEPENDENCY_COVERAGE_RECEIPT_SET_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-coverage-receipt-set.v4\0";
const DEPENDENCY_PROJECTION_NODE_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-projection-node.v4\0";
const DEPENDENCY_PROJECTION_COMPONENT_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-projection-component.v4\0";
const DEPENDENCY_PROJECTION_IMPLEMENTATION_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-projection-implementation.v4\0";
const DEPENDENCY_CALLABLE_SET_DOMAIN_V4: &[u8] = b"boon.callable-dependency-callable-set.v4\0";
const DEPENDENCY_PROGRAM_ROOT_ENTRY_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-program-root-entry.v4\0";
const DEPENDENCY_MANIFEST_DIGEST_DOMAIN_V4: &[u8] = b"boon.callable-dependency-manifest.v4\0";
const DEPENDENCY_SEALED_MANIFEST_DOMAIN_V4: &[u8] =
    b"boon.callable-dependency-sealed-manifest.v4\0";

macro_rules! dependency_id {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(
                Clone,
                Copy,
                Debug,
                Eq,
                Hash,
                Ord,
                PartialEq,
                PartialOrd,
                Serialize,
                Deserialize,
            )]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub const fn as_usize(self) -> usize {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(formatter)
                }
            }
        )+
    };
}

dependency_id!(SemanticDependencyRecordId, SemanticDependencyCoverageId);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyOwnerV1 {
    ProgramRoot,
    Callable { callable: SemanticCallableId },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyChannelV1 {
    ParentValueFormal,
    PassedContextFormal,
    OutFormal,
    OutputEvaluatedFormal,
    CompilerContext,
    LexicalCapture,
    LocalFact,
    ResourceRead,
    ResourceWrite,
    ResourceBehavior,
    CalledCallable,
    NormalizedDefault,
    ExternalValueOrCall,
    RuntimeIntrinsicOrHostEffect,
    MigrationPredecessor,
    PersistenceActivation,
    TypeAndFlowInstance,
    StructuralRepresentation,
    SemanticProfile,
    AssuranceInput,
    CoverageRouting,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyRoleV1 {
    FormulaBinder,
    FixedDefinition,
    ResourceOrProviderBehavior,
    CoverageOrRouting,
    AssuranceOrActivation,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyVisibilityV1 {
    Public,
    Private,
    Diagnostic,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyMultiplicityV1 {
    Once,
    PerTick,
    PerEvent,
    PerRowMaterialization,
    PerTransition,
    PerActivation,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyLifetimeV1 {
    Definition,
    Call,
    Snapshot,
    Event,
    Row,
    Activation,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyPhaseV1 {
    Definition,
    CurrentValue,
    PreviousCommittedValue,
    CandidateWrite,
    Commit,
    EventPayload,
    EffectCompletion,
    PersistenceActivation,
}

/// Stable domain tag for one checked or semantic identity.
///
/// `Indexed` preserves the exact typed ID value under an explicit domain.
/// Composite records use a parent identity plus an exact child ordinal.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencyEntityDomainV1 {
    CheckedScope,
    CheckedDeclaration,
    CheckedStatement,
    CheckedExpression,
    CheckedCallable,
    CheckedContextFormal,
    CheckedCall,
    CheckedCallResultPath,
    CheckedOrderChain,
    CheckedPatternBinding,
    CheckedResourceProjection,
    CheckedSource,
    CheckedState,
    CheckedList,
    CheckedOccurrence,
    ProducerMaterialization,
    OutCallInstance,
    OutPort,
    OutNet,
    StaticOwner,
    SemanticScope,
    SemanticExpression,
    SemanticValue,
    SemanticStatement,
    SemanticCallable,
    SemanticCall,
    SemanticCallOccurrence,
    SemanticSource,
    SemanticState,
    SemanticActivation,
    SemanticPulseBatch,
    SemanticFunction,
    SemanticMaterialization,
    SemanticRowScope,
    SemanticList,
    SemanticValueListAuthority,
    SemanticResourceAlias,
    SemanticMaterializationBinding,
    SemanticListProjection,
    SemanticProducerResource,
    SemanticProducerInstance,
    SemanticField,
    SemanticBinding,
    SemanticRead,
    SemanticDependencyUse,
    SemanticTriggerArm,
    SemanticStateUpdateArm,
    SemanticExternalDependency,
    SemanticListMutation,
    SemanticDerivedValue,
    SemanticHostEffect,
    SemanticCapture,
    SemanticMigrationInput,
    SemanticViewRoot,
    SemanticViewNode,
    SemanticViewArgument,
    SemanticViewBinding,
    SemanticStorageLocal,
    SemanticStorageField,
    SemanticStorageCapture,
    SemanticStorageRowValue,
    SemanticStorageRowSourceProjection,
    SemanticStorageExternalReference,
    SemanticProducerResult,
    SemanticNamedValue,
    SemanticMemory,
    SemanticMigrationEdge,
    SourceUnit,
    SourceExpression,
    RenderSlot,
    SourcePayloadShape,
    OutputContract,
    HostPort,
    Diagnostic,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyEntityV1 {
    Program,
    Indexed {
        domain: SemanticDependencyEntityDomainV1,
        index: u64,
    },
    Composite {
        domain: SemanticDependencyEntityDomainV1,
        parent_index: u64,
        child_ordinal: usize,
    },
    Digest {
        domain: SemanticDependencyEntityDomainV1,
        digest: [u8; 32],
    },
}

impl SemanticDependencyEntityV1 {
    fn indexed(domain: SemanticDependencyEntityDomainV1, index: usize) -> Self {
        Self::Indexed {
            domain,
            index: index as u64,
        }
    }

    fn checked(domain: SemanticDependencyEntityDomainV1, index: u32) -> Self {
        Self::Indexed {
            domain,
            index: u64::from(index),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticDependencySubjectKindV1 {
    CheckedProgram,
    ProducerMaterializationTable,
    ResolvedOutGraph,
    ExecutionGraph,
    ResourceGraph,
    ReactiveGraph,
    LoweringContract,
    ViewBindingGraph,
    StorageGraph,
    MemoryGraph,
    CheckedScope,
    CheckedDeclaration,
    CheckedStatement,
    CheckedExpression,
    CheckedCallable,
    CheckedCallableParameter,
    CheckedCallableContext,
    CheckedContextFormal,
    CheckedCall,
    CheckedCallEntry,
    CheckedCallContext,
    CheckedContextSubstitution,
    CheckedTypeSubstitution,
    CheckedCallResultPath,
    CheckedOrderChain,
    CheckedOrderKey,
    CheckedPatternBinding,
    CheckedResourceProjection,
    CheckedSource,
    CheckedState,
    CheckedList,
    CheckedOccurrence,
    CheckedLoweringMetadata,
    CheckedLoweringSideTableEntry,
    CheckedNamedValueStatementSite,
    ProducerMaterializationRequest,
    OutCallInstance,
    OutInputBinding,
    OutPassedBinding,
    OutPort,
    OutNet,
    OutStructuralProducer,
    OutStaticOwner,
    ExecutionScope,
    ExecutionExpression,
    ExecutionExpressionOrigin,
    ExecutionStatement,
    ExecutionCallable,
    ExecutionCallableParameter,
    ExecutionCallableContext,
    ExecutionCall,
    ExecutionCallEntry,
    ExecutionCallContext,
    ExecutionCallOccurrence,
    ExecutionSource,
    ExecutionState,
    ExecutionRoot,
    ExecutionFunction,
    ExecutionFunctionParameter,
    ExecutionMaterialization,
    ExecutionStaticOwner,
    ResourceRowScope,
    ResourceList,
    ResourceValueListAuthority,
    ResourceSource,
    ResourceState,
    ResourceAlias,
    ResourceMaterializationBinding,
    ResourceListProjection,
    ResourceProducer,
    ReactiveProducerInstance,
    ReactiveProducerParameter,
    ReactiveField,
    ReactiveBinding,
    ReactiveRead,
    ReactiveDependencyUse,
    ReactiveCallSchedule,
    ReactiveActivation,
    ReactivePulseBatch,
    ReactiveDerivedValue,
    ReactiveTriggerArm,
    ReactiveStateUpdateArm,
    ReactiveListMutation,
    ReactiveDependencyEdge,
    ReactivePossibleCauses,
    ReactiveHostEffect,
    ReactiveOutput,
    ReactiveViewCapture,
    ReactiveMigrationInput,
    LoweringMetadata,
    LoweringSourceUnit,
    LoweringExpressionType,
    LoweringExpressionOccurrence,
    LoweringFunctionType,
    LoweringFunctionParameter,
    LoweringNamedValue,
    LoweringRenderSlot,
    LoweringSourcePayload,
    LoweringSourcePayloadField,
    LoweringDiagnostic,
    LoweringOutputContract,
    LoweringHostPort,
    ViewRoot,
    ViewNode,
    ViewArgument,
    ViewBinding,
    ViewBindingTarget,
    StorageOwner,
    StorageLocal,
    StorageLocalMember,
    StorageCapture,
    StorageField,
    StorageBinding,
    StorageSource,
    StorageRowValue,
    StorageRowSourceProjection,
    StorageExternalReference,
    StorageProducerResult,
    StorageNamedValue,
    Memory,
    MemoryLeaf,
    MigrationEdge,
    MigrationInput,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticDependencySubjectV1 {
    pub kind: SemanticDependencySubjectKindV1,
    pub identity: SemanticDependencyEntityV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SemanticDependencyEntityV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_ordinal: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencySemanticsV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_type: Option<FlowType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_scope: Option<CheckedEvaluationScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_role: Option<ProgramRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    pub multiplicity: SemanticDependencyMultiplicityV1,
    pub lifetime: SemanticDependencyLifetimeV1,
    pub phase: SemanticDependencyPhaseV1,
    pub visibility: SemanticDependencyVisibilityV1,
}

impl Default for SemanticDependencySemanticsV1 {
    fn default() -> Self {
        Self {
            projection: Vec::new(),
            flow_type: None,
            evaluation_scope: None,
            program_role: None,
            static_owner: None,
            call_instance: None,
            row: None,
            multiplicity: SemanticDependencyMultiplicityV1::Once,
            lifetime: SemanticDependencyLifetimeV1::Definition,
            phase: SemanticDependencyPhaseV1::Definition,
            visibility: SemanticDependencyVisibilityV1::Private,
        }
    }
}

/// Compact record-owned semantics.
///
/// Full structural types remain authoritative in the checked and semantic
/// graphs. Dependency records commit those facts by canonical digest instead
/// of cloning the same recursive type into every occurrence record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyRecordSemanticsV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_type_digest: Option<[u8; 32]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluation_scope: Option<CheckedEvaluationScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_role: Option<ProgramRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_instance: Option<OutCallInstanceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<SemanticRowBinding>,
    pub multiplicity: SemanticDependencyMultiplicityV1,
    pub lifetime: SemanticDependencyLifetimeV1,
    pub phase: SemanticDependencyPhaseV1,
    pub visibility: SemanticDependencyVisibilityV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyRecordV1 {
    pub id: SemanticDependencyRecordId,
    pub owner: SemanticDependencyOwnerV1,
    pub channel: SemanticDependencyChannelV1,
    pub roles: Vec<SemanticDependencyRoleV1>,
    pub subject: SemanticDependencySubjectV1,
    pub semantics: SemanticDependencyRecordSemanticsV1,
    pub payload_digest: [u8; 32],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_dependencies: Vec<SemanticDependencyRecordId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_owners: Vec<SemanticDependencyOwnerV1>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyCoverageDispositionV1 {
    Dependency {
        dependency: SemanticDependencyRecordId,
    },
    Structural {
        payload_digest: [u8; 32],
    },
    Diagnostic {
        payload_digest: [u8; 32],
    },
    IntentionallyNonsemantic {
        payload_digest: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDependencyCoverageV1 {
    pub id: SemanticDependencyCoverageId,
    pub subject: SemanticDependencySubjectV1,
    pub primary_owner: SemanticDependencyOwnerV1,
    pub disposition: SemanticDependencyCoverageDispositionV1,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgramRootDependencyEntryV3 {
    pub public_shape_digest: [u8; 32],
    pub implementation_dependency_digest: [u8; 32],
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyEntryV3 {
    pub callable: SemanticCallableId,
    pub checked_callable: DeclId,
    pub public_shape_digest: [u8; 32],
    pub implementation_dependency_digest: [u8; 32],
}

/// Digests of the exhaustive proof inventories discarded when the semantic
/// program is sealed.
///
/// These values retain the exact V3 proof identity without retaining hundreds
/// of thousands of dependency and coverage records in every compiled program.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyProofDigestsV1 {
    pub program_root_entry_digest: [u8; 32],
    pub callable_entries_digest: [u8; 32],
    pub dependency_records_digest: [u8; 32],
    pub coverage_digest: [u8; 32],
    pub dependency_record_count: usize,
    pub coverage_record_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyComponentDigestsV1 {
    pub producer_materializations: [u8; 32],
    pub resolved_out_graph: [u8; 32],
    pub execution_graph: [u8; 32],
    pub resource_graph: [u8; 32],
    pub reactive_graph: [u8; 32],
    pub lowering_contract: [u8; 32],
    pub view_binding_graph: [u8; 32],
    pub scope_storage_graph: [u8; 32],
    pub memory_graph: [u8; 32],
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyManifestV3 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub checked_program_digest: CheckedProgramDigestV1,
    pub dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    pub component_digests: CallableDependencyComponentDigestsV1,
    pub program_root: ProgramRootDependencyEntryV3,
    pub callable_entries: Vec<CallableDependencyEntryV3>,
    pub proof_digests: CallableDependencyProofDigestsV1,
    pub manifest_digest: CallableDependencyManifestDigestV1,
    sealed_manifest_digest: [u8; 32],
}

/// Revision-stable identity for one dependency owner inside a compiler project.
///
/// Dense checked and semantic IDs remain useful coordinates in one compiler
/// revision, but they are not cache keys: inserting an earlier declaration can
/// renumber them. The V4 proof graph therefore names its owners by the program
/// role and the callable's authored identity. The enclosing `CompilationDb`
/// supplies the project identity. The current language rejects duplicate
/// callable names across the program, so kind/name/external identity/role is a
/// complete authored callable key today. If local, nested, or overloaded names
/// become legal, their source-unit and declaration-path identity must join this
/// key before that language change lands.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyStableOwnerV4 {
    ProgramRoot { role: ProgramRole },
    Callable { identity: [u8; 32] },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDependencyProjectionClassV4 {
    Dependency {
        channel: SemanticDependencyChannelV1,
    },
    Structural,
    Diagnostic,
    IntentionallyNonsemantic,
}

/// Stable request key for the coarse-grained rows committed by the V4 proof.
///
/// Rows stay dense inside this projection. General graph nodes exist only for
/// owner/projection requests, so expression inventories do not become a
/// second graph scheduler.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticDependencyProjectionKeyV4 {
    pub owner: SemanticDependencyStableOwnerV4,
    pub subject_kind: SemanticDependencySubjectKindV1,
    pub class: SemanticDependencyProjectionClassV4,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgramRootDependencyEntryV4 {
    pub stable_owner: SemanticDependencyStableOwnerV4,
    pub public_shape_digest: [u8; 32],
    pub implementation_dependency_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyEntryV4 {
    /// Revision-local diagnostic coordinate. It is never a request key.
    pub callable: SemanticCallableId,
    /// Revision-local checked provenance. It is never a request key.
    pub checked_callable: DeclId,
    pub stable_owner: SemanticDependencyStableOwnerV4,
    pub public_shape_digest: [u8; 32],
    pub implementation_dependency_digest: [u8; 32],
}

/// Compact commitments for the exact subject inventory and its projection
/// graph. Counts make dropped or duplicated rows observable without retaining
/// the exhaustive V3 DTOs in production.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyProofDigestsV4 {
    pub program_root_entry_digest: [u8; 32],
    pub callable_entries_digest: [u8; 32],
    pub dependency_rows_digest: [u8; 32],
    pub coverage_receipts_digest: [u8; 32],
    pub projection_receipts_digest: [u8; 32],
    pub dependency_record_count: usize,
    pub coverage_record_count: usize,
    pub projection_count: usize,
    pub projection_edge_count: usize,
}

/// Production callable dependency proof.
///
/// V4 commits the same exhaustive classifier traversal as V3, but folds rows
/// into owner/projection receipts during construction. Only those projections
/// and their cross-projection edges participate in the proof SCC graph.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDependencyManifestV4 {
    pub schema: String,
    pub source_bundle_digest_v1: SourceBundleDigestV1,
    pub checked_program_digest: CheckedProgramDigestV1,
    pub dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    pub component_digests: CallableDependencyComponentDigestsV1,
    pub program_root: ProgramRootDependencyEntryV4,
    pub callable_entries: Vec<CallableDependencyEntryV4>,
    pub proof_digests: CallableDependencyProofDigestsV4,
    pub manifest_digest: CallableDependencyManifestDigestV1,
    sealed_manifest_digest: [u8; 32],
}

/// Test-only reconstruction of the exhaustive dependency proof.
///
/// Production commits each exhaustive inventory before dropping it and never
/// assembles this aggregate. Tests retain it as the deep-validation and
/// byte-for-byte parity oracle; it never becomes part of `SemanticProgram`.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct CallableDependencyProofManifestV3 {
    schema: String,
    source_bundle_digest_v1: SourceBundleDigestV1,
    checked_program_digest: CheckedProgramDigestV1,
    dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    component_digests: CallableDependencyComponentDigestsV1,
    program_root: ProgramRootDependencyProofEntryV3,
    callable_entries: Vec<CallableDependencyProofEntryV3>,
    dependencies: Vec<SemanticDependencyRecordV1>,
    coverage: Vec<SemanticDependencyCoverageV1>,
    manifest_digest: CallableDependencyManifestDigestV1,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ProgramRootDependencyProofEntryV3 {
    direct_dependency_ids: Vec<SemanticDependencyRecordId>,
    public_shape_digest: [u8; 32],
    implementation_dependency_digest: [u8; 32],
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CallableDependencyProofEntryV3 {
    callable: SemanticCallableId,
    checked_callable: DeclId,
    direct_dependency_ids: Vec<SemanticDependencyRecordId>,
    public_shape_digest: [u8; 32],
    implementation_dependency_digest: [u8; 32],
}

#[cfg(test)]
struct ValidatedCallableDependencyProofManifestV3 {
    manifest: CallableDependencyProofManifestV3,
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum ExhaustiveProofRetention {
    Retain,
}

/// Trusted output of the private collector and graph-commitment pipeline.
///
/// Production retains only compact proof commitments. Tests may ask the same
/// pipeline to retain exhaustive inventories for byte-for-byte parity and
/// adversarial validation without extending their production lifetime.
#[cfg(test)]
struct ValidatedCallableDependencyConstructionV3 {
    schema: String,
    source_bundle_digest_v1: SourceBundleDigestV1,
    checked_program_digest: CheckedProgramDigestV1,
    dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    component_digests: CallableDependencyComponentDigestsV1,
    program_root: ProgramRootDependencyProofEntryV3,
    callable_entries: Vec<CallableDependencyProofEntryV3>,
    proof_digests: CallableDependencyProofDigestsV1,
    manifest_digest: CallableDependencyManifestDigestV1,
    #[cfg(test)]
    retained_dependencies: Option<Vec<SemanticDependencyRecordV1>>,
    #[cfg(test)]
    retained_coverage: Option<Vec<SemanticDependencyCoverageV1>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableDependencyManifestError {
    message: String,
}

impl CallableDependencyManifestError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CallableDependencyManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CallableDependencyManifestError {}

#[derive(Clone, Debug)]
struct DependencyOwnerIndex {
    callable_by_checked: BTreeMap<DeclId, SemanticCallableId>,
    checked_scope_owner: BTreeMap<LexicalScopeId, SemanticDependencyOwnerV1>,
    checked_statement_owner: BTreeMap<boon_checked::CheckedStatementId, SemanticDependencyOwnerV1>,
    expression_owner: Vec<SemanticDependencyOwnerV1>,
    statement_owner: Vec<SemanticDependencyOwnerV1>,
    call_owner: Vec<SemanticDependencyOwnerV1>,
    out_call_owner: Vec<SemanticDependencyOwnerV1>,
    static_owner: BTreeMap<StaticOwnerId, SemanticDependencyOwnerV1>,
    source_owner: Vec<SemanticDependencyOwnerV1>,
    state_owner: Vec<SemanticDependencyOwnerV1>,
    list_owner: Vec<SemanticDependencyOwnerV1>,
    value_list_owner: Vec<SemanticDependencyOwnerV1>,
    binding_owner: Vec<SemanticDependencyOwnerV1>,
    storage_field_owner: Vec<SemanticDependencyOwnerV1>,
    memory_owner: Vec<SemanticDependencyOwnerV1>,
}

impl DependencyOwnerIndex {
    fn derive(
        checked: &CheckedProgram,
        out: &ResolvedOutGraph,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        memory: &SemanticMemoryGraphV1,
    ) -> Result<Self, CallableDependencyManifestError> {
        let mut callable_by_checked = BTreeMap::new();
        for (index, callable) in execution.callables.iter().enumerate() {
            if callable.id != SemanticCallableId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic callable {} is not dense index {index}",
                    callable.id
                )));
            }
            if let Some(previous) =
                callable_by_checked.insert(callable.checked_callable, callable.id)
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "checked callable {} maps to semantic callables {previous} and {}",
                    callable.checked_callable.0, callable.id
                )));
            }
        }
        let checked_callables = checked
            .callables
            .iter()
            .map(|callable| callable.decl_id)
            .collect::<BTreeSet<_>>();
        if callable_by_checked.keys().copied().collect::<BTreeSet<_>>() != checked_callables {
            return Err(CallableDependencyManifestError::new(
                "semantic callable ownership is not a bijection over checked callables",
            ));
        }

        let mut checked_scope_owner = BTreeMap::new();
        for scope in &checked.scopes {
            let mut current = Some(scope.id);
            let mut visited = BTreeSet::new();
            let owner = loop {
                let Some(scope_id) = current else {
                    break SemanticDependencyOwnerV1::ProgramRoot;
                };
                if !visited.insert(scope_id) {
                    return Err(CallableDependencyManifestError::new(format!(
                        "checked scope ownership cycles at scope {}",
                        scope_id.0
                    )));
                }
                let candidate = checked
                    .scopes
                    .iter()
                    .find(|candidate| candidate.id == scope_id)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "checked scope ownership references missing scope {}",
                            scope_id.0
                        ))
                    })?;
                if let Some(callable) = candidate
                    .owner
                    .and_then(|declaration| callable_by_checked.get(&declaration).copied())
                {
                    break SemanticDependencyOwnerV1::Callable { callable };
                }
                current = candidate.parent;
            };
            if checked_scope_owner.insert(scope.id, owner).is_some() {
                return Err(CallableDependencyManifestError::new(format!(
                    "checked scope {} appears more than once",
                    scope.id.0
                )));
            }
        }

        let scope_owner = |scope: LexicalScopeId| {
            checked_scope_owner.get(&scope).copied().ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing checked scope {}",
                    scope.0
                ))
            })
        };

        if execution.checked_expression_origins.len() != execution.expressions.len() {
            return Err(CallableDependencyManifestError::new(
                "semantic expression-origin table is not total",
            ));
        }
        let mut expression_owner = Vec::with_capacity(execution.expressions.len());
        for (index, expression) in execution.expressions.iter().enumerate() {
            if expression.id != SemanticExprId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic expression {} is not dense index {index}",
                    expression.id
                )));
            }
            let origin = execution
                .checked_expression_origins
                .get(index)
                .filter(|origin| origin.expression == expression.id)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "semantic expression {} has no exact checked origin",
                        expression.id
                    ))
                })?;
            expression_owner.push(scope_owner(origin.checked_scope)?);
        }

        let mut statement_owner = Vec::with_capacity(execution.statements.len());
        for (index, statement) in execution.statements.iter().enumerate() {
            if statement.id != SemanticStatementId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic statement {} is not dense index {index}",
                    statement.id
                )));
            }
            let scope = execution
                .scopes
                .get(statement.scope.as_usize())
                .filter(|scope| scope.id == statement.scope)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "semantic statement {} references missing scope {}",
                        statement.id, statement.scope
                    ))
                })?;
            statement_owner.push(scope_owner(scope.checked_scope)?);
        }

        let semantic_owner_for_checked = |owner: Option<DeclId>, context: &str| {
            owner.map_or(Ok(SemanticDependencyOwnerV1::ProgramRoot), |owner| {
                callable_by_checked
                    .get(&owner)
                    .copied()
                    .map(|callable| SemanticDependencyOwnerV1::Callable { callable })
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "{context} references checked owner {} with no semantic callable identity",
                            owner.0
                        ))
                    })
            })
        };

        let mut checked_statement_owner = BTreeMap::new();
        for statement in &checked.statements {
            let owner = scope_owner(statement.scope_id)?;
            if checked_statement_owner
                .insert(statement.id, owner)
                .is_some()
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "checked statement {} appears more than once",
                    statement.id.0
                )));
            }
        }

        let mut call_owner = Vec::with_capacity(execution.calls.len());
        for (index, call) in execution.calls.iter().enumerate() {
            if call.id != SemanticCallId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic call {} is not dense index {index}",
                    call.id
                )));
            }
            let owner = call
                .owner_callable
                .map_or(SemanticDependencyOwnerV1::ProgramRoot, |callable| {
                    SemanticDependencyOwnerV1::Callable { callable }
                });
            call_owner.push(owner);
        }

        let mut producer_root_owner = BTreeMap::new();
        for producer in out.producer_roots() {
            let owner = semantic_owner_for_checked(
                Some(producer.spec.callable),
                &format!("producer root {:?}", producer.spec.identity),
            )?;
            if producer_root_owner.insert(producer.call, owner).is_some() {
                return Err(CallableDependencyManifestError::new(format!(
                    "producer OUT call {} is rooted more than once",
                    producer.call
                )));
            }
        }

        let mut out_call_owner = Vec::with_capacity(out.call_instances.len());
        for (index, call) in out.call_instances.iter().enumerate() {
            if call.id != OutCallInstanceId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "OUT call instance {} is not dense index {index}",
                    call.id
                )));
            }
            let mut root = call.id;
            let mut visited = BTreeSet::new();
            loop {
                if !visited.insert(root) {
                    return Err(CallableDependencyManifestError::new(format!(
                        "OUT call instance {} has cyclic concrete ancestry at {root}",
                        call.id
                    )));
                }
                let concrete = out
                    .call_instances
                    .get(root.as_usize())
                    .filter(|candidate| candidate.id == root)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "OUT call instance {} ancestry references missing call {root}",
                            call.id
                        ))
                    })?;
                let Some(parent) = concrete.parent else {
                    break;
                };
                root = parent;
            }
            out_call_owner.push(
                producer_root_owner
                    .get(&root)
                    .copied()
                    .unwrap_or(SemanticDependencyOwnerV1::ProgramRoot),
            );
        }

        let mut static_owner = BTreeMap::new();
        let mut attach_static_owner = |id: StaticOwnerId,
                                       owner: SemanticDependencyOwnerV1,
                                       context: &str|
         -> Result<(), CallableDependencyManifestError> {
            if let Some(previous) = static_owner.insert(id, owner)
                && previous != owner
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "static owner {id} has conflicting primary owners {previous:?} and {owner:?} at {context}"
                )));
            }
            Ok(())
        };
        for call in &out.call_instances {
            if let Some(owner) = call.owner {
                attach_static_owner(
                    owner,
                    out_call_owner[call.id.as_usize()],
                    &format!("OUT call {}", call.id),
                )?;
            }
        }
        for net in &out.nets {
            if let Some(owner) = net.owner {
                let anchor = net.owner_anchor.ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "OUT net {} has static owner {owner} without an exact port anchor",
                        net.id
                    ))
                })?;
                let port = out
                    .ports
                    .get(anchor.as_usize())
                    .filter(|port| port.id == anchor)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "OUT net {} owner anchor references missing port {anchor}",
                            net.id
                        ))
                    })?;
                attach_static_owner(
                    owner,
                    out_call_owner[port.call.as_usize()],
                    &format!("OUT net {}", net.id),
                )?;
            }
        }
        for owner in &out.static_owners {
            if !static_owner.contains_key(&owner.id) {
                return Err(CallableDependencyManifestError::new(format!(
                    "static owner {} has no exact call/net anchor; dependency ownership requires an engine identity",
                    owner.id
                )));
            }
        }

        // A semantic expression is an executable occurrence, not merely its
        // checked lexical definition. Its concrete call-root owns the
        // occurrence: the program root for ordinary expansion, or the producer
        // callable for a synthetic producer root. Static and frame coordinates
        // must agree whenever both are present. The checked expression remains
        // a referenced definition dependency under its lexical callable.
        for expression in &execution.expressions {
            let origin = execution
                .checked_expression_origins
                .get(expression.id.as_usize())
                .filter(|origin| origin.expression == expression.id)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "semantic expression {} has no exact checked origin",
                        expression.id
                    ))
                })?;
            let static_primary = expression
                .owner
                .map(|owner| {
                    static_owner.get(&owner).copied().ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "semantic expression {} references unanchored static owner {owner}",
                            expression.id
                        ))
                    })
                })
                .transpose()?;
            let frame_primary = origin
                .call_instance
                .map(|frame| {
                    out_call_owner
                        .get(frame.as_usize())
                        .copied()
                        .ok_or_else(|| {
                            CallableDependencyManifestError::new(format!(
                                "semantic expression {} references missing call frame {frame}",
                                expression.id
                            ))
                        })
                })
                .transpose()?;
            let concrete_primary = match (static_primary, frame_primary) {
                (Some(static_owner), Some(frame_owner)) if static_owner != frame_owner => {
                    return Err(CallableDependencyManifestError::new(format!(
                        "semantic expression {} has conflicting static {static_owner:?} and call-frame {frame_owner:?} occurrence owners",
                        expression.id
                    )));
                }
                (Some(owner), _) | (None, Some(owner)) => Some(owner),
                (None, None) => None,
            };
            if let Some(owner) = concrete_primary {
                expression_owner[expression.id.as_usize()] = owner;
            }
        }

        let expression_owner_at = |expression: SemanticExprId, context: &str| {
            expression_owner
                .get(expression.as_usize())
                .copied()
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "{context} references missing semantic expression {expression}"
                    ))
                })
        };
        let mut source_owner = Vec::with_capacity(resources.sources.len());
        for source in &resources.sources {
            source_owner.push(expression_owner_at(
                source.expression,
                &format!("semantic source {}", source.id),
            )?);
        }
        let mut state_owner = Vec::with_capacity(resources.states.len());
        for state in &resources.states {
            state_owner.push(expression_owner_at(
                state.expression,
                &format!("semantic state {}", state.id),
            )?);
        }
        let mut list_owner = Vec::with_capacity(resources.lists.len());
        for list in &resources.lists {
            list_owner.push(expression_owner_at(
                list.producer,
                &format!("semantic list {}", list.id),
            )?);
        }
        let mut value_list_owner = Vec::with_capacity(resources.value_list_authorities.len());
        for list in &resources.value_list_authorities {
            value_list_owner.push(expression_owner_at(
                list.producer,
                &format!("semantic value-list authority {}", list.id),
            )?);
        }

        let mut binding_owner = Vec::with_capacity(reactive.bindings.len());
        for binding in &reactive.bindings {
            binding_owner.push(expression_owner_at(
                binding.producer,
                &format!("reactive binding {}", binding.id),
            )?);
        }

        let owner_for_storage_origin = |field: &SemanticStorageFieldV1| {
            let mut candidates = Vec::new();
            if let Some(expression) = field.producer {
                candidates.push(expression_owner_at(
                    expression,
                    &format!("storage field {}", field.id),
                )?);
            }
            let explicit_owner = field
                .owner
                .map(|owner| {
                    static_owner.get(&owner).copied().ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references unanchored static owner {owner}",
                            field.id
                        ))
                    })
                })
                .transpose()?;
            match &field.origin {
                SemanticStorageFieldOriginV1::Reactive {
                    field: reactive_field,
                } => {
                    let reactive_field = reactive
                        .fields
                        .get(reactive_field.as_usize())
                        .filter(|candidate| candidate.id == *reactive_field)
                        .ok_or_else(|| {
                            CallableDependencyManifestError::new(format!(
                                "storage field {} references missing reactive field {reactive_field}",
                                field.id
                            ))
                        })?;
                    candidates.push(expression_owner_at(
                        reactive_field.producer,
                        &format!("storage field {}", field.id),
                    )?);
                }
                SemanticStorageFieldOriginV1::StateAuthority { state } => {
                    candidates.push(*state_owner.get(state.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references missing state {state}",
                            field.id
                        ))
                    })?);
                }
                SemanticStorageFieldOriginV1::ListAuthority { list, .. } => {
                    candidates.push(*list_owner.get(list.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references missing list {list}",
                            field.id
                        ))
                    })?);
                }
                SemanticStorageFieldOriginV1::ValueListAuthority { authority, .. } => {
                    candidates
                        .push(*value_list_owner.get(authority.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} references missing value-list authority {authority}",
                            field.id
                        ))
                    })?);
                }
                SemanticStorageFieldOriginV1::RecordProjection { expression, .. } => {
                    candidates.push(expression_owner_at(
                        *expression,
                        &format!("storage field {}", field.id),
                    )?);
                }
                SemanticStorageFieldOriginV1::DetachedCapture { target_owner, .. } => {
                    candidates.push(*static_owner.get(target_owner).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "storage field {} detached capture references unanchored owner {target_owner}",
                            field.id
                        ))
                    })?);
                }
            }
            explicit_owner.map_or_else(
                || exact_owner(candidates, &format!("storage field {}", field.id)),
                Ok,
            )
        };

        let mut storage_field_owner = Vec::with_capacity(storage.fields.len());
        for (index, field) in storage.fields.iter().enumerate() {
            if field.id != SemanticStorageFieldId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic storage field {} is not dense index {index}",
                    field.id
                )));
            }
            storage_field_owner.push(owner_for_storage_origin(field)?);
        }

        let mut memory_owner = Vec::with_capacity(memory.memories.len());
        for memory in &memory.memories {
            let owner = match memory.backing {
                SemanticMemoryBackingV1::State { binding, .. }
                | SemanticMemoryBackingV1::List { binding, .. } => {
                    *binding_owner.get(binding.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "semantic memory {} backing references missing binding {binding}",
                            memory.id
                        ))
                    })?
                }
                SemanticMemoryBackingV1::Collection { expression, .. } => {
                    *expression_owner.get(expression.as_usize()).ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "semantic memory {} backing references missing expression {expression}",
                            memory.id
                        ))
                    })?
                }
            };
            memory_owner.push(owner);
        }

        Ok(Self {
            callable_by_checked,
            checked_scope_owner,
            checked_statement_owner,
            expression_owner,
            statement_owner,
            call_owner,
            out_call_owner,
            static_owner,
            source_owner,
            state_owner,
            list_owner,
            value_list_owner,
            binding_owner,
            storage_field_owner,
            memory_owner,
        })
    }

    fn checked_scope(
        &self,
        scope: LexicalScopeId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.checked_scope_owner
            .get(&scope)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing checked scope {}",
                    scope.0
                ))
            })
    }

    fn checked_statement(
        &self,
        statement: boon_checked::CheckedStatementId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.checked_statement_owner
            .get(&statement)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing checked statement {}",
                    statement.0
                ))
            })
    }

    fn expression(
        &self,
        expression: SemanticExprId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.expression_owner
            .get(expression.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing semantic expression {expression}"
                ))
            })
    }

    fn statement(
        &self,
        statement: SemanticStatementId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.statement_owner
            .get(statement.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing semantic statement {statement}"
                ))
            })
    }

    fn call(
        &self,
        call: SemanticCallId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.call_owner
            .get(call.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing semantic call {call}"
                ))
            })
    }

    fn out_call(
        &self,
        call: OutCallInstanceId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.out_call_owner
            .get(call.as_usize())
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency ownership references missing OUT call {call}"
                ))
            })
    }

    fn static_owner(
        &self,
        owner: StaticOwnerId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        self.static_owner.get(&owner).copied().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references unanchored static owner {owner}"
            ))
        })
    }

    fn source(
        &self,
        source: SemanticSourceId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.source_owner,
            source.as_usize(),
            "semantic source",
            source,
        )
    }

    fn state(
        &self,
        state: SemanticStateId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(&self.state_owner, state.as_usize(), "semantic state", state)
    }

    fn list(
        &self,
        list: SemanticListId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(&self.list_owner, list.as_usize(), "semantic list", list)
    }

    fn value_list(
        &self,
        list: SemanticValueListAuthorityId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.value_list_owner,
            list.as_usize(),
            "semantic value-list authority",
            list,
        )
    }

    fn binding(
        &self,
        binding: SemanticBindingId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.binding_owner,
            binding.as_usize(),
            "semantic binding",
            binding,
        )
    }

    fn storage_field(
        &self,
        field: SemanticStorageFieldId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.storage_field_owner,
            field.as_usize(),
            "semantic storage field",
            field,
        )
    }

    fn memory(
        &self,
        memory: SemanticMemoryId,
    ) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
        indexed_owner(
            &self.memory_owner,
            memory.as_usize(),
            "semantic memory",
            memory,
        )
    }
}

fn indexed_owner(
    owners: &[SemanticDependencyOwnerV1],
    index: usize,
    kind: &str,
    id: impl fmt::Display,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    owners.get(index).copied().ok_or_else(|| {
        CallableDependencyManifestError::new(format!(
            "dependency ownership references missing {kind} {id}"
        ))
    })
}

fn exact_owner(
    candidates: Vec<SemanticDependencyOwnerV1>,
    context: &str,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let candidates = candidates.into_iter().collect::<BTreeSet<_>>();
    let candidates = candidates.iter().copied().collect::<Vec<_>>();
    let [owner] = candidates.as_slice() else {
        return Err(CallableDependencyManifestError::new(format!(
            "{context} resolves to {} primary callable/root owners {candidates:?}; an explicit engine identity is required",
            candidates.len(),
        )));
    };
    Ok(*owner)
}

#[derive(Clone, Debug)]
enum PendingDependencyReference {
    Entity(SemanticDependencyEntityV1),
    Owner(SemanticDependencyOwnerV1),
}

#[cfg(test)]
struct UnresolvedEntityReference {
    entity: SemanticDependencyEntityV1,
    owner_references_before: usize,
}

struct DependencyRecordInput<'a, P> {
    owner: SemanticDependencyOwnerV1,
    channel: SemanticDependencyChannelV1,
    roles: Vec<SemanticDependencyRoleV1>,
    subject: SemanticDependencySubjectV1,
    semantics: SemanticDependencySemanticsV1,
    payload: &'a P,
    references: Vec<PendingDependencyReference>,
}

macro_rules! collect_dependency {
    (
        $collector:expr,
        $owner:expr,
        $channel:expr,
        $roles:expr,
        $subject:expr,
        $semantics:expr,
        $payload:expr,
        $references:expr $(,)?
    ) => {
        $collector.dependency(DependencyRecordInput {
            owner: $owner,
            channel: $channel,
            roles: $roles,
            subject: $subject,
            semantics: $semantics,
            payload: $payload,
            references: $references,
        })
    };
}

#[cfg(test)]
#[derive(Debug)]
struct ValidatedDependencyCollection {
    dependencies: Vec<SemanticDependencyRecordV1>,
    coverage: Vec<SemanticDependencyCoverageV1>,
    direct: BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DenseDependencyProjectionKeyV4 {
    owner: SemanticDependencyOwnerV1,
    subject_kind: SemanticDependencySubjectKindV1,
    class: SemanticDependencyProjectionClassV4,
}

#[derive(Clone, Debug)]
struct CompactDependencyRowV4 {
    projection: DenseDependencyProjectionKeyV4,
    local_digest: [u8; 32],
    dependency: Option<SemanticDependencyRecordId>,
}

#[derive(Clone, Copy, Debug)]
struct CompactDependencyRecordV4 {
    row: usize,
    references_start: usize,
    references_end: usize,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
enum DependencyProjectionNodeV4 {
    Owner(SemanticDependencyStableOwnerV4),
    Projection(SemanticDependencyProjectionKeyV4),
}

#[derive(Debug)]
struct ValidatedCompactDependencyCollectionV4 {
    implementation_digests: BTreeMap<SemanticDependencyOwnerV1, [u8; 32]>,
    dependency_rows_digest: [u8; 32],
    coverage_receipts_digest: [u8; 32],
    projection_receipts_digest: [u8; 32],
    dependency_record_count: usize,
    coverage_record_count: usize,
    projection_count: usize,
    projection_edge_count: usize,
}

type ExpressionDependency = (
    SemanticDependencyChannelV1,
    Vec<SemanticDependencyRoleV1>,
    Vec<String>,
    Vec<PendingDependencyReference>,
);

struct DependencyCollector {
    compact_rows: Vec<CompactDependencyRowV4>,
    compact_records: Vec<CompactDependencyRecordV4>,
    compact_references: Vec<PendingDependencyReference>,
    #[cfg(test)]
    records: Vec<SemanticDependencyRecordV1>,
    // Entity references cannot be resolved until every subject has been
    // classified. Keep only those unresolved identities in a dense sidecar;
    // owner references already have their final record representation.
    #[cfg(test)]
    unresolved_entity_references: Vec<Vec<UnresolvedEntityReference>>,
    #[cfg(test)]
    coverage: Vec<SemanticDependencyCoverageV1>,
    // These indexes are lookup-only; canonical record/coverage order comes
    // from the dense vectors above. Hash tables avoid logarithmic comparisons
    // and one tree node allocation for each of hundreds of thousands of proof
    // subjects/entities without affecting deterministic artifact bytes.
    subjects: HashSet<SemanticDependencySubjectV1>,
    dependencies_by_entity: HashMap<SemanticDependencyEntityV1, Vec<SemanticDependencyRecordId>>,
    hash_scratch: Vec<u8>,
    flow_type_digests: HashMap<FlowType, [u8; 32]>,
    #[cfg(test)]
    retain_exhaustive: bool,
}

impl DependencyCollector {
    fn for_program(
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
        retain_exhaustive: bool,
    ) -> Self {
        #[cfg(not(test))]
        let _ = retain_exhaustive;
        // Expression inventories dominate the exhaustive proof. The current
        // portfolio's largest proof settles below five dependency records and
        // six coverage subjects per checked+semantic expression. These are
        // capacity hints only (the vectors still grow normally), but keeping
        // them separate avoids the former blanket six-times reserve.
        let expression_capacity = checked
            .expressions
            .len()
            .saturating_add(execution.expressions.len());
        let record_capacity = expression_capacity.saturating_mul(5);
        let coverage_capacity = expression_capacity.saturating_mul(6);
        Self {
            compact_rows: Vec::with_capacity(coverage_capacity),
            compact_records: Vec::with_capacity(record_capacity),
            compact_references: Vec::with_capacity(record_capacity),
            #[cfg(test)]
            records: retain_exhaustive
                .then(|| Vec::with_capacity(record_capacity))
                .unwrap_or_default(),
            #[cfg(test)]
            unresolved_entity_references: retain_exhaustive
                .then(|| Vec::with_capacity(record_capacity))
                .unwrap_or_default(),
            #[cfg(test)]
            coverage: retain_exhaustive
                .then(|| Vec::with_capacity(coverage_capacity))
                .unwrap_or_default(),
            subjects: HashSet::with_capacity(coverage_capacity),
            dependencies_by_entity: HashMap::with_capacity(record_capacity / 2),
            hash_scratch: Vec::new(),
            flow_type_digests: HashMap::with_capacity(checked.calls.len().saturating_mul(2)),
            #[cfg(test)]
            retain_exhaustive,
        }
    }

    #[cfg(test)]
    fn exhaustive_for_test() -> Self {
        Self {
            compact_rows: Vec::new(),
            compact_records: Vec::new(),
            compact_references: Vec::new(),
            records: Vec::new(),
            unresolved_entity_references: Vec::new(),
            coverage: Vec::new(),
            subjects: HashSet::new(),
            dependencies_by_entity: HashMap::new(),
            hash_scratch: Vec::new(),
            flow_type_digests: HashMap::new(),
            retain_exhaustive: true,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum DependencyProofNodeV3 {
    Owner { owner: SemanticDependencyOwnerV1 },
    Record { record: SemanticDependencyRecordId },
}

#[cfg(test)]
struct DependencyProofGraph<'a> {
    owners: &'a [SemanticDependencyOwnerV1],
    owner_ordinals: HashMap<SemanticDependencyOwnerV1, usize>,
    direct_by_owner: Vec<&'a [SemanticDependencyRecordId]>,
    records: &'a [SemanticDependencyRecordV1],
}

#[cfg(test)]
impl<'a> DependencyProofGraph<'a> {
    fn new(
        owners: &'a [SemanticDependencyOwnerV1],
        records: &'a [SemanticDependencyRecordV1],
        direct: &'a BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
    ) -> Result<Self, CallableDependencyManifestError> {
        let owner_ordinals = owners
            .iter()
            .copied()
            .enumerate()
            .map(|(ordinal, owner)| (owner, ordinal))
            .collect::<HashMap<_, _>>();
        if owner_ordinals.len() != owners.len() {
            return Err(CallableDependencyManifestError::new(
                "dependency proof graph has duplicate owners",
            ));
        }
        let direct_by_owner = owners
            .iter()
            .map(|owner| {
                direct.get(owner).map(Vec::as_slice).ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "dependency proof graph has no direct entry for owner {owner:?}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            owners,
            owner_ordinals,
            direct_by_owner,
            records,
        })
    }

    fn node_count(&self) -> Result<usize, CallableDependencyManifestError> {
        self.owners
            .len()
            .checked_add(self.records.len())
            .ok_or_else(|| {
                CallableDependencyManifestError::new(
                    "dependency proof graph node count overflows usize",
                )
            })
    }

    fn record_node(
        &self,
        record: SemanticDependencyRecordId,
    ) -> Result<usize, CallableDependencyManifestError> {
        self.records
            .get(record.as_usize())
            .filter(|candidate| candidate.id == record)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof graph references missing record {record}"
                ))
            })?;
        Ok(self.owners.len() + record.as_usize())
    }

    fn node_identity(
        &self,
        node: usize,
    ) -> Result<DependencyProofNodeV3, CallableDependencyManifestError> {
        if let Some(owner) = self.owners.get(node).copied() {
            return Ok(DependencyProofNodeV3::Owner { owner });
        }
        let record_index = node.checked_sub(self.owners.len()).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references invalid node {node}"
            ))
        })?;
        let record = self.records.get(record_index).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references invalid node {node}"
            ))
        })?;
        Ok(DependencyProofNodeV3::Record { record: record.id })
    }

    fn out_degree(&self, node: usize) -> Result<usize, CallableDependencyManifestError> {
        if let Some(direct) = self.direct_by_owner.get(node) {
            return Ok(direct.len());
        }
        let record_index = node.checked_sub(self.owners.len()).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references invalid node {node}"
            ))
        })?;
        let record = self.records.get(record_index).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references invalid node {node}"
            ))
        })?;
        record
            .referenced_dependencies
            .len()
            .checked_add(record.referenced_owners.len())
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency record {} edge count overflows usize",
                    record.id
                ))
            })
    }

    fn edge_target(
        &self,
        node: usize,
        edge: usize,
    ) -> Result<usize, CallableDependencyManifestError> {
        if let Some(direct) = self.direct_by_owner.get(node) {
            let dependency = direct.get(edge).copied().ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency owner {:?} has no edge {edge}",
                    self.owners[node]
                ))
            })?;
            return self.record_node(dependency);
        }
        let record_index = node.checked_sub(self.owners.len()).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references invalid node {node}"
            ))
        })?;
        let record = self.records.get(record_index).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references invalid node {node}"
            ))
        })?;
        if let Some(dependency) = record.referenced_dependencies.get(edge).copied() {
            return self.record_node(dependency);
        }
        let owner_edge = edge - record.referenced_dependencies.len();
        let owner = record
            .referenced_owners
            .get(owner_edge)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency record {} has no edge {edge}",
                    record.id
                ))
            })?;
        self.owner_ordinals.get(&owner).copied().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof graph references missing owner {owner:?}"
            ))
        })
    }
}

/// Finds the proof SCCs directly from the canonical record/owner adjacency.
///
/// The iterative Tarjan walk keeps only node-indexed state. It deliberately
/// does not materialize forward and reverse edge arrays that duplicate every
/// edge already retained by `DependencyProofGraph`.
#[cfg(test)]
struct DependencyProofComponents {
    member_offsets: Vec<usize>,
    member_arena: Vec<usize>,
}

#[cfg(test)]
impl DependencyProofComponents {
    fn with_node_capacity(node_count: usize) -> Self {
        let mut member_offsets = Vec::with_capacity(node_count.saturating_add(1));
        member_offsets.push(0);
        Self {
            member_offsets,
            member_arena: Vec::with_capacity(node_count),
        }
    }

    fn len(&self) -> usize {
        self.member_offsets.len() - 1
    }

    fn members(&self, component: usize) -> Option<&[usize]> {
        let start = *self.member_offsets.get(component)?;
        let end = *self.member_offsets.get(component + 1)?;
        self.member_arena.get(start..end)
    }

    fn iter(&self) -> impl Iterator<Item = &[usize]> {
        self.member_offsets
            .windows(2)
            .map(|range| &self.member_arena[range[0]..range[1]])
    }

    fn push_from_active_until(
        &mut self,
        active: &mut Vec<usize>,
        root: usize,
    ) -> Result<(), CallableDependencyManifestError> {
        let start = self.member_arena.len();
        loop {
            let member = active.pop().ok_or_else(|| {
                CallableDependencyManifestError::new("dependency proof component stack underflow")
            })?;
            self.member_arena.push(member);
            if member == root {
                break;
            }
        }
        self.member_arena[start..].sort_unstable();
        self.member_offsets.push(self.member_arena.len());
        Ok(())
    }
}

#[cfg(test)]
fn dependency_proof_components(
    graph: &DependencyProofGraph<'_>,
) -> Result<(Vec<usize>, DependencyProofComponents), CallableDependencyManifestError> {
    let node_count = graph.node_count()?;
    let mut discovery_index = vec![usize::MAX; node_count];
    let mut low_link = vec![usize::MAX; node_count];
    let mut component_by_node = vec![usize::MAX; node_count];
    let mut components = DependencyProofComponents::with_node_capacity(node_count);
    let mut active = Vec::new();
    let mut pending = Vec::new();
    let mut next_discovery_index = 0usize;

    for start in 0..node_count {
        if discovery_index[start] != usize::MAX {
            continue;
        }

        discovery_index[start] = next_discovery_index;
        low_link[start] = next_discovery_index;
        next_discovery_index = next_discovery_index.checked_add(1).ok_or_else(|| {
            CallableDependencyManifestError::new("dependency proof discovery index overflows usize")
        })?;
        active.push(start);
        pending.push((start, 0usize));

        while !pending.is_empty() {
            let frame = pending.len() - 1;
            let (node, next_edge) = pending[frame];
            let degree = graph.out_degree(node)?;
            if next_edge < degree {
                pending[frame].1 += 1;
                let target = graph.edge_target(node, next_edge)?;
                if discovery_index[target] == usize::MAX {
                    discovery_index[target] = next_discovery_index;
                    low_link[target] = next_discovery_index;
                    next_discovery_index =
                        next_discovery_index.checked_add(1).ok_or_else(|| {
                            CallableDependencyManifestError::new(
                                "dependency proof discovery index overflows usize",
                            )
                        })?;
                    active.push(target);
                    pending.push((target, 0usize));
                } else if component_by_node[target] == usize::MAX {
                    low_link[node] = low_link[node].min(discovery_index[target]);
                }
                continue;
            }

            pending.pop();
            if let Some((parent, _)) = pending.last().copied() {
                low_link[parent] = low_link[parent].min(low_link[node]);
            }
            if low_link[node] != discovery_index[node] {
                continue;
            }

            let component = components.len();
            components.push_from_active_until(&mut active, node)?;
            for member in components
                .members(component)
                .expect("fresh dependency proof component has members")
            {
                component_by_node[*member] = component;
            }
        }
    }

    if !active.is_empty() {
        return Err(CallableDependencyManifestError::new(
            "dependency proof component stack is not empty after traversal",
        ));
    }
    Ok((component_by_node, components))
}

#[derive(Clone, Copy, Debug, Default)]
struct DependencyGraphDigestStats {
    nodes: usize,
    edges: usize,
    components: usize,
    cyclic_components: usize,
    maximum_component_nodes: usize,
    component_edges: usize,
}

#[cfg(test)]
type DependencyGraphDigestMap = BTreeMap<SemanticDependencyOwnerV1, [u8; 32]>;

fn dependency_proof_update_usize(
    hasher: &mut Sha256,
    value: usize,
    context: &str,
) -> Result<(), CallableDependencyManifestError> {
    let value = u64::try_from(value).map_err(|_| {
        CallableDependencyManifestError::new(format!(
            "{context} exceeds the dependency proof u64 encoding"
        ))
    })?;
    hasher.update(value.to_be_bytes());
    Ok(())
}

#[cfg(test)]
fn dependency_proof_update_owner(
    hasher: &mut Sha256,
    owner: SemanticDependencyOwnerV1,
) -> Result<(), CallableDependencyManifestError> {
    match owner {
        SemanticDependencyOwnerV1::ProgramRoot => hasher.update([0]),
        SemanticDependencyOwnerV1::Callable { callable } => {
            hasher.update([1]);
            dependency_proof_update_usize(hasher, callable.as_usize(), "semantic callable ID")?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn dependency_proof_update_node(
    hasher: &mut Sha256,
    node: DependencyProofNodeV3,
) -> Result<(), CallableDependencyManifestError> {
    match node {
        DependencyProofNodeV3::Owner { owner } => {
            hasher.update([0]);
            dependency_proof_update_owner(hasher, owner)?;
        }
        DependencyProofNodeV3::Record { record } => {
            hasher.update([1]);
            dependency_proof_update_usize(hasher, record.as_usize(), "dependency record ID")?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn dependency_proof_owner_digest(
    owner: SemanticDependencyOwnerV1,
    direct: &[SemanticDependencyRecordId],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_IMPLEMENTATION_OWNER_DOMAIN);
    dependency_proof_update_owner(&mut hasher, owner)?;
    dependency_proof_update_usize(&mut hasher, direct.len(), "owner dependency count")?;
    for dependency in direct {
        dependency_proof_update_usize(
            &mut hasher,
            dependency.as_usize(),
            "owner dependency record ID",
        )?;
    }
    Ok(hasher.finalize().into())
}

/// Hashes canonical record leaves in dense record order. The callback chooses
/// whether to consume each leaf immediately or retain it; the set digest
/// remains byte-for-byte identical to the V2 record-set contract.
#[cfg(test)]
fn stream_dependency_record_digests(
    records: &[SemanticDependencyRecordV1],
    mut consume: impl FnMut(
        SemanticDependencyRecordId,
        [u8; 32],
    ) -> Result<(), CallableDependencyManifestError>,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let mut record_set_hasher = Sha256::new();
    record_set_hasher.update(DEPENDENCY_RECORD_SET_DIGEST_DOMAIN);
    dependency_proof_update_usize(
        &mut record_set_hasher,
        records.len(),
        "dependency record digest count",
    )?;
    let mut scratch = Vec::new();
    for (index, record) in records.iter().enumerate() {
        if record.id != SemanticDependencyRecordId(index) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency record {} is not dense index {index}",
                record.id
            )));
        }
        let digest = canonical_dependency_hash_with_buffer(
            DEPENDENCY_RECORD_DIGEST_DOMAIN,
            record,
            &mut scratch,
        )?;
        record_set_hasher.update(digest);
        consume(record.id, digest)?;
    }
    Ok(record_set_hasher.finalize().into())
}

#[cfg(test)]
fn reference_dependency_record_set_digest(
    records: &[SemanticDependencyRecordV1],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let mut scratch = Vec::new();
    let leaves = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            if record.id != SemanticDependencyRecordId(index) {
                return Err(CallableDependencyManifestError::new(format!(
                    "dependency record {} is not dense index {index}",
                    record.id
                )));
            }
            canonical_dependency_hash_with_buffer(
                DEPENDENCY_RECORD_DIGEST_DOMAIN,
                record,
                &mut scratch,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_RECORD_SET_DIGEST_DOMAIN);
    dependency_proof_update_usize(&mut hasher, leaves.len(), "dependency record digest count")?;
    for leaf in leaves {
        hasher.update(leaf);
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
fn dependency_proof_record_leaf_arena(
    graph: &DependencyProofGraph<'_>,
) -> Result<(Vec<[u8; 32]>, [u8; 32]), CallableDependencyManifestError> {
    // A fixed 32-byte leaf per record is substantially smaller than retaining
    // one partially initialized Sha256 state per condensed component. Local
    // component hashers are now built and finalized one at a time below.
    let mut record_leaves = Vec::with_capacity(graph.records.len());
    let record_set_digest = stream_dependency_record_digests(graph.records, |_, digest| {
        record_leaves.push(digest);
        Ok(())
    })?;
    Ok((record_leaves, record_set_digest))
}

#[cfg(test)]
fn reference_dependency_proof_component_local_hashers(
    graph: &DependencyProofGraph<'_>,
    components: &DependencyProofComponents,
    component_by_node: &[usize],
) -> Result<(Vec<Sha256>, [u8; 32]), CallableDependencyManifestError> {
    let owner_count = graph.owners.len();
    let mut member_counts = vec![0usize; components.len()];
    let mut hashers = Vec::with_capacity(components.len());
    for (component, members) in components.iter().enumerate() {
        let mut hasher = Sha256::new();
        hasher.update(DEPENDENCY_IMPLEMENTATION_COMPONENT_DOMAIN);
        dependency_proof_update_usize(&mut hasher, members.len(), "proof component member count")?;
        for node in members
            .iter()
            .copied()
            .take_while(|node| *node < owner_count)
        {
            let node_identity = graph.node_identity(node)?;
            let DependencyProofNodeV3::Owner { owner } = node_identity else {
                return Err(CallableDependencyManifestError::new(
                    "dependency proof owner prefix contains a record node",
                ));
            };
            dependency_proof_update_node(&mut hasher, node_identity)?;
            let owner_ordinal = graph.owner_ordinals[&owner];
            hasher.update(dependency_proof_owner_digest(
                owner,
                graph.direct_by_owner[owner_ordinal],
            )?);
            member_counts[component] += 1;
        }
        hashers.push(hasher);
    }

    let record_set_digest = stream_dependency_record_digests(graph.records, |record, digest| {
        let node = graph.record_node(record)?;
        let component = *component_by_node.get(node).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof record {record} has no component"
            ))
        })?;
        dependency_proof_update_node(
            &mut hashers[component],
            DependencyProofNodeV3::Record { record },
        )?;
        hashers[component].update(digest);
        member_counts[component] += 1;
        Ok(())
    })?;

    for (component, (actual, members)) in member_counts.iter().zip(components.iter()).enumerate() {
        if *actual != members.len() {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency proof component {component} streamed {actual} of {} members",
                members.len()
            )));
        }
    }
    Ok((hashers, record_set_digest))
}

#[cfg(test)]
fn dependency_proof_component_local_hasher(
    graph: &DependencyProofGraph<'_>,
    members: &[usize],
    record_leaves: &[[u8; 32]],
) -> Result<Sha256, CallableDependencyManifestError> {
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_IMPLEMENTATION_COMPONENT_DOMAIN);
    dependency_proof_update_usize(&mut hasher, members.len(), "proof component member count")?;
    for node in members.iter().copied() {
        let node_identity = graph.node_identity(node)?;
        dependency_proof_update_node(&mut hasher, node_identity)?;
        match node_identity {
            DependencyProofNodeV3::Owner { owner } => {
                let owner_ordinal = graph.owner_ordinals[&owner];
                hasher.update(dependency_proof_owner_digest(
                    owner,
                    graph.direct_by_owner[owner_ordinal],
                )?);
            }
            DependencyProofNodeV3::Record { record } => {
                hasher.update(
                    record_leaves
                        .get(record.as_usize())
                        .copied()
                        .ok_or_else(|| {
                            CallableDependencyManifestError::new(format!(
                                "dependency proof record {record} has no canonical leaf digest"
                            ))
                        })?,
                );
            }
        }
    }
    Ok(hasher)
}

#[cfg(test)]
fn finish_dependency_proof_component_digest(
    mut hasher: Sha256,
    graph: &DependencyProofGraph<'_>,
    dependencies: &[usize],
    representatives: &[usize],
    component_digests: &[Option<[u8; 32]>],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    dependency_proof_update_usize(
        &mut hasher,
        dependencies.len(),
        "proof component dependency count",
    )?;
    for dependency in dependencies.iter().copied() {
        dependency_proof_update_node(
            &mut hasher,
            graph.node_identity(representatives[dependency])?,
        )?;
        hasher.update(component_digests[dependency].ok_or_else(|| {
            CallableDependencyManifestError::new(
                "dependency proof component was hashed before its dependency",
            )
        })?);
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
fn dependency_proof_implementation_digest(
    owner: SemanticDependencyOwnerV1,
    representative: DependencyProofNodeV3,
    component_digest: [u8; 32],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_IMPLEMENTATION_DOMAIN);
    dependency_proof_update_owner(&mut hasher, owner)?;
    dependency_proof_update_node(&mut hasher, representative)?;
    hasher.update(component_digest);
    Ok(hasher.finalize().into())
}

#[cfg(test)]
struct DependencyProofCsr {
    edge_offsets: Vec<usize>,
    edge_arena: Vec<usize>,
}

#[cfg(test)]
impl DependencyProofCsr {
    fn node_count(&self) -> usize {
        self.edge_offsets.len() - 1
    }

    fn edges(&self, node: usize) -> Option<&[usize]> {
        let start = *self.edge_offsets.get(node)?;
        let end = *self.edge_offsets.get(node.checked_add(1)?)?;
        self.edge_arena.get(start..end)
    }

    fn iter(&self) -> impl Iterator<Item = &[usize]> {
        self.edge_offsets
            .windows(2)
            .map(|range| &self.edge_arena[range[0]..range[1]])
    }

    fn edge_count(&self) -> usize {
        self.edge_arena.len()
    }

    fn reverse(&self) -> Result<Self, CallableDependencyManifestError> {
        let node_count = self.node_count();
        let mut cursors = vec![0usize; node_count];
        for target in self.edge_arena.iter().copied() {
            let count = cursors.get_mut(target).ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof condensed edge references missing component {target}"
                ))
            })?;
            *count = count.checked_add(1).ok_or_else(|| {
                CallableDependencyManifestError::new(
                    "dependency proof reverse edge count overflows usize",
                )
            })?;
        }

        let mut edge_offsets = Vec::with_capacity(node_count.saturating_add(1));
        edge_offsets.push(0usize);
        for count in &cursors {
            let next = edge_offsets
                .last()
                .copied()
                .expect("reverse CSR has an initial offset")
                .checked_add(*count)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(
                        "dependency proof reverse edge offsets overflow usize",
                    )
                })?;
            edge_offsets.push(next);
        }
        for (component, cursor) in cursors.iter_mut().enumerate() {
            *cursor = edge_offsets[component];
        }
        let mut edge_arena = vec![0usize; self.edge_arena.len()];
        for (source, targets) in self.iter().enumerate() {
            for target in targets.iter().copied() {
                let slot = cursors[target];
                edge_arena[slot] = source;
                cursors[target] += 1;
            }
        }
        drop(cursors);
        Ok(Self {
            edge_offsets,
            edge_arena,
        })
    }
}

#[cfg(test)]
fn build_dependency_proof_outgoing_csr(
    graph: &DependencyProofGraph<'_>,
    component_by_node: &[usize],
    component_count: usize,
    representatives: &[usize],
) -> Result<(DependencyProofCsr, usize), CallableDependencyManifestError> {
    let node_count = graph.node_count()?;
    let mut cursors = vec![0usize; component_count];
    let mut graph_edge_count = 0usize;
    for source in 0..node_count {
        let source_component = component_by_node[source];
        let degree = graph.out_degree(source)?;
        graph_edge_count = graph_edge_count.checked_add(degree).ok_or_else(|| {
            CallableDependencyManifestError::new(
                "dependency proof graph edge count overflows usize",
            )
        })?;
        for edge in 0..degree {
            let target = graph.edge_target(source, edge)?;
            let target_component = component_by_node[target];
            if source_component != target_component {
                cursors[source_component] =
                    cursors[source_component].checked_add(1).ok_or_else(|| {
                        CallableDependencyManifestError::new(
                            "dependency proof component edge count overflows usize",
                        )
                    })?;
            }
        }
    }

    let mut edge_offsets = Vec::with_capacity(component_count.saturating_add(1));
    edge_offsets.push(0usize);
    for count in &cursors {
        let next = edge_offsets
            .last()
            .copied()
            .expect("outgoing CSR has an initial offset")
            .checked_add(*count)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(
                    "dependency proof component edge offsets overflow usize",
                )
            })?;
        edge_offsets.push(next);
    }
    let raw_component_edge_count = edge_offsets.last().copied().unwrap_or(0);
    for (component, cursor) in cursors.iter_mut().enumerate() {
        *cursor = edge_offsets[component];
    }
    let mut edge_arena = vec![0usize; raw_component_edge_count];
    for source in 0..node_count {
        let source_component = component_by_node[source];
        for edge in 0..graph.out_degree(source)? {
            let target = graph.edge_target(source, edge)?;
            let target_component = component_by_node[target];
            if source_component != target_component {
                let slot = cursors[source_component];
                edge_arena[slot] = target_component;
                cursors[source_component] += 1;
            }
        }
    }
    drop(cursors);

    let mut write = 0usize;
    for component in 0..component_count {
        let start = edge_offsets[component];
        let end = edge_offsets[component + 1];
        edge_arena[start..end].sort_unstable_by_key(|target| representatives[*target]);
        edge_offsets[component] = write;
        let mut previous = None;
        for read in start..end {
            let target = edge_arena[read];
            if previous == Some(target) {
                continue;
            }
            edge_arena[write] = target;
            write += 1;
            previous = Some(target);
        }
    }
    edge_offsets[component_count] = write;
    edge_arena.truncate(write);
    Ok((
        DependencyProofCsr {
            edge_offsets,
            edge_arena,
        },
        graph_edge_count,
    ))
}

#[cfg(test)]
fn dependency_proof_component_digests(
    graph: &DependencyProofGraph<'_>,
    components: &DependencyProofComponents,
    record_leaves: &[[u8; 32]],
    outgoing: &DependencyProofCsr,
    parents: &DependencyProofCsr,
    representatives: &[usize],
) -> Result<Vec<Option<[u8; 32]>>, CallableDependencyManifestError> {
    let mut remaining_dependencies = outgoing.iter().map(<[usize]>::len).collect::<Vec<_>>();
    let mut ready = remaining_dependencies
        .iter()
        .enumerate()
        .filter_map(|(component, remaining)| (*remaining == 0).then_some(component))
        .collect::<VecDeque<_>>();
    let mut component_digests = vec![None; components.len()];
    let mut completed_components = 0usize;
    while let Some(component) = ready.pop_front() {
        let members = components.members(component).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof has no member slice for component {component}"
            ))
        })?;
        let local_hasher = dependency_proof_component_local_hasher(graph, members, record_leaves)?;
        component_digests[component] = Some(finish_dependency_proof_component_digest(
            local_hasher,
            graph,
            outgoing.edges(component).ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof has no outgoing edge slice for component {component}"
                ))
            })?,
            representatives,
            &component_digests,
        )?);
        completed_components += 1;
        for parent in parents
            .edges(component)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof has no reverse edge slice for component {component}"
                ))
            })?
            .iter()
            .copied()
        {
            remaining_dependencies[parent] -= 1;
            if remaining_dependencies[parent] == 0 {
                ready.push_back(parent);
            }
        }
    }
    if completed_components != components.len() {
        return Err(CallableDependencyManifestError::new(
            "dependency proof condensation graph is unexpectedly cyclic",
        ));
    }
    Ok(component_digests)
}

/// Commits condensed proof components directly in Tarjan completion order.
///
/// Tarjan emits a component only after every distinct component reachable from
/// it has completed. Component IDs are therefore already a dependency-first
/// topological order: every inter-component edge from `component` targets a
/// lower component ID. Reusing that order avoids materializing both forward
/// and reverse condensation CSRs plus a second ready-queue traversal. One
/// reusable scratch vector is sufficient to canonicalize each component's
/// distinct outgoing dependencies before its digest is finalized.
#[cfg(test)]
fn dependency_proof_component_digests_in_tarjan_order(
    graph: &DependencyProofGraph<'_>,
    components: &DependencyProofComponents,
    component_by_node: &[usize],
    record_leaves: &[[u8; 32]],
    representatives: &[usize],
) -> Result<(Vec<Option<[u8; 32]>>, usize, usize), CallableDependencyManifestError> {
    let mut component_digests = vec![None; components.len()];
    let mut dependencies = Vec::new();
    let mut graph_edge_count = 0usize;
    let mut component_edge_count = 0usize;

    for component in 0..components.len() {
        let members = components.members(component).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof has no member slice for component {component}"
            ))
        })?;
        dependencies.clear();
        for node in members.iter().copied() {
            let degree = graph.out_degree(node)?;
            graph_edge_count = graph_edge_count.checked_add(degree).ok_or_else(|| {
                CallableDependencyManifestError::new(
                    "dependency proof graph edge count overflows usize",
                )
            })?;
            for edge in 0..degree {
                let target = graph.edge_target(node, edge)?;
                let target_component = *component_by_node.get(target).ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "dependency proof edge references unclassified node {target}"
                    ))
                })?;
                if target_component != component {
                    dependencies.push(target_component);
                }
            }
        }
        dependencies.sort_unstable_by_key(|target| representatives[*target]);
        dependencies.dedup();
        for dependency in dependencies.iter().copied() {
            if dependency >= component {
                return Err(CallableDependencyManifestError::new(format!(
                    "dependency proof Tarjan order is not dependency-first: component {component} references component {dependency}"
                )));
            }
        }
        component_edge_count = component_edge_count
            .checked_add(dependencies.len())
            .ok_or_else(|| {
                CallableDependencyManifestError::new(
                    "dependency proof component edge count overflows usize",
                )
            })?;
        let local_hasher = dependency_proof_component_local_hasher(graph, members, record_leaves)?;
        component_digests[component] = Some(finish_dependency_proof_component_digest(
            local_hasher,
            graph,
            &dependencies,
            representatives,
            &component_digests,
        )?);
    }

    Ok((component_digests, graph_edge_count, component_edge_count))
}

#[cfg(test)]
fn reference_dependency_proof_component_digests(
    graph: &DependencyProofGraph<'_>,
    components: &DependencyProofComponents,
    component_local_hashers: Vec<Sha256>,
    outgoing: &DependencyProofCsr,
    parents: &DependencyProofCsr,
    representatives: &[usize],
) -> Result<Vec<Option<[u8; 32]>>, CallableDependencyManifestError> {
    let mut component_local_hashers = component_local_hashers
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    let mut remaining_dependencies = outgoing.iter().map(<[usize]>::len).collect::<Vec<_>>();
    let mut ready = remaining_dependencies
        .iter()
        .enumerate()
        .filter_map(|(component, remaining)| (*remaining == 0).then_some(component))
        .collect::<VecDeque<_>>();
    let mut component_digests = vec![None; components.len()];
    let mut completed_components = 0usize;
    while let Some(component) = ready.pop_front() {
        let local_hasher = component_local_hashers[component].take().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency proof component {component} was finalized more than once"
            ))
        })?;
        component_digests[component] = Some(finish_dependency_proof_component_digest(
            local_hasher,
            graph,
            outgoing.edges(component).ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof has no outgoing edge slice for component {component}"
                ))
            })?,
            representatives,
            &component_digests,
        )?);
        completed_components += 1;
        for parent in parents
            .edges(component)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof has no reverse edge slice for component {component}"
                ))
            })?
            .iter()
            .copied()
        {
            remaining_dependencies[parent] -= 1;
            if remaining_dependencies[parent] == 0 {
                ready.push_back(parent);
            }
        }
    }
    if completed_components != components.len() {
        return Err(CallableDependencyManifestError::new(
            "dependency proof condensation graph is unexpectedly cyclic",
        ));
    }
    Ok(component_digests)
}

#[cfg(test)]
fn dependency_proof_owner_implementation_digests(
    graph: &DependencyProofGraph<'_>,
    component_by_node: &[usize],
    representatives: &[usize],
    component_digests: &[Option<[u8; 32]>],
) -> Result<DependencyGraphDigestMap, CallableDependencyManifestError> {
    let mut digests = BTreeMap::new();
    for (owner_ordinal, owner) in graph.owners.iter().copied().enumerate() {
        let component = component_by_node[owner_ordinal];
        let digest = dependency_proof_implementation_digest(
            owner,
            graph.node_identity(representatives[component])?,
            component_digests[component].ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "dependency proof owner {owner:?} has no completed component digest"
                ))
            })?,
        )?;
        digests.insert(owner, digest);
    }
    Ok(digests)
}

/// Commits the exact reachable implementation graph once rather than
/// enumerating every owner's transitive record set. Strongly connected
/// components make cycles explicit; the condensed graph is a DAG whose
/// content-addressed child digests propagate every reachable record and edge
/// change to the owning callable without pulling in unrelated components.
#[cfg(test)]
fn build_dependency_graph_digests(
    owners: &[SemanticDependencyOwnerV1],
    records: &[SemanticDependencyRecordV1],
    direct: &BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
) -> Result<
    (
        DependencyGraphDigestMap,
        DependencyGraphDigestStats,
        [u8; 32],
    ),
    CallableDependencyManifestError,
> {
    let graph = DependencyProofGraph::new(owners, records, direct)?;
    let (component_by_node, components) = dependency_proof_components(&graph)?;
    let (record_leaves, dependency_record_set_digest) = dependency_proof_record_leaf_arena(&graph)?;
    let representatives = components
        .iter()
        .map(|members| {
            members.first().copied().ok_or_else(|| {
                CallableDependencyManifestError::new(
                    "dependency proof graph produced an empty component",
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let node_count = graph.node_count()?;
    let (component_digests, edge_count, component_edge_count) =
        dependency_proof_component_digests_in_tarjan_order(
            &graph,
            &components,
            &component_by_node,
            &record_leaves,
            &representatives,
        )?;
    drop(record_leaves);

    let digests = dependency_proof_owner_implementation_digests(
        &graph,
        &component_by_node,
        &representatives,
        &component_digests,
    )?;

    let stats = DependencyGraphDigestStats {
        nodes: node_count,
        edges: edge_count,
        components: components.len(),
        cyclic_components: components
            .iter()
            .filter(|members| members.len() > 1)
            .count(),
        maximum_component_nodes: components.iter().map(<[usize]>::len).max().unwrap_or(0),
        component_edges: component_edge_count,
    };
    Ok((digests, stats, dependency_record_set_digest))
}

impl DependencyCollector {
    fn trace_counts(&self, phase: &str) {
        eprintln!(
            "boon_semantic dependency_manifest {phase}:counts pending={} coverage={} subjects={} entities={} flow_types={}",
            self.compact_records.len(),
            self.compact_rows.len(),
            self.subjects.len(),
            self.dependencies_by_entity.len(),
            self.flow_type_digests.len(),
        );
    }

    fn compact_local_dependency_digest(
        &mut self,
        roles: &[SemanticDependencyRoleV1],
        subject: &SemanticDependencySubjectV1,
        semantics: &SemanticDependencyRecordSemanticsV1,
        payload_digest: [u8; 32],
    ) -> Result<[u8; 32], CallableDependencyManifestError> {
        compact_local_dependency_digest_v4(
            roles,
            subject,
            semantics,
            payload_digest,
            &mut self.hash_scratch,
        )
    }

    fn compact_local_classification_digest(
        &mut self,
        subject: &SemanticDependencySubjectV1,
        payload_digest: [u8; 32],
    ) -> Result<[u8; 32], CallableDependencyManifestError> {
        compact_local_classification_digest_v4(subject, payload_digest, &mut self.hash_scratch)
    }

    fn push_compact_classification(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: &SemanticDependencySubjectV1,
        class: SemanticDependencyProjectionClassV4,
        local_digest: [u8; 32],
        dependency: Option<SemanticDependencyRecordId>,
    ) {
        self.compact_rows.push(CompactDependencyRowV4 {
            projection: DenseDependencyProjectionKeyV4 {
                owner,
                subject_kind: subject.kind,
                class,
            },
            local_digest,
            dependency,
        });
    }

    fn compact_semantics(
        &mut self,
        semantics: SemanticDependencySemanticsV1,
    ) -> Result<SemanticDependencyRecordSemanticsV1, CallableDependencyManifestError> {
        let flow_type_digest = if let Some(flow_type) = semantics.flow_type {
            if let Some(digest) = self.flow_type_digests.get(&flow_type) {
                Some(*digest)
            } else {
                let digest = boon_contract::canonical_serde_hash_v1_with_buffer(
                    DEPENDENCY_FLOW_TYPE_DIGEST_DOMAIN,
                    &flow_type,
                    &mut self.hash_scratch,
                )
                .map_err(|error| {
                    CallableDependencyManifestError::new(format!(
                        "failed to hash dependency flow type: {error}"
                    ))
                })?;
                self.flow_type_digests.insert(flow_type, digest);
                Some(digest)
            }
        } else {
            None
        };
        Ok(SemanticDependencyRecordSemanticsV1 {
            projection: semantics.projection,
            flow_type_digest,
            evaluation_scope: semantics.evaluation_scope,
            program_role: semantics.program_role,
            static_owner: semantics.static_owner,
            call_instance: semantics.call_instance,
            row: semantics.row,
            multiplicity: semantics.multiplicity,
            lifetime: semantics.lifetime,
            phase: semantics.phase,
            visibility: semantics.visibility,
        })
    }

    fn dependency<P: Serialize>(
        &mut self,
        input: DependencyRecordInput<'_, P>,
    ) -> Result<SemanticDependencyRecordId, CallableDependencyManifestError> {
        self.dependency_with_flow_type_digest(input, None)
    }

    fn dependency_with_flow_type_digest<P: Serialize>(
        &mut self,
        input: DependencyRecordInput<'_, P>,
        flow_type_digest: Option<[u8; 32]>,
    ) -> Result<SemanticDependencyRecordId, CallableDependencyManifestError> {
        let DependencyRecordInput {
            owner,
            channel,
            mut roles,
            subject,
            semantics,
            payload,
            references,
        } = input;
        self.claim_subject(&subject)?;
        if flow_type_digest.is_some() && semantics.flow_type.is_some() {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency subject {subject:?} supplies both a structural flow type and its canonical digest"
            )));
        }
        let mut semantics = self.compact_semantics(semantics)?;
        if flow_type_digest.is_some() {
            semantics.flow_type_digest = flow_type_digest;
        }
        roles.sort();
        roles.dedup();
        if roles.is_empty() {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency subject {subject:?} has no explicit semantic role"
            )));
        }
        let payload_digest = dependency_payload_digest(payload, &mut self.hash_scratch)?;
        let id = SemanticDependencyRecordId(self.compact_records.len());
        let local_digest =
            self.compact_local_dependency_digest(&roles, &subject, &semantics, payload_digest)?;
        let row = self.compact_rows.len();
        self.push_compact_classification(
            owner,
            &subject,
            SemanticDependencyProjectionClassV4::Dependency { channel },
            local_digest,
            Some(id),
        );
        let references_start = self.compact_references.len();
        self.compact_references.extend(references.iter().cloned());
        let references_end = self.compact_references.len();
        self.compact_records.push(CompactDependencyRecordV4 {
            row,
            references_start,
            references_end,
        });

        #[cfg(test)]
        if self.retain_exhaustive {
            let mut unresolved_entity_references = Vec::new();
            let mut referenced_owners = Vec::new();
            for reference in &references {
                match reference {
                    PendingDependencyReference::Entity(entity) => {
                        unresolved_entity_references.push(UnresolvedEntityReference {
                            entity: entity.clone(),
                            owner_references_before: referenced_owners.len(),
                        });
                    }
                    PendingDependencyReference::Owner(owner) => referenced_owners.push(*owner),
                }
            }
            self.records.push(SemanticDependencyRecordV1 {
                id,
                owner,
                channel,
                roles,
                subject: subject.clone(),
                semantics,
                payload_digest,
                referenced_dependencies: Vec::new(),
                referenced_owners,
            });
            self.unresolved_entity_references
                .push(unresolved_entity_references);
            self.coverage.push(SemanticDependencyCoverageV1 {
                id: SemanticDependencyCoverageId(self.coverage.len()),
                subject: subject.clone(),
                primary_owner: owner,
                disposition: SemanticDependencyCoverageDispositionV1::Dependency { dependency: id },
            });
        }
        self.dependencies_by_entity
            .entry(subject.identity.clone())
            .or_default()
            .push(id);
        Ok(id)
    }

    fn structural(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: SemanticDependencySubjectV1,
        payload: &impl Serialize,
    ) -> Result<(), CallableDependencyManifestError> {
        self.claim_subject(&subject)?;
        let payload_digest = dependency_payload_digest(payload, &mut self.hash_scratch)?;
        let local_digest = self.compact_local_classification_digest(&subject, payload_digest)?;
        self.push_compact_classification(
            owner,
            &subject,
            SemanticDependencyProjectionClassV4::Structural,
            local_digest,
            None,
        );
        #[cfg(test)]
        if self.retain_exhaustive {
            self.coverage.push(SemanticDependencyCoverageV1 {
                id: SemanticDependencyCoverageId(self.coverage.len()),
                subject,
                primary_owner: owner,
                disposition: SemanticDependencyCoverageDispositionV1::Structural { payload_digest },
            });
        }
        Ok(())
    }

    fn checked_program_structural(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: SemanticDependencySubjectV1,
        checked: &CheckedProgram,
    ) -> Result<CheckedProgramDigestV1, CallableDependencyManifestError> {
        self.claim_subject(&subject)?;
        let [payload_digest, checked_program_digest] =
            boon_contract::canonical_serde_hashes_v1_streaming(
                [
                    DEPENDENCY_RECORD_PAYLOAD_DOMAIN,
                    CHECKED_PROGRAM_DIGEST_DOMAIN,
                ],
                checked,
            )
            .map_err(|error| {
                CallableDependencyManifestError::new(format!(
                    "failed to stream checked-program dependency payload: {error}"
                ))
            })?;
        let local_digest = self.compact_local_classification_digest(&subject, payload_digest)?;
        self.push_compact_classification(
            owner,
            &subject,
            SemanticDependencyProjectionClassV4::Structural,
            local_digest,
            None,
        );
        #[cfg(test)]
        if self.retain_exhaustive {
            self.coverage.push(SemanticDependencyCoverageV1 {
                id: SemanticDependencyCoverageId(self.coverage.len()),
                subject,
                primary_owner: owner,
                disposition: SemanticDependencyCoverageDispositionV1::Structural { payload_digest },
            });
        }
        Ok(CheckedProgramDigestV1(checked_program_digest))
    }

    fn structural_with_component_digest(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: SemanticDependencySubjectV1,
        payload: &impl Serialize,
    ) -> Result<[u8; 32], CallableDependencyManifestError> {
        self.claim_subject(&subject)?;
        let [payload_digest, component_digest] =
            boon_contract::canonical_serde_hashes_v1_streaming(
                [
                    DEPENDENCY_RECORD_PAYLOAD_DOMAIN,
                    DEPENDENCY_COMPONENT_DIGEST_DOMAIN,
                ],
                payload,
            )
            .map_err(|error| {
                CallableDependencyManifestError::new(format!(
                    "failed to hash dependency component payload: {error}"
                ))
            })?;
        let local_digest = self.compact_local_classification_digest(&subject, payload_digest)?;
        self.push_compact_classification(
            owner,
            &subject,
            SemanticDependencyProjectionClassV4::Structural,
            local_digest,
            None,
        );
        #[cfg(test)]
        if self.retain_exhaustive {
            self.coverage.push(SemanticDependencyCoverageV1 {
                id: SemanticDependencyCoverageId(self.coverage.len()),
                subject,
                primary_owner: owner,
                disposition: SemanticDependencyCoverageDispositionV1::Structural { payload_digest },
            });
        }
        Ok(component_digest)
    }

    fn diagnostic(
        &mut self,
        owner: SemanticDependencyOwnerV1,
        subject: SemanticDependencySubjectV1,
        payload: &impl Serialize,
    ) -> Result<(), CallableDependencyManifestError> {
        self.claim_subject(&subject)?;
        let payload_digest = dependency_payload_digest(payload, &mut self.hash_scratch)?;
        let local_digest = self.compact_local_classification_digest(&subject, payload_digest)?;
        self.push_compact_classification(
            owner,
            &subject,
            SemanticDependencyProjectionClassV4::Diagnostic,
            local_digest,
            None,
        );
        #[cfg(test)]
        if self.retain_exhaustive {
            self.coverage.push(SemanticDependencyCoverageV1 {
                id: SemanticDependencyCoverageId(self.coverage.len()),
                subject,
                primary_owner: owner,
                disposition: SemanticDependencyCoverageDispositionV1::Diagnostic { payload_digest },
            });
        }
        Ok(())
    }

    fn claim_subject(
        &mut self,
        subject: &SemanticDependencySubjectV1,
    ) -> Result<(), CallableDependencyManifestError> {
        if !self.subjects.insert(subject.clone()) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency coverage subject {subject:?} is classified more than once"
            )));
        }
        Ok(())
    }

    #[cfg(test)]
    fn finish(
        self,
        owners: &BTreeSet<SemanticDependencyOwnerV1>,
    ) -> Result<ValidatedDependencyCollection, CallableDependencyManifestError> {
        if !self.retain_exhaustive {
            return Err(CallableDependencyManifestError::new(
                "V3 exhaustive proof finish was requested from a compact-only collector",
            ));
        }
        let trace_dependency_manifest = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
        if trace_dependency_manifest {
            eprintln!(
                "boon_semantic dependency_manifest finish:start owners={} pending={} coverage={}",
                owners.len(),
                self.records.len(),
                self.coverage.len()
            );
        }
        let DependencyCollector {
            compact_rows: _,
            compact_records: _,
            compact_references: _,
            mut records,
            unresolved_entity_references,
            coverage,
            subjects,
            mut dependencies_by_entity,
            hash_scratch,
            flow_type_digests,
            retain_exhaustive: _,
        } = self;
        if subjects.len() != coverage.len() {
            return Err(CallableDependencyManifestError::new(
                "dependency subject and coverage inventories are not aligned",
            ));
        }
        // Classification and hashing are complete. Release their large tables
        // before resolved dependency vectors begin allocating; only the entity
        // index participates in reference resolution.
        drop((subjects, flow_type_digests, hash_scratch));

        for ids in dependencies_by_entity.values_mut() {
            ids.sort();
            ids.dedup();
        }

        let records_started = trace_dependency_manifest.then(std::time::Instant::now);
        if records.len() != unresolved_entity_references.len() {
            return Err(CallableDependencyManifestError::new(
                "dependency records and unresolved-reference sidecar are not aligned",
            ));
        }
        for (record, references) in records
            .iter_mut()
            .zip(unresolved_entity_references.into_iter())
        {
            let record_id = record.id;
            let record_owner = record.owner;
            let mut validated_owner_references = 0usize;
            for reference in references {
                let preceding_owners = record
                    .referenced_owners
                    .get(validated_owner_references..reference.owner_references_before)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(
                            "dependency owner/entity reference partition is not aligned",
                        )
                    })?;
                for owner in preceding_owners {
                    if !owners.contains(owner) {
                        return Err(CallableDependencyManifestError::new(format!(
                            "dependency {} references missing owner {owner:?}",
                            record_id
                        )));
                    }
                }
                validated_owner_references = reference.owner_references_before;
                let targets = dependencies_by_entity.get(&reference.entity).ok_or_else(|| {
                    let entity = &reference.entity;
                    CallableDependencyManifestError::new(format!(
                        "dependency {} references entity {entity:?} with no dependency classification",
                        record_id
                    ))
                })?;
                record
                    .referenced_dependencies
                    .extend(targets.iter().copied());
            }
            for owner in &record.referenced_owners[validated_owner_references..] {
                if !owners.contains(owner) {
                    return Err(CallableDependencyManifestError::new(format!(
                        "dependency {} references missing owner {owner:?}",
                        record_id
                    )));
                }
            }
            record.referenced_dependencies.sort();
            record.referenced_dependencies.dedup();
            record
                .referenced_dependencies
                .retain(|dependency| *dependency != record_id);
            record.referenced_owners.sort();
            record.referenced_owners.dedup();
            record
                .referenced_owners
                .retain(|owner| *owner != record_owner);
        }
        drop(dependencies_by_entity);
        if trace_dependency_manifest {
            eprintln!(
                "boon_semantic dependency_manifest finish:records_done records={} elapsed_ms={:.3}",
                records.len(),
                records_started
                    .expect("traced dependency records have a start time")
                    .elapsed()
                    .as_secs_f64()
                    * 1000.0,
            );
        }

        let direct_started = trace_dependency_manifest.then(std::time::Instant::now);
        let mut direct = owners
            .iter()
            .copied()
            .map(|owner| (owner, Vec::new()))
            .collect::<BTreeMap<_, _>>();
        for record in &records {
            direct
                .get_mut(&record.owner)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "dependency {} has unregistered owner {:?}",
                        record.id, record.owner
                    ))
                })?
                .push(record.id);
        }
        for ids in direct.values_mut() {
            ids.sort();
            ids.dedup();
        }
        if trace_dependency_manifest {
            eprintln!(
                "boon_semantic dependency_manifest finish:direct_done elapsed_ms={:.3}",
                direct_started
                    .expect("traced direct dependencies have a start time")
                    .elapsed()
                    .as_secs_f64()
                    * 1000.0,
            );
        }

        let coverage_started = trace_dependency_manifest.then(std::time::Instant::now);
        for (index, entry) in coverage.iter().enumerate() {
            if entry.id != SemanticDependencyCoverageId(index) {
                return Err(CallableDependencyManifestError::new(
                    "dependency coverage IDs are not dense",
                ));
            }
            if !owners.contains(&entry.primary_owner) {
                return Err(CallableDependencyManifestError::new(format!(
                    "coverage {} references missing owner {:?}",
                    entry.id, entry.primary_owner
                )));
            }
            if let SemanticDependencyCoverageDispositionV1::Dependency { dependency } =
                &entry.disposition
            {
                let record = records
                    .get(dependency.as_usize())
                    .filter(|record| record.id == *dependency)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "coverage {} references missing dependency {dependency}",
                            entry.id
                        ))
                    })?;
                if record.subject != entry.subject || record.owner != entry.primary_owner {
                    return Err(CallableDependencyManifestError::new(format!(
                        "coverage {} disagrees with dependency {dependency}",
                        entry.id
                    )));
                }
            }
        }
        if trace_dependency_manifest {
            eprintln!(
                "boon_semantic dependency_manifest finish:done coverage_elapsed_ms={:.3}",
                coverage_started
                    .expect("traced dependency coverage has a start time")
                    .elapsed()
                    .as_secs_f64()
                    * 1000.0,
            );
        }
        Ok(ValidatedDependencyCollection {
            dependencies: records,
            coverage,
            direct,
        })
    }

    fn finish_compact_v4(
        mut self,
        owners: &BTreeSet<SemanticDependencyOwnerV1>,
        stable_owners: &BTreeMap<SemanticDependencyOwnerV1, SemanticDependencyStableOwnerV4>,
    ) -> Result<ValidatedCompactDependencyCollectionV4, CallableDependencyManifestError> {
        if self.subjects.len() != self.compact_rows.len() {
            return Err(CallableDependencyManifestError::new(
                "dependency subjects and compact coverage receipts are not aligned",
            ));
        }
        if self.compact_records.len() != self.dependencies_by_entity.values().flatten().count() {
            return Err(CallableDependencyManifestError::new(
                "compact dependency records and entity classifications are not aligned",
            ));
        }
        if stable_owners.len() != owners.len()
            || owners
                .iter()
                .any(|owner| !stable_owners.contains_key(owner))
        {
            return Err(CallableDependencyManifestError::new(
                "compact dependency proof does not have one stable identity for every owner",
            ));
        }
        let unique_stable_owners = stable_owners.values().copied().collect::<BTreeSet<_>>();
        if unique_stable_owners.len() != stable_owners.len() {
            return Err(CallableDependencyManifestError::new(
                "compact dependency proof stable owner identities are not unique",
            ));
        }
        for (index, row) in self.compact_rows.iter().enumerate() {
            if !owners.contains(&row.projection.owner) {
                return Err(CallableDependencyManifestError::new(format!(
                    "compact coverage row {index} references missing owner {:?}",
                    row.projection.owner
                )));
            }
            if let Some(dependency) = row.dependency
                && self
                    .compact_records
                    .get(dependency.as_usize())
                    .is_none_or(|record| record.row != index)
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "compact coverage row {index} disagrees with dependency {dependency}",
                )));
            }
        }
        for ids in self.dependencies_by_entity.values_mut() {
            ids.sort();
            ids.dedup();
        }

        let stable_projection = |dense: DenseDependencyProjectionKeyV4| {
            stable_owners
                .get(&dense.owner)
                .copied()
                .map(|owner| SemanticDependencyProjectionKeyV4 {
                    owner,
                    subject_kind: dense.subject_kind,
                    class: dense.class,
                })
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "compact projection references missing owner {:?}",
                        dense.owner
                    ))
                })
        };

        let mut final_row_digests = vec![None; self.compact_rows.len()];
        let mut receipt_hash_cache = CompactReceiptHashCacheV4::default();
        let mut projection_targets = BTreeMap::<
            SemanticDependencyProjectionKeyV4,
            BTreeSet<DependencyProjectionNodeV4>,
        >::new();
        for (index, record) in self.compact_records.iter().copied().enumerate() {
            let record_id = SemanticDependencyRecordId(index);
            let row = self.compact_rows.get(record.row).ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "compact dependency {record_id} references missing row {}",
                    record.row
                ))
            })?;
            if row.dependency != Some(record_id) {
                return Err(CallableDependencyManifestError::new(format!(
                    "compact dependency {record_id} is not bound by its exact coverage row",
                )));
            }
            let source_projection = stable_projection(row.projection)?;
            let references = self
                .compact_references
                .get(record.references_start..record.references_end)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "compact dependency {record_id} has an invalid reference span"
                    ))
                })?;
            let mut targets = Vec::new();
            for reference in references {
                match reference {
                    PendingDependencyReference::Owner(owner) => {
                        if !owners.contains(owner) {
                            return Err(CallableDependencyManifestError::new(format!(
                                "dependency {record_id} references missing owner {owner:?}"
                            )));
                        }
                        if *owner != row.projection.owner {
                            targets.push(DependencyProjectionNodeV4::Owner(stable_owners[owner]));
                        }
                    }
                    PendingDependencyReference::Entity(entity) => {
                        let dependencies =
                            self.dependencies_by_entity.get(entity).ok_or_else(|| {
                                CallableDependencyManifestError::new(format!(
                                    "dependency {record_id} references entity {entity:?} with no dependency classification"
                                ))
                            })?;
                        for dependency in dependencies.iter().copied() {
                            if dependency == record_id {
                                continue;
                            }
                            let target_record = self
                                .compact_records
                                .get(dependency.as_usize())
                                .ok_or_else(|| {
                                    CallableDependencyManifestError::new(format!(
                                        "dependency {record_id} references missing dependency {dependency}"
                                    ))
                                })?;
                            let target_row =
                                self.compact_rows.get(target_record.row).ok_or_else(|| {
                                    CallableDependencyManifestError::new(format!(
                                        "dependency {dependency} references missing compact row {}",
                                        target_record.row
                                    ))
                                })?;
                            targets.push(DependencyProjectionNodeV4::Projection(
                                stable_projection(target_row.projection)?,
                            ));
                        }
                    }
                }
            }
            targets.sort();
            targets.dedup();
            let final_digest = compact_projection_row_digest_v4(
                source_projection,
                row.local_digest,
                &targets,
                &mut receipt_hash_cache,
            )?;
            final_row_digests[record.row] = Some(final_digest);
            projection_targets
                .entry(source_projection)
                .or_default()
                .extend(targets);
        }

        for (index, row) in self.compact_rows.iter().enumerate() {
            if final_row_digests[index].is_some() {
                continue;
            }
            if row.dependency.is_some() {
                return Err(CallableDependencyManifestError::new(format!(
                    "compact dependency coverage row {index} has no dependency record"
                )));
            }
            let projection = stable_projection(row.projection)?;
            final_row_digests[index] = Some(compact_projection_row_digest_v4(
                projection,
                row.local_digest,
                &[],
                &mut receipt_hash_cache,
            )?);
            projection_targets.entry(projection).or_default();
        }
        let final_row_digests = final_row_digests
            .into_iter()
            .enumerate()
            .map(|(index, digest)| {
                digest.ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "compact coverage row {index} has no receipt digest"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let dependency_row_digests = self
            .compact_records
            .iter()
            .map(|record| final_row_digests[record.row])
            .collect::<Vec<_>>();
        let dependency_rows_digest = compact_digest_sequence_v4(
            DEPENDENCY_ROW_RECEIPT_SET_DOMAIN_V4,
            &dependency_row_digests,
        )?;
        let coverage_receipts_digest = compact_digest_sequence_v4(
            DEPENDENCY_COVERAGE_RECEIPT_SET_DOMAIN_V4,
            &final_row_digests,
        )?;

        let mut projection_rows =
            BTreeMap::<SemanticDependencyProjectionKeyV4, Vec<[u8; 32]>>::new();
        for (row, digest) in self.compact_rows.iter().zip(&final_row_digests) {
            projection_rows
                .entry(stable_projection(row.projection)?)
                .or_default()
                .push(*digest);
        }
        let mut projection_receipts = BTreeMap::new();
        for (projection, rows) in projection_rows {
            projection_receipts.insert(
                projection,
                compact_projection_receipt_digest_v4(projection, &rows)?,
            );
        }
        let projection_receipts_digest = compact_digest_sequence_v4(
            DEPENDENCY_PROJECTION_RECEIPT_SET_DOMAIN_V4,
            &projection_receipts.values().copied().collect::<Vec<_>>(),
        )?;
        let (stable_implementation_digests, graph_stats) =
            build_dependency_projection_graph_digests_v4(
                &unique_stable_owners,
                &projection_receipts,
                &projection_targets,
            )?;
        let implementation_digests = stable_owners
            .iter()
            .map(|(dense, stable)| {
                stable_implementation_digests
                    .get(stable)
                    .copied()
                    .map(|digest| (*dense, digest))
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "stable dependency owner {stable:?} has no implementation digest"
                        ))
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        if std::env::var_os("BOON_SEMANTIC_TRACE").is_some() {
            eprintln!(
                "boon_semantic dependency_manifest projection_graph:counts nodes={} edges={} components={} cyclic_components={} maximum_component_nodes={} component_edges={}",
                graph_stats.nodes,
                graph_stats.edges,
                graph_stats.components,
                graph_stats.cyclic_components,
                graph_stats.maximum_component_nodes,
                graph_stats.component_edges,
            );
        }
        Ok(ValidatedCompactDependencyCollectionV4 {
            implementation_digests,
            dependency_rows_digest,
            coverage_receipts_digest,
            projection_receipts_digest,
            dependency_record_count: self.compact_records.len(),
            coverage_record_count: self.compact_rows.len(),
            projection_count: projection_receipts.len(),
            projection_edge_count: graph_stats.edges,
        })
    }
}

fn compact_projection_row_digest_v4(
    projection: SemanticDependencyProjectionKeyV4,
    local_digest: [u8; 32],
    targets: &[DependencyProjectionNodeV4],
    cache: &mut CompactReceiptHashCacheV4,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_PROJECTION_ROW_DOMAIN_V4);
    hasher.update(cache.projection_digest(projection)?);
    hasher.update(local_digest);
    dependency_proof_update_usize(&mut hasher, targets.len(), "projection row target count")?;
    for target in targets {
        hasher.update(cache.node_digest(*target)?);
    }
    Ok(hasher.finalize().into())
}

#[derive(Default)]
struct CompactReceiptHashCacheV4 {
    owners: BTreeMap<SemanticDependencyStableOwnerV4, [u8; 32]>,
    projections: BTreeMap<SemanticDependencyProjectionKeyV4, [u8; 32]>,
    nodes: BTreeMap<DependencyProjectionNodeV4, [u8; 32]>,
}

impl CompactReceiptHashCacheV4 {
    fn owner_digest(
        &mut self,
        owner: SemanticDependencyStableOwnerV4,
    ) -> Result<[u8; 32], CallableDependencyManifestError> {
        if let Some(digest) = self.owners.get(&owner) {
            return Ok(*digest);
        }
        let digest = canonical_dependency_hash(DEPENDENCY_STABLE_OWNER_DOMAIN_V4, &owner)?;
        self.owners.insert(owner, digest);
        Ok(digest)
    }

    fn projection_digest(
        &mut self,
        projection: SemanticDependencyProjectionKeyV4,
    ) -> Result<[u8; 32], CallableDependencyManifestError> {
        if let Some(digest) = self.projections.get(&projection) {
            return Ok(*digest);
        }
        let digest = canonical_dependency_hash(DEPENDENCY_PROJECTION_KEY_DOMAIN_V4, &projection)?;
        self.projections.insert(projection, digest);
        Ok(digest)
    }

    fn node_digest(
        &mut self,
        node: DependencyProjectionNodeV4,
    ) -> Result<[u8; 32], CallableDependencyManifestError> {
        if let Some(digest) = self.nodes.get(&node) {
            return Ok(*digest);
        }
        let mut hasher = Sha256::new();
        hasher.update(DEPENDENCY_PROJECTION_NODE_DOMAIN_V4);
        match node {
            DependencyProjectionNodeV4::Owner(owner) => {
                hasher.update([0]);
                hasher.update(self.owner_digest(owner)?);
            }
            DependencyProjectionNodeV4::Projection(projection) => {
                hasher.update([1]);
                hasher.update(self.projection_digest(projection)?);
            }
        }
        let digest = hasher.finalize().into();
        self.nodes.insert(node, digest);
        Ok(digest)
    }
}

fn compact_local_dependency_digest_v4(
    _roles: &[SemanticDependencyRoleV1],
    _subject: &SemanticDependencySubjectV1,
    _semantics: &SemanticDependencyRecordSemanticsV1,
    payload_digest: [u8; 32],
    _scratch: &mut Vec<u8>,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    // The payload digest commits the authoritative checked/semantic row. The
    // manifest's classifier-schema digest commits the deterministic mapping
    // from that row to subject, roles, semantics, and projection; the final
    // receipt separately commits the projection key and dependency targets.
    // Re-encoding those derived fields for every row recreated the V3 proof
    // multiplier and is deliberately not part of the V4 byte contract.
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_PROJECTION_LOCAL_ROW_DOMAIN_V4);
    hasher.update(payload_digest);
    Ok(hasher.finalize().into())
}

fn compact_local_classification_digest_v4(
    _subject: &SemanticDependencySubjectV1,
    payload_digest: [u8; 32],
    _scratch: &mut Vec<u8>,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    // See `compact_local_dependency_digest_v4`: classification metadata is a
    // schema-bound projection of this exact authoritative payload.
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_PROJECTION_LOCAL_ROW_DOMAIN_V4);
    hasher.update(payload_digest);
    Ok(hasher.finalize().into())
}

fn compact_digest_sequence_v4(
    domain: &[u8],
    digests: &[[u8; 32]],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    dependency_proof_update_usize(&mut hasher, digests.len(), "compact receipt count")?;
    for digest in digests {
        hasher.update(digest);
    }
    Ok(hasher.finalize().into())
}

fn compact_projection_receipt_digest_v4(
    projection: SemanticDependencyProjectionKeyV4,
    rows: &[[u8; 32]],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let projection_digest =
        canonical_dependency_hash(DEPENDENCY_PROJECTION_KEY_DOMAIN_V4, &projection)?;
    let mut hasher = Sha256::new();
    hasher.update(DEPENDENCY_PROJECTION_RECEIPT_DOMAIN_V4);
    hasher.update(projection_digest);
    dependency_proof_update_usize(&mut hasher, rows.len(), "projection receipt row count")?;
    for row in rows {
        hasher.update(row);
    }
    Ok(hasher.finalize().into())
}

fn dependency_projection_node_digest_v4(
    node: DependencyProjectionNodeV4,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    canonical_dependency_hash(DEPENDENCY_PROJECTION_NODE_DOMAIN_V4, &node)
}

fn stable_dependency_owner_index_v4(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    owners: &BTreeSet<SemanticDependencyOwnerV1>,
) -> Result<
    BTreeMap<SemanticDependencyOwnerV1, SemanticDependencyStableOwnerV4>,
    CallableDependencyManifestError,
> {
    #[derive(Serialize)]
    struct CallableIdentity<'a> {
        kind: boon_checked::CheckedCallableKind,
        name: &'a str,
        external_identity: &'a Option<boon_checked::CheckedExternalDeclarationIdentityV1>,
        role: ProgramRole,
    }

    let mut stable = BTreeMap::new();
    stable.insert(
        SemanticDependencyOwnerV1::ProgramRoot,
        SemanticDependencyStableOwnerV4::ProgramRoot { role: checked.role },
    );
    for callable in &execution.callables {
        let dense = SemanticDependencyOwnerV1::Callable {
            callable: callable.id,
        };
        if !owners.contains(&dense) {
            return Err(CallableDependencyManifestError::new(format!(
                "semantic callable {} has no dependency owner",
                callable.id
            )));
        }
        let identity = canonical_dependency_hash(
            DEPENDENCY_STABLE_OWNER_DOMAIN_V4,
            &CallableIdentity {
                kind: callable.kind,
                name: &callable.name,
                external_identity: &callable.external_identity,
                role: callable.role,
            },
        )?;
        if stable
            .insert(
                dense,
                SemanticDependencyStableOwnerV4::Callable { identity },
            )
            .is_some()
        {
            return Err(CallableDependencyManifestError::new(format!(
                "semantic callable {} has duplicate dependency ownership",
                callable.id
            )));
        }
    }
    if stable.len() != owners.len() {
        return Err(CallableDependencyManifestError::new(format!(
            "stable dependency owner index has {} entries for {} owners",
            stable.len(),
            owners.len()
        )));
    }
    let mut reverse = BTreeMap::new();
    for (dense, identity) in &stable {
        if let Some(previous) = reverse.insert(*identity, *dense) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency owners {previous:?} and {dense:?} share stable identity {identity:?}"
            )));
        }
    }
    Ok(stable)
}

fn build_dependency_projection_graph_digests_v4(
    owners: &BTreeSet<SemanticDependencyStableOwnerV4>,
    projection_receipts: &BTreeMap<SemanticDependencyProjectionKeyV4, [u8; 32]>,
    projection_targets: &BTreeMap<
        SemanticDependencyProjectionKeyV4,
        BTreeSet<DependencyProjectionNodeV4>,
    >,
) -> Result<
    (
        BTreeMap<SemanticDependencyStableOwnerV4, [u8; 32]>,
        DependencyGraphDigestStats,
    ),
    CallableDependencyManifestError,
> {
    let mut graph = RequestGraphBuilder::new();
    for owner in owners.iter().copied() {
        let node = DependencyProjectionNodeV4::Owner(owner);
        graph
            .insert(
                node,
                dependency_projection_node_digest_v4(node)?,
                canonical_dependency_hash(DEPENDENCY_PROJECTION_NODE_DOMAIN_V4, &owner)?,
            )
            .map_err(|error| CallableDependencyManifestError::new(error.to_string()))?;
    }
    for (projection, receipt) in projection_receipts {
        let node = DependencyProjectionNodeV4::Projection(*projection);
        graph
            .insert(node, dependency_projection_node_digest_v4(node)?, *receipt)
            .map_err(|error| CallableDependencyManifestError::new(error.to_string()))?;
        graph.add_dependency(DependencyProjectionNodeV4::Owner(projection.owner), node);
    }
    for (projection, targets) in projection_targets {
        let source = DependencyProjectionNodeV4::Projection(*projection);
        for target in targets.iter().copied() {
            graph.add_dependency(source, target);
        }
    }
    let graph = graph
        .seal(RequestGraphDigestDomains {
            component: DEPENDENCY_PROJECTION_COMPONENT_DOMAIN_V4,
        })
        .map_err(|error| CallableDependencyManifestError::new(error.to_string()))?;
    let mut implementation_digests = BTreeMap::new();
    for owner in owners.iter().copied() {
        let digest = graph
            .implementation_digest(
                &DependencyProjectionNodeV4::Owner(owner),
                DEPENDENCY_PROJECTION_IMPLEMENTATION_DOMAIN_V4,
            )
            .map_err(|error| CallableDependencyManifestError::new(error.to_string()))?;
        implementation_digests.insert(owner, digest);
    }
    let stats = graph.stats();
    Ok((
        implementation_digests,
        DependencyGraphDigestStats {
            nodes: stats.nodes,
            edges: stats.edges,
            components: stats.components,
            cyclic_components: stats.cyclic_components,
            maximum_component_nodes: stats.maximum_component_nodes,
            component_edges: stats.component_edges,
        },
    ))
}

fn top_subject(
    kind: SemanticDependencySubjectKindV1,
    identity: SemanticDependencyEntityV1,
) -> SemanticDependencySubjectV1 {
    SemanticDependencySubjectV1 {
        kind,
        identity,
        parent: None,
        child_ordinal: None,
    }
}

fn child_subject(
    kind: SemanticDependencySubjectKindV1,
    parent: SemanticDependencyEntityV1,
    ordinal: usize,
) -> SemanticDependencySubjectV1 {
    SemanticDependencySubjectV1 {
        kind,
        identity: parent.clone(),
        parent: Some(parent),
        child_ordinal: Some(ordinal),
    }
}

fn indexed_entity(
    domain: SemanticDependencyEntityDomainV1,
    index: usize,
) -> SemanticDependencyEntityV1 {
    SemanticDependencyEntityV1::indexed(domain, index)
}

fn expression_entity(expression: SemanticExprId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticExpression,
        expression.as_usize(),
    )
}

fn statement_entity(statement: SemanticStatementId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticStatement,
        statement.as_usize(),
    )
}

fn callable_entity(callable: SemanticCallableId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticCallable,
        callable.as_usize(),
    )
}

fn call_entity(call: SemanticCallId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticCall,
        call.as_usize(),
    )
}

fn out_call_entity(call: OutCallInstanceId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::OutCallInstance,
        call.as_usize(),
    )
}

fn source_entity(source: SemanticSourceId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticSource,
        source.as_usize(),
    )
}

fn state_entity(state: SemanticStateId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticState,
        state.as_usize(),
    )
}

fn activation_entity(activation: SemanticActivationId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticActivation,
        activation.as_usize(),
    )
}

fn pulse_batch_entity(pulse: SemanticPulseBatchId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticPulseBatch,
        pulse.as_usize(),
    )
}

fn list_entity(list: SemanticListId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticList,
        list.as_usize(),
    )
}

fn value_list_entity(list: SemanticValueListAuthorityId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticValueListAuthority,
        list.as_usize(),
    )
}

fn binding_entity(binding: SemanticBindingId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticBinding,
        binding.as_usize(),
    )
}

fn read_entity(read: SemanticReadId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticRead,
        read.as_usize(),
    )
}

fn field_entity(field: SemanticFieldId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticField,
        field.as_usize(),
    )
}

fn static_owner_entity(owner: StaticOwnerId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::StaticOwner,
        owner.as_usize(),
    )
}

fn storage_local_entity(
    owner: StaticOwnerId,
    local: SemanticMaterializationLocalId,
) -> SemanticDependencyEntityV1 {
    SemanticDependencyEntityV1::Composite {
        domain: SemanticDependencyEntityDomainV1::SemanticStorageLocal,
        parent_index: owner.as_usize() as u64,
        child_ordinal: local.as_usize(),
    }
}

fn view_root_entity(root: SemanticViewRootId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewRoot,
        root.as_usize(),
    )
}

fn view_node_entity(node: SemanticViewNodeId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewNode,
        node.as_usize(),
    )
}

fn view_argument_entity(argument: SemanticViewArgumentId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewArgument,
        argument.as_usize(),
    )
}

fn view_binding_entity(binding: SemanticViewBindingId) -> SemanticDependencyEntityV1 {
    indexed_entity(
        SemanticDependencyEntityDomainV1::SemanticViewBinding,
        binding.as_usize(),
    )
}

fn dependency_entity(entity: SemanticDependencyEntityV1) -> PendingDependencyReference {
    PendingDependencyReference::Entity(entity)
}

fn dependency_owner(owner: SemanticDependencyOwnerV1) -> PendingDependencyReference {
    PendingDependencyReference::Owner(owner)
}

fn dependency_payload_digest(
    payload: &impl Serialize,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    boon_contract::canonical_serde_hash_v1_with_buffer(
        DEPENDENCY_RECORD_PAYLOAD_DOMAIN,
        payload,
        scratch,
    )
    .map_err(|error| {
        CallableDependencyManifestError::new(format!(
            "failed to hash dependency record payload: {error}"
        ))
    })
}

fn canonical_dependency_hash(
    domain: &[u8],
    payload: &impl Serialize,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    boon_contract::canonical_serde_hash_v1(domain, payload).map_err(|error| {
        CallableDependencyManifestError::new(format!(
            "failed to hash callable dependency payload: {error}"
        ))
    })
}

fn canonical_dependency_hash_streaming(
    domain: &[u8],
    payload: &(impl Serialize + ?Sized),
) -> Result<[u8; 32], CallableDependencyManifestError> {
    boon_contract::canonical_serde_hash_v1_streaming(domain, payload).map_err(|error| {
        CallableDependencyManifestError::new(format!(
            "failed to stream callable dependency payload: {error}"
        ))
    })
}

#[cfg(test)]
fn canonical_dependency_hash_with_buffer(
    domain: &[u8],
    payload: &impl Serialize,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    boon_contract::canonical_serde_hash_v1_with_buffer(domain, payload, scratch).map_err(|error| {
        CallableDependencyManifestError::new(format!(
            "failed to hash callable dependency payload: {error}"
        ))
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_callable_dependency_manifest(
    dependency_classifier_schema_digest: [u8; 32],
    checked: &CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
    view: &SemanticViewBindingGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    memory: &SemanticMemoryGraphV1,
) -> Result<CallableDependencyManifestV4, CallableDependencyManifestError> {
    let trace_dependency_manifest = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    macro_rules! dependency_manifest_phase_v4 {
        ($name:literal, $expression:expr) => {{
            let started = trace_dependency_manifest.then(std::time::Instant::now);
            if trace_dependency_manifest {
                eprintln!(concat!(
                    "boon_semantic dependency_manifest ",
                    $name,
                    ":start"
                ));
            }
            let result = $expression;
            if let Some(started) = started {
                eprintln!(
                    concat!(
                        "boon_semantic dependency_manifest ",
                        $name,
                        ":done elapsed_ms={:.3}"
                    ),
                    started.elapsed().as_secs_f64() * 1000.0,
                );
            }
            result
        }};
    }
    macro_rules! dependency_manifest_inventory_phase_v4 {
        ($name:literal, $expression:expr, $collector:expr) => {{
            let result = dependency_manifest_phase_v4!($name, $expression);
            if trace_dependency_manifest {
                $collector.trace_counts($name);
            }
            result
        }};
    }

    if checked.source_bundle_digest_v1 != lowering.metadata.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != view.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != storage.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != memory.source_bundle_digest_v1
    {
        return Err(CallableDependencyManifestError::new(
            "dependency manifest inputs disagree on source-bundle identity",
        ));
    }
    let owner_index = dependency_manifest_phase_v4!(
        "owner_index",
        DependencyOwnerIndex::derive(
            checked, out, execution, resources, reactive, storage, memory,
        )
    )?;
    let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
    owners.extend(
        execution
            .callables
            .iter()
            .map(|callable| SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }),
    );
    let stable_owners = dependency_manifest_phase_v4!(
        "stable_owner_index",
        stable_dependency_owner_index_v4(checked, execution, &owners)
    )?;
    let mut collector = DependencyCollector::for_program(checked, execution, false);
    let checked_program_digest = dependency_manifest_inventory_phase_v4!(
        "inventory_checked",
        inventory_checked(checked, execution, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase_v4!(
        "inventory_producer_requests",
        inventory_producer_requests(producer_materializations, &owner_index, &mut collector),
        collector
    )?;
    let resolved_out_graph_digest = dependency_manifest_inventory_phase_v4!(
        "inventory_out",
        inventory_out(out, &owner_index, &mut collector),
        collector
    )?;
    let execution_graph_digest = dependency_manifest_inventory_phase_v4!(
        "inventory_execution",
        inventory_execution(execution, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase_v4!(
        "inventory_resources",
        inventory_resources(resources, execution, &owner_index, &mut collector),
        collector
    )?;
    let reactive_graph_digest = dependency_manifest_inventory_phase_v4!(
        "inventory_reactive",
        inventory_reactive(reactive, execution, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase_v4!(
        "inventory_lowering",
        inventory_lowering(lowering, execution, resources, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase_v4!(
        "inventory_view",
        inventory_view(view, execution, reactive, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase_v4!(
        "inventory_storage",
        inventory_storage(
            storage,
            execution,
            resources,
            reactive,
            &owner_index,
            &mut collector,
        ),
        collector
    )?;
    dependency_manifest_inventory_phase_v4!(
        "inventory_memory",
        inventory_memory(memory, execution, &owner_index, &mut collector),
        collector
    )?;

    let compact = dependency_manifest_phase_v4!(
        "finish_projection_receipts",
        collector.finish_compact_v4(&owners, &stable_owners)
    )?;
    let component_digests = dependency_manifest_phase_v4!(
        "component_digests",
        CallableDependencyComponentDigestsV1 {
            producer_materializations: canonical_dependency_hash(
                DEPENDENCY_COMPONENT_DIGEST_DOMAIN,
                &producer_materializations,
            )?,
            resolved_out_graph: resolved_out_graph_digest,
            execution_graph: execution_graph_digest,
            resource_graph: *resources.digest.as_bytes(),
            reactive_graph: reactive_graph_digest,
            lowering_contract: *lowering.digest.as_bytes(),
            view_binding_graph: *view.digest.as_bytes(),
            scope_storage_graph: *storage.digest.as_bytes(),
            memory_graph: *memory.digest.as_bytes(),
        }
    );

    let root_owner = SemanticDependencyOwnerV1::ProgramRoot;
    let program_root = ProgramRootDependencyEntryV4 {
        stable_owner: stable_owners[&root_owner],
        public_shape_digest: canonical_dependency_hash(
            DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
            &(checked.role, checked.source_bundle_digest_v1),
        )?,
        implementation_dependency_digest: compact.implementation_digests[&root_owner],
    };
    let mut callable_entries = Vec::with_capacity(execution.callables.len());
    for (index, callable) in execution.callables.iter().enumerate() {
        if callable.id != SemanticCallableId(index) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency manifest callable {} is not dense index {index}",
                callable.id
            )));
        }
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: callable.id,
        };
        callable_entries.push(CallableDependencyEntryV4 {
            callable: callable.id,
            checked_callable: callable.checked_callable,
            stable_owner: stable_owners[&owner],
            public_shape_digest: callable_public_shape_digest(callable)?,
            implementation_dependency_digest: compact.implementation_digests[&owner],
        });
    }

    let program_root_entry_digest = canonical_dependency_hash_streaming(
        DEPENDENCY_PROGRAM_ROOT_ENTRY_DOMAIN_V4,
        &program_root,
    )?;
    let callable_entries_digest =
        canonical_dependency_hash_streaming(DEPENDENCY_CALLABLE_SET_DOMAIN_V4, &callable_entries)?;
    let proof_digests = CallableDependencyProofDigestsV4 {
        program_root_entry_digest,
        callable_entries_digest,
        dependency_rows_digest: compact.dependency_rows_digest,
        coverage_receipts_digest: compact.coverage_receipts_digest,
        projection_receipts_digest: compact.projection_receipts_digest,
        dependency_record_count: compact.dependency_record_count,
        coverage_record_count: compact.coverage_record_count,
        projection_count: compact.projection_count,
        projection_edge_count: compact.projection_edge_count,
    };
    let schema = CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V4.to_owned();
    let source_bundle_digest_v1 = checked.source_bundle_digest_v1;
    let dependency_classifier_schema_digest =
        DependencyClassifierSchemaDigestV1(dependency_classifier_schema_digest);
    let manifest_digest = callable_dependency_manifest_digest_v4(
        &schema,
        source_bundle_digest_v1,
        checked_program_digest,
        dependency_classifier_schema_digest,
        &component_digests,
        &program_root,
        &callable_entries,
        &proof_digests,
    )?;
    let mut manifest = CallableDependencyManifestV4 {
        schema,
        source_bundle_digest_v1,
        checked_program_digest,
        dependency_classifier_schema_digest,
        component_digests,
        program_root,
        callable_entries,
        proof_digests,
        manifest_digest,
        sealed_manifest_digest: [0; 32],
    };
    manifest.sealed_manifest_digest = sealed_callable_dependency_manifest_digest_v4(&manifest)?;
    Ok(manifest)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn build_callable_dependency_construction(
    dependency_classifier_schema_digest: [u8; 32],
    checked: &CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
    view: &SemanticViewBindingGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    memory: &SemanticMemoryGraphV1,
    retention: ExhaustiveProofRetention,
) -> Result<ValidatedCallableDependencyConstructionV3, CallableDependencyManifestError> {
    let trace_dependency_manifest = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    macro_rules! dependency_manifest_phase {
        ($name:literal, $expression:expr) => {{
            let started = trace_dependency_manifest.then(std::time::Instant::now);
            if trace_dependency_manifest {
                eprintln!(concat!(
                    "boon_semantic dependency_manifest ",
                    $name,
                    ":start"
                ));
            }
            let result = $expression;
            if let Some(started) = started {
                eprintln!(
                    concat!(
                        "boon_semantic dependency_manifest ",
                        $name,
                        ":done elapsed_ms={:.3}"
                    ),
                    started.elapsed().as_secs_f64() * 1000.0,
                );
            }
            result
        }};
    }

    macro_rules! dependency_manifest_inventory_phase {
        ($name:literal, $expression:expr, $collector:expr) => {{
            let result = dependency_manifest_phase!($name, $expression);
            if trace_dependency_manifest {
                $collector.trace_counts($name);
            }
            result
        }};
    }

    if checked.source_bundle_digest_v1 != lowering.metadata.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != view.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != storage.source_bundle_digest_v1
        || checked.source_bundle_digest_v1 != memory.source_bundle_digest_v1
    {
        return Err(CallableDependencyManifestError::new(
            "dependency manifest inputs disagree on source-bundle identity",
        ));
    }

    let owner_index = dependency_manifest_phase!(
        "owner_index",
        DependencyOwnerIndex::derive(
            checked, out, execution, resources, reactive, storage, memory,
        )
    )?;
    let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
    owners.extend(
        execution
            .callables
            .iter()
            .map(|callable| SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }),
    );

    #[cfg(test)]
    let retain_exhaustive = matches!(retention, ExhaustiveProofRetention::Retain);
    #[cfg(not(test))]
    let retain_exhaustive = false;
    let mut collector = DependencyCollector::for_program(checked, execution, retain_exhaustive);
    let checked_program_digest = dependency_manifest_inventory_phase!(
        "inventory_checked",
        inventory_checked(checked, execution, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase!(
        "inventory_producer_requests",
        inventory_producer_requests(producer_materializations, &owner_index, &mut collector),
        collector
    )?;
    let resolved_out_graph_digest = dependency_manifest_inventory_phase!(
        "inventory_out",
        inventory_out(out, &owner_index, &mut collector),
        collector
    )?;
    let execution_graph_digest = dependency_manifest_inventory_phase!(
        "inventory_execution",
        inventory_execution(execution, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase!(
        "inventory_resources",
        inventory_resources(resources, execution, &owner_index, &mut collector),
        collector
    )?;
    let reactive_graph_digest = dependency_manifest_inventory_phase!(
        "inventory_reactive",
        inventory_reactive(reactive, execution, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase!(
        "inventory_lowering",
        inventory_lowering(lowering, execution, resources, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase!(
        "inventory_view",
        inventory_view(view, execution, reactive, &owner_index, &mut collector),
        collector
    )?;
    dependency_manifest_inventory_phase!(
        "inventory_storage",
        inventory_storage(
            storage,
            execution,
            resources,
            reactive,
            &owner_index,
            &mut collector,
        ),
        collector
    )?;
    dependency_manifest_inventory_phase!(
        "inventory_memory",
        inventory_memory(memory, execution, &owner_index, &mut collector),
        collector
    )?;

    let ValidatedDependencyCollection {
        dependencies,
        coverage,
        mut direct,
    } = dependency_manifest_phase!("finish", collector.finish(&owners))?;
    let coverage_record_count = coverage.len();
    let coverage_digest =
        dependency_manifest_phase!("coverage_digest", dependency_coverage_digest(&coverage))?;
    let retained_coverage = Some(coverage);

    let dependency_record_count = dependencies.len();
    let owner_list = owners.iter().copied().collect::<Vec<_>>();
    let (graph_digests, graph_stats, dependency_records_digest) = dependency_manifest_phase!(
        "dependency_graph_digests",
        build_dependency_graph_digests(&owner_list, &dependencies, &direct)
    )?;
    if trace_dependency_manifest {
        eprintln!(
            "boon_semantic dependency_manifest graph:counts nodes={} edges={} components={} cyclic_components={} maximum_component_nodes={} component_edges={}",
            graph_stats.nodes,
            graph_stats.edges,
            graph_stats.components,
            graph_stats.cyclic_components,
            graph_stats.maximum_component_nodes,
            graph_stats.component_edges,
        );
    }
    let retained_dependencies = Some(dependencies);

    let component_digests = dependency_manifest_phase!("component_digests", {
        CallableDependencyComponentDigestsV1 {
            producer_materializations: canonical_dependency_hash(
                DEPENDENCY_COMPONENT_DIGEST_DOMAIN,
                &producer_materializations,
            )?,
            resolved_out_graph: resolved_out_graph_digest,
            execution_graph: execution_graph_digest,
            resource_graph: *resources.digest.as_bytes(),
            reactive_graph: reactive_graph_digest,
            lowering_contract: *lowering.digest.as_bytes(),
            view_binding_graph: *view.digest.as_bytes(),
            scope_storage_graph: *storage.digest.as_bytes(),
            memory_graph: *memory.digest.as_bytes(),
        }
    });

    let root_owner = SemanticDependencyOwnerV1::ProgramRoot;
    let root_direct = direct.remove(&root_owner).unwrap_or_default();
    let root_implementation_dependency_digest =
        graph_digests.get(&root_owner).copied().ok_or_else(|| {
            CallableDependencyManifestError::new(
                "program root has no rederived implementation dependency digest",
            )
        })?;
    let program_root = dependency_manifest_phase!(
        "program_root_digest",
        ProgramRootDependencyProofEntryV3 {
            public_shape_digest: canonical_dependency_hash(
                DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
                &(checked.role, checked.source_bundle_digest_v1),
            )?,
            implementation_dependency_digest: root_implementation_dependency_digest,
            direct_dependency_ids: root_direct,
        }
    );

    let callable_entries_started = trace_dependency_manifest.then(std::time::Instant::now);
    if trace_dependency_manifest {
        eprintln!("boon_semantic dependency_manifest callable_entries:start");
    }
    let callable_owners = owners
        .iter()
        .filter_map(|owner| match owner {
            SemanticDependencyOwnerV1::ProgramRoot => None,
            SemanticDependencyOwnerV1::Callable { callable } => Some(*callable),
        })
        .collect::<Vec<_>>();
    if execution.callables.len() != callable_owners.len() {
        return Err(CallableDependencyManifestError::new(format!(
            "dependency manifest has {} callable entries for {} callable owners",
            execution.callables.len(),
            callable_owners.len()
        )));
    }
    let mut callable_entries = Vec::with_capacity(execution.callables.len());
    for (ordinal, (callable, expected_callable)) in
        execution.callables.iter().zip(callable_owners).enumerate()
    {
        if trace_dependency_manifest && ordinal.is_multiple_of(64) {
            eprintln!(
                "boon_semantic dependency_manifest callable_entries:progress callable={ordinal}/{}",
                execution.callables.len()
            );
        }
        if callable.id != expected_callable {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency manifest callable {} is out of canonical order; expected {expected_callable}",
                callable.id
            )));
        }
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: callable.id,
        };
        let direct_dependency_ids = direct.remove(&owner).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "callable {} has no initialized dependency owner entry",
                callable.id
            ))
        })?;
        let implementation_dependency_digest =
            graph_digests.get(&owner).copied().ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "callable {} has no rederived implementation dependency digest",
                    callable.id
                ))
            })?;
        callable_entries.push(CallableDependencyProofEntryV3 {
            callable: callable.id,
            checked_callable: callable.checked_callable,
            public_shape_digest: callable_public_shape_digest(callable)?,
            implementation_dependency_digest,
            direct_dependency_ids,
        });
    }
    if let Some(started) = callable_entries_started {
        eprintln!(
            "boon_semantic dependency_manifest callable_entries:done elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }

    let schema = CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V3.to_owned();
    let source_bundle_digest_v1 = checked.source_bundle_digest_v1;
    let dependency_classifier_schema_digest =
        DependencyClassifierSchemaDigestV1(dependency_classifier_schema_digest);
    dependency_manifest_phase!("validate_shape", {
        if direct.is_empty() {
            Ok(())
        } else {
            Err(CallableDependencyManifestError::new(
                "dependency construction retained unconsumed owner entries",
            ))
        }
    })?;
    let callable_entries_digest = dependency_manifest_phase!(
        "callable_entries_digest",
        callable_entries_digest(&callable_entries)
    )?;
    let program_root_entry_digest = dependency_manifest_phase!(
        "program_root_entry_digest",
        canonical_dependency_hash_streaming(
            DEPENDENCY_PROGRAM_ROOT_ENTRY_DIGEST_DOMAIN,
            &program_root,
        )
    )?;
    let manifest_digest = dependency_manifest_phase!(
        "manifest_digest",
        callable_dependency_manifest_digest_from_content(
            &schema,
            source_bundle_digest_v1,
            checked_program_digest,
            dependency_classifier_schema_digest,
            &component_digests,
            &program_root,
            callable_entries_digest,
            dependency_records_digest,
            coverage_digest,
        )
    )?;
    let proof_digests = CallableDependencyProofDigestsV1 {
        program_root_entry_digest,
        callable_entries_digest,
        dependency_records_digest,
        coverage_digest,
        dependency_record_count,
        coverage_record_count,
    };
    Ok(ValidatedCallableDependencyConstructionV3 {
        schema,
        source_bundle_digest_v1,
        checked_program_digest,
        dependency_classifier_schema_digest,
        component_digests,
        program_root,
        callable_entries,
        proof_digests,
        manifest_digest,
        #[cfg(test)]
        retained_dependencies,
        #[cfg(test)]
        retained_coverage,
    })
}

#[cfg(test)]
impl ValidatedCallableDependencyConstructionV3 {
    fn into_exhaustive_proof(self) -> ValidatedCallableDependencyProofManifestV3 {
        let Self {
            schema,
            source_bundle_digest_v1,
            checked_program_digest,
            dependency_classifier_schema_digest,
            component_digests,
            program_root,
            callable_entries,
            proof_digests: _,
            manifest_digest,
            retained_dependencies,
            retained_coverage,
        } = self;
        ValidatedCallableDependencyProofManifestV3 {
            manifest: CallableDependencyProofManifestV3 {
                schema,
                source_bundle_digest_v1,
                checked_program_digest,
                dependency_classifier_schema_digest,
                component_digests,
                program_root,
                callable_entries,
                dependencies: retained_dependencies
                    .expect("exhaustive dependency records were explicitly retained"),
                coverage: retained_coverage
                    .expect("exhaustive dependency coverage was explicitly retained"),
                manifest_digest,
            },
        }
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn build_callable_dependency_proof_manifest(
    dependency_classifier_schema_digest: [u8; 32],
    checked: &CheckedProgram,
    producer_materializations: &[ProducerMaterializationRequest],
    out: &ResolvedOutGraph,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    lowering: &SemanticLoweringContractV1,
    view: &SemanticViewBindingGraphV1,
    storage: &SemanticScopeStorageGraphV1,
    memory: &SemanticMemoryGraphV1,
) -> Result<ValidatedCallableDependencyProofManifestV3, CallableDependencyManifestError> {
    let validated = build_callable_dependency_construction(
        dependency_classifier_schema_digest,
        checked,
        producer_materializations,
        out,
        execution,
        resources,
        reactive,
        lowering,
        view,
        storage,
        memory,
        ExhaustiveProofRetention::Retain,
    )?
    .into_exhaustive_proof();
    let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
    owners.extend(
        execution
            .callables
            .iter()
            .map(|callable| SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }),
    );
    validate_manifest_shape(&validated.manifest, &owners)?;
    Ok(validated)
}

#[cfg(test)]
fn seal_callable_dependency_manifest(
    validated: ValidatedCallableDependencyConstructionV3,
) -> Result<CallableDependencyManifestV3, CallableDependencyManifestError> {
    let ValidatedCallableDependencyConstructionV3 {
        schema,
        source_bundle_digest_v1,
        checked_program_digest,
        dependency_classifier_schema_digest,
        component_digests,
        program_root,
        callable_entries,
        proof_digests,
        manifest_digest,
        #[cfg(test)]
            retained_dependencies: _,
        #[cfg(test)]
            retained_coverage: _,
    } = validated;
    let program_root = ProgramRootDependencyEntryV3 {
        public_shape_digest: program_root.public_shape_digest,
        implementation_dependency_digest: program_root.implementation_dependency_digest,
    };
    let callable_entries = callable_entries
        .into_iter()
        .map(|entry| CallableDependencyEntryV3 {
            callable: entry.callable,
            checked_callable: entry.checked_callable,
            public_shape_digest: entry.public_shape_digest,
            implementation_dependency_digest: entry.implementation_dependency_digest,
        })
        .collect();
    let mut manifest = CallableDependencyManifestV3 {
        schema,
        source_bundle_digest_v1,
        checked_program_digest,
        dependency_classifier_schema_digest,
        component_digests,
        program_root,
        callable_entries,
        proof_digests,
        manifest_digest,
        sealed_manifest_digest: [0; 32],
    };
    manifest.sealed_manifest_digest = sealed_callable_dependency_manifest_digest(&manifest)?;
    Ok(manifest)
}

#[cfg(test)]
fn sealed_callable_dependency_manifest_digest(
    manifest: &CallableDependencyManifestV3,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    #[derive(Serialize)]
    struct Payload<'a> {
        schema: &'a str,
        source_bundle_digest_v1: SourceBundleDigestV1,
        checked_program_digest: CheckedProgramDigestV1,
        dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
        component_digests: &'a CallableDependencyComponentDigestsV1,
        program_root: &'a ProgramRootDependencyEntryV3,
        callable_entries: &'a [CallableDependencyEntryV3],
        proof_digests: &'a CallableDependencyProofDigestsV1,
        manifest_digest: CallableDependencyManifestDigestV1,
    }

    canonical_dependency_hash(
        DEPENDENCY_SEALED_MANIFEST_DIGEST_DOMAIN,
        &Payload {
            schema: &manifest.schema,
            source_bundle_digest_v1: manifest.source_bundle_digest_v1,
            checked_program_digest: manifest.checked_program_digest,
            dependency_classifier_schema_digest: manifest.dependency_classifier_schema_digest,
            component_digests: &manifest.component_digests,
            program_root: &manifest.program_root,
            callable_entries: &manifest.callable_entries,
            proof_digests: &manifest.proof_digests,
            manifest_digest: manifest.manifest_digest,
        },
    )
}

fn callable_dependency_manifest_digest_v4(
    schema: &str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    checked_program_digest: CheckedProgramDigestV1,
    dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    component_digests: &CallableDependencyComponentDigestsV1,
    program_root: &ProgramRootDependencyEntryV4,
    callable_entries: &[CallableDependencyEntryV4],
    proof_digests: &CallableDependencyProofDigestsV4,
) -> Result<CallableDependencyManifestDigestV1, CallableDependencyManifestError> {
    #[derive(Serialize)]
    struct Payload<'a> {
        schema: &'a str,
        source_bundle_digest_v1: SourceBundleDigestV1,
        checked_program_digest: CheckedProgramDigestV1,
        dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
        component_digests: &'a CallableDependencyComponentDigestsV1,
        program_root: &'a ProgramRootDependencyEntryV4,
        callable_entries: &'a [CallableDependencyEntryV4],
        proof_digests: &'a CallableDependencyProofDigestsV4,
    }

    canonical_dependency_hash_streaming(
        DEPENDENCY_MANIFEST_DIGEST_DOMAIN_V4,
        &Payload {
            schema,
            source_bundle_digest_v1,
            checked_program_digest,
            dependency_classifier_schema_digest,
            component_digests,
            program_root,
            callable_entries,
            proof_digests,
        },
    )
    .map(CallableDependencyManifestDigestV1)
}

fn sealed_callable_dependency_manifest_digest_v4(
    manifest: &CallableDependencyManifestV4,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    #[derive(Serialize)]
    struct Payload<'a> {
        schema: &'a str,
        source_bundle_digest_v1: SourceBundleDigestV1,
        checked_program_digest: CheckedProgramDigestV1,
        dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
        component_digests: &'a CallableDependencyComponentDigestsV1,
        program_root: &'a ProgramRootDependencyEntryV4,
        callable_entries: &'a [CallableDependencyEntryV4],
        proof_digests: &'a CallableDependencyProofDigestsV4,
        manifest_digest: CallableDependencyManifestDigestV1,
    }

    canonical_dependency_hash(
        DEPENDENCY_SEALED_MANIFEST_DOMAIN_V4,
        &Payload {
            schema: &manifest.schema,
            source_bundle_digest_v1: manifest.source_bundle_digest_v1,
            checked_program_digest: manifest.checked_program_digest,
            dependency_classifier_schema_digest: manifest.dependency_classifier_schema_digest,
            component_digests: &manifest.component_digests,
            program_root: &manifest.program_root,
            callable_entries: &manifest.callable_entries,
            proof_digests: &manifest.proof_digests,
            manifest_digest: manifest.manifest_digest,
        },
    )
}

#[cfg(test)]
impl CallableDependencyProofManifestV3 {
    #[cfg(test)]
    pub(crate) fn validate_integrity(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        if self.source_bundle_digest_v1 != checked.source_bundle_digest_v1 {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest source-bundle identity differs from its checked program",
            ));
        }
        if self.dependency_classifier_schema_digest
            != DependencyClassifierSchemaDigestV1(dependency_classifier_schema_digest)
        {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest classifier schema digest is stale",
            ));
        }
        let expected_checked_program_digest = CheckedProgramDigestV1(
            canonical_dependency_hash_streaming(CHECKED_PROGRAM_DIGEST_DOMAIN, checked)?,
        );
        if self.checked_program_digest != expected_checked_program_digest {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest checked-program digest is stale",
            ));
        }
        let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
        owners.extend(execution.callables.iter().map(|callable| {
            SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }
        }));
        let direct = validate_manifest_shape(self, &owners)?;
        for (entry, callable) in self.callable_entries.iter().zip(&execution.callables) {
            if entry.callable != callable.id || entry.checked_callable != callable.checked_callable
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "dependency manifest callable {} does not match semantic callable {} / checked declaration {}",
                    entry.callable, callable.id, callable.checked_callable.0
                )));
            }
        }
        let expected_manifest_digest = callable_dependency_manifest_digest(self)?;
        if self.manifest_digest != expected_manifest_digest {
            return Err(CallableDependencyManifestError::new(
                "callable dependency manifest digest does not match its canonical payload",
            ));
        }
        validate_implementation_dependency_digests(self, &owners, &direct)?;
        Ok(())
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_against(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        producer_materializations: &[ProducerMaterializationRequest],
        out: &ResolvedOutGraph,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        lowering: &SemanticLoweringContractV1,
        view: &SemanticViewBindingGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        memory: &SemanticMemoryGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        let expected = build_callable_dependency_proof_manifest(
            dependency_classifier_schema_digest,
            checked,
            producer_materializations,
            out,
            execution,
            resources,
            reactive,
            lowering,
            view,
            storage,
            memory,
        )?
        .manifest;
        if self != &expected {
            return Err(CallableDependencyManifestError::new(
                "callable dependency manifest differs from its deterministic checked+semantic rederivation",
            ));
        }
        Ok(())
    }
}

impl CallableDependencyManifestV4 {
    pub(crate) fn validate_integrity(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        if self.schema != CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V4 {
            return Err(CallableDependencyManifestError::new(format!(
                "unsupported callable dependency manifest schema `{}`",
                self.schema
            )));
        }
        if self.source_bundle_digest_v1 != checked.source_bundle_digest_v1 {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest source-bundle identity differs from its checked program",
            ));
        }
        if self.dependency_classifier_schema_digest
            != DependencyClassifierSchemaDigestV1(dependency_classifier_schema_digest)
        {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest classifier schema digest is stale",
            ));
        }
        let expected_checked_program_digest = CheckedProgramDigestV1(
            canonical_dependency_hash_streaming(CHECKED_PROGRAM_DIGEST_DOMAIN, checked)?,
        );
        if self.checked_program_digest != expected_checked_program_digest {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest checked-program digest is stale",
            ));
        }
        let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
        owners.extend(execution.callables.iter().map(|callable| {
            SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }
        }));
        let stable_owners = stable_dependency_owner_index_v4(checked, execution, &owners)?;
        let root_owner = SemanticDependencyOwnerV1::ProgramRoot;
        if self.program_root.stable_owner != stable_owners[&root_owner]
            || self.program_root.public_shape_digest
                != canonical_dependency_hash(
                    DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
                    &(checked.role, checked.source_bundle_digest_v1),
                )?
        {
            return Err(CallableDependencyManifestError::new(
                "sealed dependency manifest program-root identity or public shape is stale",
            ));
        }
        if self.callable_entries.len() != execution.callables.len() {
            return Err(CallableDependencyManifestError::new(format!(
                "sealed dependency manifest has {} callable identities for {} semantic callables",
                self.callable_entries.len(),
                execution.callables.len()
            )));
        }
        for (entry, callable) in self.callable_entries.iter().zip(&execution.callables) {
            let owner = SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            };
            if entry.callable != callable.id
                || entry.checked_callable != callable.checked_callable
                || entry.stable_owner != stable_owners[&owner]
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "sealed dependency manifest callable {} does not match semantic callable {} / checked declaration {}",
                    entry.callable, callable.id, callable.checked_callable.0
                )));
            }
            if entry.public_shape_digest != callable_public_shape_digest(callable)? {
                return Err(CallableDependencyManifestError::new(format!(
                    "sealed dependency manifest callable {} public shape is stale",
                    entry.callable
                )));
            }
        }
        if self.proof_digests.coverage_record_count < self.proof_digests.dependency_record_count
            || self.proof_digests.projection_count == 0
            || self.proof_digests.projection_count > self.proof_digests.coverage_record_count
        {
            return Err(CallableDependencyManifestError::new(
                "sealed dependency manifest has inconsistent projection receipt counts",
            ));
        }
        let expected_manifest_digest = callable_dependency_manifest_digest_v4(
            &self.schema,
            self.source_bundle_digest_v1,
            self.checked_program_digest,
            self.dependency_classifier_schema_digest,
            &self.component_digests,
            &self.program_root,
            &self.callable_entries,
            &self.proof_digests,
        )?;
        if self.manifest_digest != expected_manifest_digest {
            return Err(CallableDependencyManifestError::new(
                "callable dependency manifest digest does not match its projection receipt payload",
            ));
        }
        let expected_seal = sealed_callable_dependency_manifest_digest_v4(self)?;
        if self.sealed_manifest_digest != expected_seal {
            return Err(CallableDependencyManifestError::new(
                "sealed dependency manifest digest does not match its retained proof identity",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_against(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        producer_materializations: &[ProducerMaterializationRequest],
        out: &ResolvedOutGraph,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        lowering: &SemanticLoweringContractV1,
        view: &SemanticViewBindingGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        memory: &SemanticMemoryGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        let expected = build_callable_dependency_manifest(
            dependency_classifier_schema_digest,
            checked,
            producer_materializations,
            out,
            execution,
            resources,
            reactive,
            lowering,
            view,
            storage,
            memory,
        )?;
        if self != &expected {
            return Err(CallableDependencyManifestError::new(
                "callable dependency manifest differs from its deterministic checked+semantic rederivation",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
impl CallableDependencyManifestV3 {
    pub(crate) fn validate_integrity(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        execution: &SemanticExecutionGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        if self.schema != CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V3 {
            return Err(CallableDependencyManifestError::new(format!(
                "unsupported callable dependency manifest schema `{}`",
                self.schema
            )));
        }
        if self.source_bundle_digest_v1 != checked.source_bundle_digest_v1 {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest source-bundle identity differs from its checked program",
            ));
        }
        if self.dependency_classifier_schema_digest
            != DependencyClassifierSchemaDigestV1(dependency_classifier_schema_digest)
        {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest classifier schema digest is stale",
            ));
        }
        let expected_checked_program_digest = CheckedProgramDigestV1(
            canonical_dependency_hash_streaming(CHECKED_PROGRAM_DIGEST_DOMAIN, checked)?,
        );
        if self.checked_program_digest != expected_checked_program_digest {
            return Err(CallableDependencyManifestError::new(
                "dependency manifest checked-program digest is stale",
            ));
        }
        let expected_root_public_shape = canonical_dependency_hash(
            DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
            &(checked.role, checked.source_bundle_digest_v1),
        )?;
        if self.program_root.public_shape_digest != expected_root_public_shape {
            return Err(CallableDependencyManifestError::new(
                "sealed dependency manifest program-root public shape is stale",
            ));
        }
        if self.callable_entries.len() != execution.callables.len() {
            return Err(CallableDependencyManifestError::new(format!(
                "sealed dependency manifest has {} callable identities for {} semantic callables",
                self.callable_entries.len(),
                execution.callables.len()
            )));
        }
        for (entry, callable) in self.callable_entries.iter().zip(&execution.callables) {
            if entry.callable != callable.id || entry.checked_callable != callable.checked_callable
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "sealed dependency manifest callable {} does not match semantic callable {} / checked declaration {}",
                    entry.callable, callable.id, callable.checked_callable.0
                )));
            }
            if entry.public_shape_digest != callable_public_shape_digest(callable)? {
                return Err(CallableDependencyManifestError::new(format!(
                    "sealed dependency manifest callable {} public shape is stale",
                    entry.callable
                )));
            }
        }
        if self.manifest_digest == CallableDependencyManifestDigestV1([0; 32]) {
            return Err(CallableDependencyManifestError::new(
                "sealed dependency manifest has a zero exhaustive-proof digest",
            ));
        }
        let expected_seal = sealed_callable_dependency_manifest_digest(self)?;
        if self.sealed_manifest_digest != expected_seal {
            return Err(CallableDependencyManifestError::new(
                "sealed dependency manifest digest does not match its retained proof identity",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_against(
        &self,
        dependency_classifier_schema_digest: [u8; 32],
        checked: &CheckedProgram,
        producer_materializations: &[ProducerMaterializationRequest],
        out: &ResolvedOutGraph,
        execution: &SemanticExecutionGraphV1,
        resources: &SemanticResourceGraphV1,
        reactive: &SemanticReactiveGraphV1,
        lowering: &SemanticLoweringContractV1,
        view: &SemanticViewBindingGraphV1,
        storage: &SemanticScopeStorageGraphV1,
        memory: &SemanticMemoryGraphV1,
    ) -> Result<(), CallableDependencyManifestError> {
        let expected = seal_callable_dependency_manifest(build_callable_dependency_construction(
            dependency_classifier_schema_digest,
            checked,
            producer_materializations,
            out,
            execution,
            resources,
            reactive,
            lowering,
            view,
            storage,
            memory,
            ExhaustiveProofRetention::Retain,
        )?)?;
        if self != &expected {
            return Err(CallableDependencyManifestError::new(
                "callable dependency manifest differs from its deterministic checked+semantic rederivation",
            ));
        }
        Ok(())
    }
}

fn callable_public_shape_digest(
    callable: &SemanticCallable,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    #[derive(Serialize)]
    struct PublicParameter<'a> {
        ordinal: usize,
        name: &'a str,
        kind: boon_checked::CheckedParameterKind,
        flow_type: &'a FlowType,
        requirement: &'a boon_checked::CheckedParameterRequirement,
        evaluation_scope: boon_checked::CheckedEvaluationScope,
    }
    #[derive(Serialize)]
    struct PublicCallable<'a> {
        kind: boon_checked::CheckedCallableKind,
        name: &'a str,
        external_identity: &'a Option<boon_checked::CheckedExternalDeclarationIdentityV1>,
        parameters: Vec<PublicParameter<'a>>,
        contexts: &'a [SemanticCallableContext],
        context_scheme: Option<&'a boon_checked::CheckedContextScheme>,
        result: &'a FlowType,
        role: ProgramRole,
        effect: boon_checked::CheckedEffectSummary,
        contextual_operation: &'a Option<boon_checked::CheckedContextualOperation>,
    }

    let parameters = callable
        .parameters
        .iter()
        .map(|parameter| PublicParameter {
            ordinal: parameter.ordinal,
            name: &parameter.name,
            kind: parameter.kind,
            flow_type: &parameter.flow_type,
            requirement: &parameter.requirement,
            evaluation_scope: parameter.evaluation_scope,
        })
        .collect();
    // The semantic callable retains the stable formal identity. The complete
    // principal scheme lives in the checked formal table and is included in
    // the checked-program digest; wiring supplies it to future theorem DTOs.
    canonical_dependency_hash(
        DEPENDENCY_PUBLIC_SHAPE_DOMAIN,
        &PublicCallable {
            kind: callable.kind,
            name: &callable.name,
            external_identity: &callable.external_identity,
            parameters,
            contexts: &callable.contexts,
            context_scheme: None,
            result: &callable.result,
            role: callable.role,
            effect: callable.effect,
            contextual_operation: &callable.contextual_operation,
        },
    )
}

#[cfg(test)]
fn validate_implementation_dependency_digests(
    manifest: &CallableDependencyProofManifestV3,
    owners: &BTreeSet<SemanticDependencyOwnerV1>,
    direct: &BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
) -> Result<(), CallableDependencyManifestError> {
    let owner_list = owners.iter().copied().collect::<Vec<_>>();
    let (graph_digests, _, _) =
        build_dependency_graph_digests(&owner_list, &manifest.dependencies, direct)?;

    let root_owner = SemanticDependencyOwnerV1::ProgramRoot;
    let expected_root_digest = graph_digests.get(&root_owner).copied().ok_or_else(|| {
        CallableDependencyManifestError::new(
            "program root has no rederived implementation dependency digest",
        )
    })?;
    if manifest.program_root.implementation_dependency_digest != expected_root_digest {
        return Err(CallableDependencyManifestError::new(
            "program-root implementation dependency digest differs from its exact dependency graph proof",
        ));
    }

    for entry in &manifest.callable_entries {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: entry.callable,
        };
        let expected_digest = graph_digests.get(&owner).copied().ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "callable {} has no rederived implementation dependency digest",
                entry.callable
            ))
        })?;
        if entry.implementation_dependency_digest != expected_digest {
            return Err(CallableDependencyManifestError::new(format!(
                "callable {} implementation dependency digest differs from its exact dependency graph proof",
                entry.callable
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_manifest_shape(
    manifest: &CallableDependencyProofManifestV3,
    owners: &BTreeSet<SemanticDependencyOwnerV1>,
) -> Result<
    BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
    CallableDependencyManifestError,
> {
    if manifest.schema != CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V3 {
        return Err(CallableDependencyManifestError::new(format!(
            "unsupported callable dependency manifest schema `{}`",
            manifest.schema
        )));
    }
    let mut direct = owners
        .iter()
        .copied()
        .map(|owner| (owner, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for (index, dependency) in manifest.dependencies.iter().enumerate() {
        if dependency.id != SemanticDependencyRecordId(index) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency record {} is not dense index {index}",
                dependency.id
            )));
        }
        if !owners.contains(&dependency.owner) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency {} references missing owner {:?}",
                dependency.id, dependency.owner
            )));
        }
        direct
            .get_mut(&dependency.owner)
            .expect("validated dependency owner has a dense direct entry")
            .push(dependency.id);
        if dependency.roles.is_empty() || !dependency.roles.windows(2).all(|pair| pair[0] < pair[1])
        {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency {} roles are empty, duplicated, or non-canonical",
                dependency.id
            )));
        }
        if !dependency
            .referenced_dependencies
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency {} references dependencies out of canonical order",
                dependency.id
            )));
        }
        for referenced in &dependency.referenced_dependencies {
            if manifest
                .dependencies
                .get(referenced.as_usize())
                .is_none_or(|candidate| candidate.id != *referenced)
            {
                return Err(CallableDependencyManifestError::new(format!(
                    "dependency {} references missing dependency {referenced}",
                    dependency.id
                )));
            }
        }
        if !dependency
            .referenced_owners
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency {} references owners out of canonical order",
                dependency.id
            )));
        }
        for referenced_owner in &dependency.referenced_owners {
            if *referenced_owner == dependency.owner || !owners.contains(referenced_owner) {
                return Err(CallableDependencyManifestError::new(format!(
                    "dependency {} references invalid owner {referenced_owner:?}",
                    dependency.id
                )));
            }
        }
    }

    let root_direct = direct
        .get(&SemanticDependencyOwnerV1::ProgramRoot)
        .expect("program root is a mandatory dependency owner");
    if manifest.program_root.direct_dependency_ids != *root_direct {
        return Err(CallableDependencyManifestError::new(
            "program-root direct dependency IDs differ from the owned dependency records",
        ));
    }
    let callable_owners = owners
        .iter()
        .filter_map(|owner| match owner {
            SemanticDependencyOwnerV1::ProgramRoot => None,
            SemanticDependencyOwnerV1::Callable { callable } => Some(*callable),
        })
        .collect::<Vec<_>>();
    if manifest.callable_entries.len() != callable_owners.len() {
        return Err(CallableDependencyManifestError::new(format!(
            "dependency manifest has {} callable entries for {} callable owners",
            manifest.callable_entries.len(),
            callable_owners.len()
        )));
    }
    for (entry, expected_callable) in manifest.callable_entries.iter().zip(callable_owners) {
        if entry.callable != expected_callable {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency manifest callable {} is out of canonical order; expected {expected_callable}",
                entry.callable
            )));
        }
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: entry.callable,
        };
        let expected_direct = direct
            .get(&owner)
            .expect("validated callable owner has a dense direct entry");
        if entry.direct_dependency_ids != *expected_direct {
            return Err(CallableDependencyManifestError::new(format!(
                "callable {} direct dependency IDs differ from its owned dependency records",
                entry.callable
            )));
        }
    }
    let mut subjects = BTreeSet::new();
    for (index, coverage) in manifest.coverage.iter().enumerate() {
        if coverage.id != SemanticDependencyCoverageId(index) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency coverage {} is not dense index {index}",
                coverage.id
            )));
        }
        if !subjects.insert(&coverage.subject) {
            return Err(CallableDependencyManifestError::new(format!(
                "dependency subject {:?} has duplicate coverage",
                coverage.subject
            )));
        }
        if !owners.contains(&coverage.primary_owner) {
            return Err(CallableDependencyManifestError::new(format!(
                "coverage {} references missing owner {:?}",
                coverage.id, coverage.primary_owner
            )));
        }
        if let SemanticDependencyCoverageDispositionV1::Dependency { dependency } =
            coverage.disposition
        {
            let record = manifest
                .dependencies
                .get(dependency.as_usize())
                .filter(|record| record.id == dependency)
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "coverage {} references missing dependency {dependency}",
                        coverage.id
                    ))
                })?;
            if record.subject != coverage.subject || record.owner != coverage.primary_owner {
                return Err(CallableDependencyManifestError::new(format!(
                    "coverage {} disagrees with dependency {dependency}",
                    coverage.id
                )));
            }
        }
    }
    Ok(direct)
}

#[cfg(test)]
#[derive(Serialize)]
struct CallableDependencyManifestDigestPayload<'a> {
    schema: &'a str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    checked_program_digest: CheckedProgramDigestV1,
    dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    component_digests: &'a CallableDependencyComponentDigestsV1,
    program_root: &'a ProgramRootDependencyProofEntryV3,
    callable_entries_digest: [u8; 32],
    dependency_records_digest: [u8; 32],
    coverage_digest: [u8; 32],
}

#[cfg(test)]
fn callable_entries_digest(
    entries: &[CallableDependencyProofEntryV3],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    canonical_dependency_hash_streaming(DEPENDENCY_CALLABLE_SET_DIGEST_DOMAIN, entries)
}

#[cfg(test)]
fn dependency_coverage_digest(
    coverage: &[SemanticDependencyCoverageV1],
) -> Result<[u8; 32], CallableDependencyManifestError> {
    canonical_dependency_hash_streaming(DEPENDENCY_COVERAGE_SET_DIGEST_DOMAIN, coverage)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn callable_dependency_manifest_digest_from_content(
    schema: &str,
    source_bundle_digest_v1: SourceBundleDigestV1,
    checked_program_digest: CheckedProgramDigestV1,
    dependency_classifier_schema_digest: DependencyClassifierSchemaDigestV1,
    component_digests: &CallableDependencyComponentDigestsV1,
    program_root: &ProgramRootDependencyProofEntryV3,
    callable_entries_digest: [u8; 32],
    dependency_records_digest: [u8; 32],
    coverage_digest: [u8; 32],
) -> Result<CallableDependencyManifestDigestV1, CallableDependencyManifestError> {
    canonical_dependency_hash_streaming(
        DEPENDENCY_MANIFEST_DIGEST_DOMAIN,
        &CallableDependencyManifestDigestPayload {
            schema,
            source_bundle_digest_v1,
            checked_program_digest,
            dependency_classifier_schema_digest,
            component_digests,
            program_root,
            callable_entries_digest,
            dependency_records_digest,
            coverage_digest,
        },
    )
    .map(CallableDependencyManifestDigestV1)
}

#[cfg(test)]
fn callable_dependency_manifest_digest(
    manifest: &CallableDependencyProofManifestV3,
) -> Result<CallableDependencyManifestDigestV1, CallableDependencyManifestError> {
    callable_dependency_manifest_digest_from_content(
        &manifest.schema,
        manifest.source_bundle_digest_v1,
        manifest.checked_program_digest,
        manifest.dependency_classifier_schema_digest,
        &manifest.component_digests,
        &manifest.program_root,
        callable_entries_digest(&manifest.callable_entries)?,
        stream_dependency_record_digests(&manifest.dependencies, |_, _| Ok(()))?,
        dependency_coverage_digest(&manifest.coverage)?,
    )
}

fn inventory_checked(
    checked: &CheckedProgram,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<CheckedProgramDigestV1, CallableDependencyManifestError> {
    let checked_program_digest = collector.checked_program_structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::CheckedProgram,
            SemanticDependencyEntityV1::Program,
        ),
        checked,
    )?;

    for scope in &checked.scopes {
        let owner = owners.checked_scope(scope.id)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedScope,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedScope,
                    scope.id.0,
                ),
            ),
            SemanticDependencySemanticsV1::default(),
            scope,
            Vec::new(),
        )?;
    }

    for declaration in &checked.declarations {
        let owner = if declaration.kind == CheckedDeclarationKind::Function {
            let callable = owners
                .callable_by_checked
                .get(&declaration.id)
                .copied()
                .ok_or_else(|| {
                    CallableDependencyManifestError::new(format!(
                        "function declaration {} has no semantic callable identity",
                        declaration.id.0
                    ))
                })?;
            SemanticDependencyOwnerV1::Callable { callable }
        } else {
            owners.checked_scope(declaration.scope_id)?
        };
        collect_dependency!(
            collector,
            owner,
            declaration_channel(declaration.kind),
            declaration_roles(declaration.kind),
            top_subject(
                SemanticDependencySubjectKindV1::CheckedDeclaration,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    declaration.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(declaration.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            declaration,
            declaration
                .value
                .map(|expression| {
                    vec![PendingDependencyReference::Entity(
                        SemanticDependencyEntityV1::checked(
                            SemanticDependencyEntityDomainV1::CheckedExpression,
                            expression.0,
                        ),
                    )]
                })
                .unwrap_or_default(),
        )?;
    }

    for statement in &checked.statements {
        let owner = owners.checked_scope(statement.scope_id)?;
        let entity = SemanticDependencyEntityV1::checked(
            SemanticDependencyEntityDomainV1::CheckedStatement,
            statement.id.0,
        );
        collect_dependency!(
            collector,
            owner,
            checked_statement_channel(&statement.kind),
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedStatement,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1::default(),
            statement,
            statement
                .value
                .map(|expression| {
                    vec![PendingDependencyReference::Entity(
                        SemanticDependencyEntityV1::checked(
                            SemanticDependencyEntityDomainV1::CheckedExpression,
                            expression.0,
                        ),
                    )]
                })
                .unwrap_or_default(),
        )?;
        for (ordinal, resource) in statement.resources.iter().enumerate() {
            collector.structural(
                owner,
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedResourceProjection,
                    entity.clone(),
                    ordinal,
                ),
                resource,
            )?;
        }
    }

    for expression in &checked.expressions {
        let owner = owners.checked_scope(expression.scope_id)?;
        let entity = SemanticDependencyEntityV1::checked(
            SemanticDependencyEntityDomainV1::CheckedExpression,
            expression.id.0,
        );
        let (channel, roles, projection, references) =
            checked_expression_dependency(&expression.kind)?;
        collect_dependency!(
            collector,
            owner,
            channel,
            roles,
            top_subject(
                SemanticDependencySubjectKindV1::CheckedExpression,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                projection,
                flow_type: Some(expression.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            expression,
            references,
        )?;
        match &expression.kind {
            CheckedExpressionKind::TextTemplate { segments } => {
                for (ordinal, segment) in segments.iter().enumerate() {
                    collector.structural(
                        owner,
                        child_subject(
                            SemanticDependencySubjectKindV1::CheckedLoweringSideTableEntry,
                            entity.clone(),
                            ordinal,
                        ),
                        segment,
                    )?;
                }
            }
            CheckedExpressionKind::TaggedObject { fields, .. }
            | CheckedExpressionKind::Object { fields } => {
                for (ordinal, field) in fields.iter().enumerate() {
                    collector.structural(
                        owner,
                        child_subject(
                            SemanticDependencySubjectKindV1::CheckedLoweringSideTableEntry,
                            entity.clone(),
                            ordinal,
                        ),
                        field,
                    )?;
                }
            }
            CheckedExpressionKind::MatchArm { pattern, .. } => {
                collector.structural(
                    owner,
                    child_subject(
                        SemanticDependencySubjectKindV1::CheckedPatternBinding,
                        entity,
                        0,
                    ),
                    pattern,
                )?;
            }
            _ => {}
        }
    }

    let semantic_callable_by_checked = execution
        .callables
        .iter()
        .map(|callable| (callable.checked_callable, callable))
        .collect::<BTreeMap<_, _>>();
    for callable in &checked.callables {
        let semantic = semantic_callable_by_checked
            .get(&callable.decl_id)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "checked callable {} has no exact semantic callable",
                    callable.decl_id.0
                ))
            })?;
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: semantic.id,
        };
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::SemanticCallable,
            semantic.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedCallable,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedCallable,
                    callable.decl_id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(callable.result.clone()),
                program_role: Some(callable.role),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            callable,
            Vec::new(),
        )?;
        for (ordinal, parameter) in callable.parameters.iter().enumerate() {
            let (channel, roles) = parameter_dependency(parameter);
            collect_dependency!(
                collector,
                owner,
                channel,
                roles,
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallableParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    evaluation_scope: Some(parameter.evaluation_scope),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    multiplicity: match parameter.evaluation_scope {
                        CheckedEvaluationScope::Parent => SemanticDependencyMultiplicityV1::Once,
                        CheckedEvaluationScope::Output { .. } => {
                            SemanticDependencyMultiplicityV1::PerRowMaterialization
                        }
                    },
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                Vec::new(),
            )?;
        }
        for (ordinal, context) in callable.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallableContext,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(context.flow_type.clone()),
                    multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                Vec::new(),
            )?;
        }
    }

    for formal in &checked.context_formals {
        let callable = owners
            .callable_by_checked
            .get(&formal.callable)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "context formal {} references missing semantic callable for declaration {}",
                    formal.id.0, formal.callable.0
                ))
            })?;
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::Callable { callable },
            SemanticDependencyChannelV1::PassedContextFormal,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedContextFormal,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedContextFormal,
                    formal.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(formal.scheme.flow_type.clone()),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            formal,
            Vec::new(),
        )?;
    }

    let semantic_call_by_checked = execution
        .calls
        .iter()
        .map(|call| (call.checked_call, call))
        .collect::<BTreeMap<_, _>>();
    for call in &checked.calls {
        let semantic = semantic_call_by_checked.get(&call.id).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "checked call {} has no exact semantic call",
                call.id.0
            ))
        })?;
        let owner = owners.call(semantic.id)?;
        let call_entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::SemanticCall,
            semantic.id.as_usize(),
        );
        let callee = execution
            .callables
            .get(semantic.callable.as_usize())
            .filter(|candidate| candidate.id == semantic.callable)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "semantic call {} references missing callable {}",
                    semantic.id, semantic.callable
                ))
            })?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedCall,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedCall,
                    call.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(call.result.clone()),
                program_role: Some(call.role),
                lifetime: SemanticDependencyLifetimeV1::Call,
                ..SemanticDependencySemanticsV1::default()
            },
            call,
            vec![
                PendingDependencyReference::Owner(SemanticDependencyOwnerV1::Callable {
                    callable: callee.id,
                }),
                PendingDependencyReference::Entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    call.expression.0,
                )),
            ],
        )?;
        for (ordinal, entry) in call.entries.iter().enumerate() {
            let (channel, flow, references) = checked_call_entry_dependency(entry);
            collect_dependency!(
                collector,
                owner,
                channel,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallEntry,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: flow,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                entry,
                references,
            )?;
        }
        for (ordinal, context) in call.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedCallContext,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                Vec::new(),
            )?;
        }
        for (ordinal, substitution) in call.contextual_substitutions.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedContextSubstitution,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                substitution,
                Vec::new(),
            )?;
        }
        for (ordinal, substitution) in call.type_substitutions.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedTypeSubstitution,
                    call_entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                substitution,
                Vec::new(),
            )?;
        }
    }

    for (ordinal, path) in checked.call_result_paths.iter().enumerate() {
        let call = semantic_call_by_checked.get(&path.call).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "checked result path references call {} without semantic identity",
                path.call.0
            ))
        })?;
        collect_dependency!(
            collector,
            owners.call(call.id)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedCallResultPath,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedCallResultPath,
                    ordinal,
                ),
            ),
            SemanticDependencySemanticsV1::default(),
            path,
            Vec::new(),
        )?;
    }

    for (ordinal, chain) in checked.order_chains.iter().enumerate() {
        let call = semantic_call_by_checked.get(&chain.call).ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "checked order chain references call {} without semantic identity",
                chain.call.0
            ))
        })?;
        let owner = owners.call(call.id)?;
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::CheckedOrderChain,
            ordinal,
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedOrderChain,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1::default(),
            chain,
            Vec::new(),
        )?;
        for (key_ordinal, key) in chain.chain.keys.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CoverageRouting,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                child_subject(
                    SemanticDependencySubjectKindV1::CheckedOrderKey,
                    entity.clone(),
                    key_ordinal,
                ),
                SemanticDependencySemanticsV1::default(),
                key,
                vec![PendingDependencyReference::Entity(
                    SemanticDependencyEntityV1::checked(
                        SemanticDependencyEntityDomainV1::CheckedExpression,
                        key.key.0,
                    ),
                )],
            )?;
        }
    }

    for (ordinal, binding) in checked.pattern_bindings.iter().enumerate() {
        collect_dependency!(
            collector,
            owners.checked_scope(
                checked
                    .expressions
                    .get(binding.selector.0 as usize)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "pattern binding {} references missing selector {}",
                            binding.declaration.0, binding.selector.0
                        ))
                    })?
                    .scope_id,
            )?,
            SemanticDependencyChannelV1::LocalFact,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedPatternBinding,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedPatternBinding,
                    ordinal,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: binding.projection.clone(),
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    binding.selector.0,
                ),
            )],
        )?;
    }

    for (ordinal, requirement) in checked.resource_projection_requirements.iter().enumerate() {
        let expression = checked
            .expressions
            .get(requirement.expression.0 as usize)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "resource projection requirement references missing expression {}",
                    requirement.expression.0
                ))
            })?;
        collect_dependency!(
            collector,
            owners.checked_scope(expression.scope_id)?,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedResourceProjection,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedResourceProjection,
                    ordinal,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: requirement.projection.clone(),
                ..SemanticDependencySemanticsV1::default()
            },
            requirement,
            vec![
                PendingDependencyReference::Entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    requirement.expression.0,
                )),
                PendingDependencyReference::Entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    requirement.target.0,
                )),
            ],
        )?;
    }

    for source in &checked.sources {
        collect_dependency!(
            collector,
            owners.checked_scope(source.owner_scope)?,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedSource,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedSource,
                    source.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            Vec::new(),
        )?;
    }
    for state in &checked.states {
        collect_dependency!(
            collector,
            owners.checked_scope(state.owner_scope)?,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedState,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedState,
                    state.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(state.flow_type.clone()),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::CurrentValue,
                ..SemanticDependencySemanticsV1::default()
            },
            state,
            Vec::new(),
        )?;
    }
    for list in &checked.lists {
        collect_dependency!(
            collector,
            owners.checked_scope(list.owner_scope)?,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::CheckedList,
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedList,
                    list.id.0,
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            list,
            Vec::new(),
        )?;
    }
    for (ordinal, occurrence) in checked.occurrences.iter().enumerate() {
        let declaration = checked
            .declarations
            .iter()
            .find(|declaration| declaration.id == occurrence.target)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "checked occurrence references missing declaration {}",
                    occurrence.target.0
                ))
            })?;
        collector.diagnostic(
            if declaration.kind == CheckedDeclarationKind::Function {
                SemanticDependencyOwnerV1::Callable {
                    callable: owners.callable_by_checked[&declaration.id],
                }
            } else {
                owners.checked_scope(declaration.scope_id)?
            },
            top_subject(
                SemanticDependencySubjectKindV1::CheckedOccurrence,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::CheckedOccurrence,
                    ordinal,
                ),
            ),
            occurrence,
        )?;
    }

    inventory_checked_lowering_metadata(checked, owners, collector)?;
    Ok(checked_program_digest)
}

fn declaration_channel(kind: CheckedDeclarationKind) -> SemanticDependencyChannelV1 {
    match kind {
        CheckedDeclarationKind::ValueParameter => SemanticDependencyChannelV1::ParentValueFormal,
        CheckedDeclarationKind::OutParameter | CheckedDeclarationKind::FreshOut => {
            SemanticDependencyChannelV1::OutFormal
        }
        CheckedDeclarationKind::PatternBinding | CheckedDeclarationKind::Field => {
            SemanticDependencyChannelV1::LocalFact
        }
        CheckedDeclarationKind::Source
        | CheckedDeclarationKind::Hold
        | CheckedDeclarationKind::List => SemanticDependencyChannelV1::ResourceBehavior,
        CheckedDeclarationKind::ElementState => SemanticDependencyChannelV1::CompilerContext,
        CheckedDeclarationKind::Function | CheckedDeclarationKind::Builtin => {
            SemanticDependencyChannelV1::CalledCallable
        }
        CheckedDeclarationKind::External => SemanticDependencyChannelV1::ExternalValueOrCall,
    }
}

fn declaration_roles(kind: CheckedDeclarationKind) -> Vec<SemanticDependencyRoleV1> {
    match kind {
        CheckedDeclarationKind::ValueParameter | CheckedDeclarationKind::OutParameter => vec![
            SemanticDependencyRoleV1::FormulaBinder,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
        CheckedDeclarationKind::Source
        | CheckedDeclarationKind::Hold
        | CheckedDeclarationKind::List
        | CheckedDeclarationKind::ElementState
        | CheckedDeclarationKind::External => vec![
            SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
        CheckedDeclarationKind::Function
        | CheckedDeclarationKind::Builtin
        | CheckedDeclarationKind::FreshOut
        | CheckedDeclarationKind::PatternBinding
        | CheckedDeclarationKind::Field => vec![
            SemanticDependencyRoleV1::FixedDefinition,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
    }
}

fn checked_statement_channel(kind: &CheckedStatementKind) -> SemanticDependencyChannelV1 {
    match kind {
        CheckedStatementKind::Source { .. }
        | CheckedStatementKind::Hold { .. }
        | CheckedStatementKind::List { .. } => SemanticDependencyChannelV1::ResourceBehavior,
        CheckedStatementKind::Function { .. }
        | CheckedStatementKind::Field { .. }
        | CheckedStatementKind::Block
        | CheckedStatementKind::Spread
        | CheckedStatementKind::Expression => SemanticDependencyChannelV1::LocalFact,
    }
}

fn checked_expression_dependency(
    kind: &CheckedExpressionKind,
) -> Result<ExpressionDependency, CallableDependencyManifestError> {
    let fixed = vec![SemanticDependencyRoleV1::FixedDefinition];
    Ok(match kind {
        CheckedExpressionKind::Read {
            target, projection, ..
        } => (
            SemanticDependencyChannelV1::LexicalCapture,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            projection.clone(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    target.0,
                ),
            )],
        ),
        CheckedExpressionKind::Passed {
            formal, projection, ..
        } => (
            SemanticDependencyChannelV1::PassedContextFormal,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            projection.clone(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedContextFormal,
                    formal.0,
                ),
            )],
        ),
        CheckedExpressionKind::ExternalRead { .. } => (
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            Vec::new(),
            Vec::new(),
        ),
        CheckedExpressionKind::Drain { target, projection } => (
            SemanticDependencyChannelV1::MigrationPredecessor,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            projection.clone(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    target.0,
                ),
            )],
        ),
        CheckedExpressionKind::Source
        | CheckedExpressionKind::Hold { .. }
        | CheckedExpressionKind::Latest { .. } => (
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            Vec::new(),
            Vec::new(),
        ),
        CheckedExpressionKind::Draining { .. } => (
            SemanticDependencyChannelV1::MigrationPredecessor,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            Vec::new(),
            Vec::new(),
        ),
        CheckedExpressionKind::Call { call } => (
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            Vec::new(),
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedCall,
                    call.0,
                ),
            )],
        ),
        CheckedExpressionKind::Invalid { .. } => {
            return Err(CallableDependencyManifestError::new(
                "invalid checked expression cannot enter a verified dependency manifest",
            ));
        }
        CheckedExpressionKind::Text { .. }
        | CheckedExpressionKind::TextTemplate { .. }
        | CheckedExpressionKind::Number { .. }
        | CheckedExpressionKind::Bits { .. }
        | CheckedExpressionKind::BytesByte { .. }
        | CheckedExpressionKind::Absent
        | CheckedExpressionKind::Flush { .. }
        | CheckedExpressionKind::Tag { .. }
        | CheckedExpressionKind::TaggedObject { .. }
        | CheckedExpressionKind::When { .. }
        | CheckedExpressionKind::While { .. }
        | CheckedExpressionKind::Then { .. }
        | CheckedExpressionKind::Infix { .. }
        | CheckedExpressionKind::MatchArm { .. }
        | CheckedExpressionKind::Block { .. }
        | CheckedExpressionKind::Object { .. }
        | CheckedExpressionKind::List { .. }
        | CheckedExpressionKind::MapEntry { .. }
        | CheckedExpressionKind::Map { .. }
        | CheckedExpressionKind::Set { .. }
        | CheckedExpressionKind::Bytes { .. }
        | CheckedExpressionKind::Delimiter => (
            SemanticDependencyChannelV1::StructuralRepresentation,
            fixed,
            Vec::new(),
            Vec::new(),
        ),
    })
}

fn parameter_dependency(
    parameter: &boon_checked::CheckedParameter,
) -> (SemanticDependencyChannelV1, Vec<SemanticDependencyRoleV1>) {
    let channel = match (parameter.kind, parameter.evaluation_scope) {
        (CheckedParameterKind::Out, _) => SemanticDependencyChannelV1::OutFormal,
        (CheckedParameterKind::Value, CheckedEvaluationScope::Output { .. }) => {
            SemanticDependencyChannelV1::OutputEvaluatedFormal
        }
        (CheckedParameterKind::Value, CheckedEvaluationScope::Parent) => {
            if parameter.requirement.is_optional() {
                SemanticDependencyChannelV1::NormalizedDefault
            } else {
                SemanticDependencyChannelV1::ParentValueFormal
            }
        }
    };
    (
        channel,
        vec![
            SemanticDependencyRoleV1::FormulaBinder,
            SemanticDependencyRoleV1::CoverageOrRouting,
        ],
    )
}

fn checked_call_entry_dependency(
    entry: &CheckedCallEntry,
) -> (
    SemanticDependencyChannelV1,
    Option<FlowType>,
    Vec<PendingDependencyReference>,
) {
    match entry {
        CheckedCallEntry::Input {
            value,
            evaluation_scope,
            ..
        } => (
            match evaluation_scope {
                CheckedEvaluationScope::Parent => SemanticDependencyChannelV1::ParentValueFormal,
                CheckedEvaluationScope::Output { .. } => {
                    SemanticDependencyChannelV1::OutputEvaluatedFormal
                }
            },
            None,
            vec![PendingDependencyReference::Entity(
                SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedExpression,
                    value.0,
                ),
            )],
        ),
        CheckedCallEntry::FreshOut { .. } | CheckedCallEntry::ForwardOut { .. } => {
            (SemanticDependencyChannelV1::OutFormal, None, Vec::new())
        }
    }
}

fn inventory_checked_lowering_metadata(
    checked: &CheckedProgram,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    let metadata_entity =
        SemanticDependencyEntityV1::indexed(SemanticDependencyEntityDomainV1::Diagnostic, 0);
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::CheckedLoweringMetadata,
            metadata_entity.clone(),
        ),
        &checked.lowering_metadata,
    )?;
    for (ordinal, statement) in checked
        .lowering_metadata
        .named_value_type_table
        .checked_statement_sites
        .iter()
        .copied()
        .enumerate()
    {
        collect_dependency!(
            collector,
            owners.checked_statement(statement)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::CheckedNamedValueStatementSite,
                metadata_entity.clone(),
                ordinal,
            ),
            SemanticDependencySemanticsV1::default(),
            &statement,
            vec![dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedStatement,
                statement.0,
            ))],
        )?;
    }
    let mut ordinal = 0usize;
    macro_rules! cover_entries {
        ($entries:expr) => {
            for entry in $entries {
                collector.structural(
                    SemanticDependencyOwnerV1::ProgramRoot,
                    child_subject(
                        SemanticDependencySubjectKindV1::CheckedLoweringSideTableEntry,
                        metadata_entity.clone(),
                        ordinal,
                    ),
                    entry,
                )?;
                ordinal += 1;
            }
        };
    }
    cover_entries!(&checked.lowering_metadata.source_units);
    cover_entries!(&checked.lowering_metadata.source_payload_shape_table);
    cover_entries!(&checked.lowering_metadata.output_root_types);
    cover_entries!(&checked.lowering_metadata.expr_type_table.entries);
    cover_entries!(&checked.lowering_metadata.function_type_table.entries);
    cover_entries!(&checked.lowering_metadata.named_value_type_table.entries);
    cover_entries!(&checked.lowering_metadata.render_slot_table.slots);
    cover_entries!(&checked.lowering_metadata.diagnostics);
    if let Some(http) = &checked.lowering_metadata.host_port_table.http {
        cover_entries!(std::slice::from_ref(http));
    }
    if let Some(websocket) = &checked.lowering_metadata.host_port_table.websocket {
        cover_entries!(std::slice::from_ref(websocket));
    }
    let _ = owners;
    Ok(())
}

fn inventory_producer_requests(
    requests: &[ProducerMaterializationRequest],
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ProducerMaterializationTable,
            SemanticDependencyEntityV1::Program,
        ),
        &requests,
    )?;
    for request in requests {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: request.callable,
        };
        if !owners
            .callable_by_checked
            .values()
            .any(|callable| *callable == request.callable)
        {
            return Err(CallableDependencyManifestError::new(format!(
                "producer materialization {:?} references missing callable {}",
                request.identity, request.callable
            )));
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ProducerMaterializationRequest,
                SemanticDependencyEntityV1::Digest {
                    domain: SemanticDependencyEntityDomainV1::ProducerMaterialization,
                    digest: request.identity,
                },
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: match request.mode {
                    ProducerMaterializationMode::Current => {
                        SemanticDependencyMultiplicityV1::PerTick
                    }
                    ProducerMaterializationMode::Invocation => {
                        SemanticDependencyMultiplicityV1::PerEvent
                    }
                },
                ..SemanticDependencySemanticsV1::default()
            },
            request,
            Vec::new(),
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct CanonicalOutTypeSubstitutionV1 {
    variable: TypeVar,
    value_digest: [u8; 32],
}

/// Canonical semantic identity for one concrete OUT call frame.
///
/// Recursive structural types are committed by digest and interned across the
/// complete OUT component. Serializing the expanded type tree once per
/// contextual frame made equivalent inherited environments quadratic in the
/// number of frames without adding dependency evidence.
#[derive(Serialize)]
struct CanonicalOutCallHeaderV1 {
    id: OutCallInstanceId,
    parent: Option<OutCallInstanceId>,
    provenance: OutCallProvenance,
    parent_output: Option<DeclId>,
    ports: Vec<OutPortId>,
    local_type_substitutions: Vec<CanonicalOutTypeSubstitutionV1>,
    result_mode: FlowMode,
    result_type_digest: [u8; 32],
    owner: Option<StaticOwnerId>,
}

#[derive(Serialize)]
struct CanonicalOutInputBindingV1 {
    formal: DeclId,
    value: CanonicalOutInputValueV1,
}

#[derive(Serialize)]
enum CanonicalOutInputValueV1 {
    Checked(ScopedCheckedExpr),
    ProducerParameter {
        parameter: ProducerParameterId,
        flow_mode: FlowMode,
        flow_type_digest: [u8; 32],
    },
}

#[derive(Serialize)]
struct CanonicalOutCallComponentV1 {
    header: CanonicalOutCallHeaderV1,
    inputs: Vec<CanonicalOutInputBindingV1>,
    passed: Option<PassedBinding>,
}

fn dependency_out_type_digest<'a>(
    ty: &'a Type,
    cache: &mut HashMap<&'a Type, [u8; 32]>,
    scratch: &mut Vec<u8>,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    if let Some(digest) = cache.get(ty) {
        return Ok(*digest);
    }
    let digest = boon_contract::canonical_serde_hash_v1_with_buffer(
        DEPENDENCY_OUT_TYPE_DIGEST_DOMAIN,
        ty,
        scratch,
    )
    .map_err(|error| {
        CallableDependencyManifestError::new(format!("failed to hash canonical OUT type: {error}"))
    })?;
    cache.insert(ty, digest);
    Ok(digest)
}

fn inventory_out(
    out: &ResolvedOutGraph,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let trace = std::env::var_os("BOON_SEMANTIC_TRACE").is_some();
    let component_started = trace.then(std::time::Instant::now);
    let mut type_digests = HashMap::<&Type, [u8; 32]>::new();
    let mut type_digest_scratch = Vec::new();
    let canonical_calls = out
        .call_instances
        .iter()
        .map(|call| {
            let local_type_substitutions = call
                .local_type_substitutions
                .iter()
                .map(|substitution| {
                    Ok(CanonicalOutTypeSubstitutionV1 {
                        variable: substitution.variable,
                        value_digest: dependency_out_type_digest(
                            &substitution.value,
                            &mut type_digests,
                            &mut type_digest_scratch,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, CallableDependencyManifestError>>()?;
            let inputs = call
                .inputs
                .iter()
                .map(|input| {
                    let value = match &input.value {
                        OutInputValue::Checked(value) => CanonicalOutInputValueV1::Checked(*value),
                        OutInputValue::ProducerParameter {
                            parameter,
                            flow_type,
                        } => CanonicalOutInputValueV1::ProducerParameter {
                            parameter: *parameter,
                            flow_mode: flow_type.mode,
                            flow_type_digest: dependency_out_type_digest(
                                &flow_type.ty,
                                &mut type_digests,
                                &mut type_digest_scratch,
                            )?,
                        },
                    };
                    Ok(CanonicalOutInputBindingV1 {
                        formal: input.formal,
                        value,
                    })
                })
                .collect::<Result<Vec<_>, CallableDependencyManifestError>>()?;
            Ok(CanonicalOutCallComponentV1 {
                header: CanonicalOutCallHeaderV1 {
                    id: call.id,
                    parent: call.parent,
                    provenance: call.provenance,
                    parent_output: call.parent_output,
                    ports: call.ports.clone(),
                    local_type_substitutions,
                    result_mode: call.result.mode,
                    result_type_digest: dependency_out_type_digest(
                        &call.result.ty,
                        &mut type_digests,
                        &mut type_digest_scratch,
                    )?,
                    owner: call.owner,
                },
                inputs,
                passed: call.passed,
            })
        })
        .collect::<Result<Vec<_>, CallableDependencyManifestError>>()?;
    // OutNet's private lookup maps and parent-output node indexes are derived
    // acceleration structures. The canonical payload commits the dense graph,
    // every exact type through its domain-separated digest, and producer roots
    // without serializing repeated inherited type trees per contextual frame.
    let canonical_out_payload = (
        &canonical_calls,
        &out.ports,
        &out.nets,
        &out.static_owners,
        out.producer_roots(),
    );
    let component_digest = collector.structural_with_component_digest(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ResolvedOutGraph,
            SemanticDependencyEntityV1::Program,
        ),
        &canonical_out_payload,
    )?;
    if let Some(started) = component_started {
        eprintln!(
            "boon_semantic dependency_manifest inventory_out.component:done elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    let calls_started = trace.then(std::time::Instant::now);
    for (call, canonical_call) in out.call_instances.iter().zip(&canonical_calls) {
        let owner = owners.out_call(call.id)?;
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::OutCallInstance,
            call.id.as_usize(),
        );
        let callee = owners
            .callable_by_checked
            .get(&call.provenance.callable)
            .copied()
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "OUT call {} references callable declaration {} without semantic identity",
                    call.id, call.provenance.callable.0
                ))
            })?;
        let mut call_references = vec![PendingDependencyReference::Owner(
            SemanticDependencyOwnerV1::Callable { callable: callee },
        )];
        call_references.extend(call.parent.map(|parent| {
            dependency_entity(SemanticDependencyEntityV1::indexed(
                SemanticDependencyEntityDomainV1::OutCallInstance,
                parent.as_usize(),
            ))
        }));
        collector.dependency_with_flow_type_digest(
            DependencyRecordInput {
                owner,
                channel: SemanticDependencyChannelV1::CalledCallable,
                roles: vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                subject: top_subject(
                    SemanticDependencySubjectKindV1::OutCallInstance,
                    entity.clone(),
                ),
                semantics: SemanticDependencySemanticsV1 {
                    call_instance: Some(call.id),
                    static_owner: call.owner,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                payload: &canonical_call.header,
                references: call_references,
            },
            Some(canonical_call.header.result_type_digest),
        )?;
        for (ordinal, input) in canonical_call.inputs.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::OutInputBinding,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    call_instance: Some(call.id),
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                input,
                Vec::new(),
            )?;
        }
        if let Some(passed) = &call.passed {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::PassedContextFormal,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(SemanticDependencySubjectKindV1::OutPassedBinding, entity, 0),
                SemanticDependencySemanticsV1 {
                    call_instance: Some(call.id),
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                passed,
                Vec::new(),
            )?;
        }
    }
    if let Some(started) = calls_started {
        eprintln!(
            "boon_semantic dependency_manifest inventory_out.calls:done elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    let ports_started = trace.then(std::time::Instant::now);
    for port in &out.ports {
        let owner = owners.out_call(port.call)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::OutFormal,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::OutPort,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::OutPort,
                    port.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                call_instance: Some(port.call),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            port,
            Vec::new(),
        )?;
    }
    if let Some(started) = ports_started {
        eprintln!(
            "boon_semantic dependency_manifest inventory_out.ports:done elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    let nets_started = trace.then(std::time::Instant::now);
    for net in &out.nets {
        let static_owner = net.owner.ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "OUT net {} has no explicit static owner identity",
                net.id
            ))
        })?;
        let owner = owners.static_owner(static_owner)?;
        let entity = SemanticDependencyEntityV1::indexed(
            SemanticDependencyEntityDomainV1::OutNet,
            net.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::OutFormal,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(SemanticDependencySubjectKindV1::OutNet, entity.clone()),
            SemanticDependencySemanticsV1 {
                static_owner: net.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            net,
            Vec::new(),
        )?;
        for (ordinal, producer) in net.producers.iter().enumerate() {
            collector.structural(
                owner,
                child_subject(
                    SemanticDependencySubjectKindV1::OutStructuralProducer,
                    entity.clone(),
                    ordinal,
                ),
                producer,
            )?;
        }
    }
    if let Some(started) = nets_started {
        eprintln!(
            "boon_semantic dependency_manifest inventory_out.nets:done elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    let owners_started = trace.then(std::time::Instant::now);
    for owner in &out.static_owners {
        collect_dependency!(
            collector,
            owners.static_owner(owner.id)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::OutStaticOwner,
                SemanticDependencyEntityV1::indexed(
                    SemanticDependencyEntityDomainV1::StaticOwner,
                    owner.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(owner.id),
                ..SemanticDependencySemanticsV1::default()
            },
            owner,
            Vec::new(),
        )?;
    }
    if let Some(started) = owners_started {
        eprintln!(
            "boon_semantic dependency_manifest inventory_out.owners:done elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    Ok(component_digest)
}

fn inventory_execution(
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let component_digest = collector.structural_with_component_digest(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ExecutionGraph,
            SemanticDependencyEntityV1::Program,
        ),
        execution,
    )?;

    for scope in &execution.scopes {
        let owner = owners.checked_scope(scope.checked_scope)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticScope,
            scope.id.as_usize(),
        );
        let references = scope
            .parent
            .map(|parent| {
                vec![dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticScope,
                    parent.as_usize(),
                ))]
            })
            .unwrap_or_default();
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(SemanticDependencySubjectKindV1::ExecutionScope, entity),
            SemanticDependencySemanticsV1::default(),
            scope,
            references,
        )?;
    }

    for expression in &execution.expressions {
        let origin = execution
            .checked_expression_origins
            .get(expression.id.as_usize())
            .filter(|origin| origin.expression == expression.id)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "semantic expression {} has no exact checked origin",
                    expression.id
                ))
            })?;
        let expression_owner = owners.expression(expression.id)?;
        let owner = if let Some(static_owner) = expression.owner {
            exact_owner(
                vec![expression_owner, owners.static_owner(static_owner)?],
                &format!("semantic expression {}", expression.id),
            )?
        } else {
            expression_owner
        };
        let (channel, roles, projection, mut references) =
            semantic_expression_dependency(expression, owners)?;
        references.push(dependency_entity(SemanticDependencyEntityV1::checked(
            SemanticDependencyEntityDomainV1::CheckedExpression,
            expression.checked_expr_id.0,
        )));
        references.extend(
            origin
                .owning_statement
                .map(statement_entity)
                .map(dependency_entity),
        );
        references.extend(
            origin
                .call_instance
                .map(out_call_entity)
                .map(dependency_entity),
        );
        references.extend(
            expression
                .owner
                .map(static_owner_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            channel,
            roles,
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionExpression,
                expression_entity(expression.id),
            ),
            SemanticDependencySemanticsV1 {
                projection,
                flow_type: Some(expression.flow_type.clone()),
                call_instance: origin.call_instance,
                static_owner: expression.owner,
                phase: semantic_expression_phase(&expression.kind),
                multiplicity: semantic_expression_multiplicity(&expression.kind),
                lifetime: semantic_expression_lifetime(&expression.kind),
                ..SemanticDependencySemanticsV1::default()
            },
            expression,
            references,
        )?;
    }

    for origin in &execution.checked_expression_origins {
        collector.diagnostic(
            owners.expression(origin.expression)?,
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionExpressionOrigin,
                expression_entity(origin.expression),
            ),
            origin,
        )?;
    }

    for statement in &execution.statements {
        let owner = owners.statement(statement.id)?;
        let mut references = Vec::new();
        references.extend(
            statement
                .parent
                .map(|parent| dependency_entity(statement_entity(parent))),
        );
        references.extend(
            statement
                .value
                .map(|value| dependency_entity(expression_entity(value))),
        );
        references.extend(
            statement
                .children
                .iter()
                .copied()
                .map(statement_entity)
                .map(dependency_entity),
        );
        match &statement.origin {
            SemanticStatementOrigin::Checked {
                statement: checked_statement,
            } => {
                references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedStatement,
                    checked_statement.0,
                )));
            }
            SemanticStatementOrigin::ProducerResult { callable, .. } => {
                let callable = owners
                    .callable_by_checked
                    .get(callable)
                    .copied()
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "semantic producer-result statement {} references checked callable {} without semantic identity",
                            statement.id, callable.0
                        ))
                    })?;
                references.push(dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable,
                }));
            }
        }
        collect_dependency!(
            collector,
            owner,
            match &statement.kind {
                SemanticStatementKind::Source { .. }
                | SemanticStatementKind::Hold { .. }
                | SemanticStatementKind::List { .. } => {
                    SemanticDependencyChannelV1::ResourceBehavior
                }
                SemanticStatementKind::Field { .. }
                | SemanticStatementKind::Block
                | SemanticStatementKind::Spread
                | SemanticStatementKind::Expression => {
                    SemanticDependencyChannelV1::StructuralRepresentation
                }
            },
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionStatement,
                statement_entity(statement.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: statement.flow_type.clone(),
                call_instance: statement.call_instance,
                ..SemanticDependencySemanticsV1::default()
            },
            statement,
            references,
        )?;
    }

    for callable in &execution.callables {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: callable.id,
        };
        let entity = callable_entity(callable.id);
        let mut references = vec![dependency_entity(indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticScope,
            callable.scope.as_usize(),
        ))];
        references.extend(callable.body.map(|statement| {
            dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedStatement,
                statement.0,
            ))
        }));
        references.extend(callable.result_expression.map(|expression| {
            dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedExpression,
                expression.0,
            ))
        }));
        references.extend(
            callable
                .semantic_root
                .map(expression_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::SemanticProfile,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionCallable,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(callable.result.clone()),
                program_role: Some(callable.role),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            callable,
            references,
        )?;
        for (ordinal, parameter) in callable.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                match parameter.evaluation_scope {
                    CheckedEvaluationScope::Parent => {
                        SemanticDependencyChannelV1::ParentValueFormal
                    }
                    CheckedEvaluationScope::Output { .. } => {
                        SemanticDependencyChannelV1::OutputEvaluatedFormal
                    }
                },
                vec![SemanticDependencyRoleV1::FormulaBinder],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallableParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    evaluation_scope: Some(parameter.evaluation_scope),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                vec![dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    parameter.formal.0,
                ))],
            )?;
        }
        for (ordinal, context) in callable.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallableContext,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(context.flow_type.clone()),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                vec![dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    context.provider.0,
                ))],
            )?;
        }
    }

    for call in &execution.calls {
        let owner = owners.call(call.id)?;
        let entity = call_entity(call.id);
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CalledCallable,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionCall,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(call.result.clone()),
                program_role: Some(call.role),
                lifetime: SemanticDependencyLifetimeV1::Call,
                ..SemanticDependencySemanticsV1::default()
            },
            call,
            vec![
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: call.callable,
                }),
                dependency_entity(callable_entity(call.callable)),
            ],
        )?;
        for (ordinal, entry) in call.entries.iter().enumerate() {
            let (channel, flow_type, reference) = match entry {
                SemanticCallEntry::Input {
                    checked_value,
                    value_flow_type,
                    evaluation_scope,
                    ..
                } => (
                    match evaluation_scope {
                        CheckedEvaluationScope::Parent => {
                            SemanticDependencyChannelV1::ParentValueFormal
                        }
                        CheckedEvaluationScope::Output { .. } => {
                            SemanticDependencyChannelV1::OutputEvaluatedFormal
                        }
                    },
                    Some(value_flow_type.clone()),
                    Some(dependency_entity(SemanticDependencyEntityV1::checked(
                        SemanticDependencyEntityDomainV1::CheckedExpression,
                        checked_value.0,
                    ))),
                ),
                SemanticCallEntry::FreshOut { .. } | SemanticCallEntry::ForwardOut { .. } => {
                    (SemanticDependencyChannelV1::OutFormal, None, None)
                }
            };
            collect_dependency!(
                collector,
                owner,
                channel,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallEntry,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type,
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                entry,
                reference.into_iter().collect(),
            )?;
        }
        for (ordinal, context) in call.contexts.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::CompilerContext,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionCallContext,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                context,
                Vec::new(),
            )?;
        }
    }

    for occurrence in &execution.call_occurrences {
        let owner = occurrence
            .call
            .map(|call| owners.call(call))
            .transpose()?
            .unwrap_or(SemanticDependencyOwnerV1::ProgramRoot);
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticCallOccurrence,
            occurrence.id.as_usize(),
        );
        let mut references = vec![dependency_entity(out_call_entity(occurrence.id))];
        references.extend(occurrence.call.map(call_entity).map(dependency_entity));
        references.extend(occurrence.parent.map(|parent| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticCallOccurrence,
                parent.as_usize(),
            ))
        }));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionCallOccurrence,
                entity,
            ),
            SemanticDependencySemanticsV1 {
                call_instance: Some(occurrence.id),
                lifetime: SemanticDependencyLifetimeV1::Call,
                ..SemanticDependencySemanticsV1::default()
            },
            occurrence,
            references,
        )?;
    }

    for source in &execution.sources {
        let owner = owners.source(source.id)?;
        let mut references = vec![
            dependency_entity(statement_entity(source.statement)),
            dependency_entity(expression_entity(source.expression)),
        ];
        match &source.origin {
            SemanticSourceOrigin::Checked {
                source: checked_source,
            } => {
                references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedSource,
                    checked_source.0,
                )));
            }
            SemanticSourceOrigin::ProducerInvocation { function, .. } => {
                references.push(dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: *function,
                }));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionSource,
                source_entity(source.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: source.owner,
                call_instance: source.call_instance,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            references,
        )?;
    }

    for state in &execution.states {
        let owner = owners.state(state.id)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionState,
                state_entity(state.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: state.owner,
                call_instance: state.call_instance,
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::Commit,
                ..SemanticDependencySemanticsV1::default()
            },
            state,
            vec![
                dependency_entity(statement_entity(state.statement)),
                dependency_entity(expression_entity(state.expression)),
                dependency_entity(expression_entity(state.initial)),
                dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedState,
                    state.checked_state.0,
                )),
            ],
        )?;
    }

    for (ordinal, root) in execution.roots.iter().enumerate() {
        let owner = owners.expression(root.expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            child_subject(
                SemanticDependencySubjectKindV1::ExecutionRoot,
                SemanticDependencyEntityV1::Program,
                ordinal,
            ),
            SemanticDependencySemanticsV1::default(),
            root,
            vec![dependency_entity(expression_entity(root.expression))],
        )?;
    }

    for function in &execution.functions {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: function.callable,
        };
        let entity = SemanticDependencyEntityV1::Digest {
            domain: SemanticDependencyEntityDomainV1::SemanticFunction,
            digest: function.identity,
        };
        let mut references = vec![
            dependency_owner(owner),
            dependency_entity(expression_entity(function.root)),
        ];
        references.extend(
            function
                .invocation_source
                .map(expression_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionFunction,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(function.result_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            function,
            references,
        )?;
        for (ordinal, parameter) in function.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                child_subject(
                    SemanticDependencySubjectKindV1::ExecutionFunctionParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                parameter
                    .input_expressions
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity)
                    .collect(),
            )?;
        }
    }

    for materialization in &execution.materializations {
        let owner = owners.static_owner(materialization.owner)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMaterialization,
            materialization.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionMaterialization,
                entity,
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(materialization.owner),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            materialization,
            materialization
                .expression_roots()
                .into_iter()
                .map(expression_entity)
                .map(dependency_entity)
                .collect(),
        )?;
    }

    for static_owner in &execution.static_owners {
        collect_dependency!(
            collector,
            owners.static_owner(static_owner.id)?,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::ExecutionStaticOwner,
                static_owner_entity(static_owner.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(static_owner.id),
                ..SemanticDependencySemanticsV1::default()
            },
            static_owner,
            static_owner
                .parent
                .map(static_owner_entity)
                .map(dependency_entity)
                .into_iter()
                .collect(),
        )?;
    }
    Ok(component_digest)
}

fn semantic_expression_dependency(
    expression: &SemanticExpression,
    owners: &DependencyOwnerIndex,
) -> Result<ExpressionDependency, CallableDependencyManifestError> {
    let mut references = Vec::new();
    let (channel, roles, projection) = match &expression.kind {
        SemanticExpressionKind::CanonicalRead {
            target,
            projection,
            source,
            ..
        } => {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedDeclaration,
                target.0,
            )));
            references.extend(
                source
                    .as_ref()
                    .map(|source| source_entity(source.source))
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::LexicalCapture,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::LocalRead {
            declaration,
            projection,
            ..
        } => {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedDeclaration,
                declaration.0,
            )));
            (
                SemanticDependencyChannelV1::LocalFact,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::ExternalRead { .. } => (
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            Vec::new(),
        ),
        SemanticExpressionKind::ElementState { projection, .. } => (
            SemanticDependencyChannelV1::CompilerContext,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            projection.clone(),
        ),
        SemanticExpressionKind::Drain {
            target, projection, ..
        } => {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedDeclaration,
                target.0,
            )));
            (
                SemanticDependencyChannelV1::MigrationPredecessor,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                projection.clone(),
            )
        }
        SemanticExpressionKind::Call {
            call,
            callable,
            instance,
            arguments,
            parameter_bindings,
            context_argument,
            ..
        } => {
            references.push(dependency_entity(call_entity(*call)));
            if let Some(instance) = instance {
                references.push(dependency_entity(out_call_entity(*instance)));
            }
            references.push(dependency_owner(SemanticDependencyOwnerV1::Callable {
                callable: *callable,
            }));
            references.extend(
                arguments
                    .iter()
                    .map(|argument| argument.value)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            references.extend(parameter_bindings.iter().filter_map(
                |binding| match &binding.kind {
                    SemanticCallParameterBindingKind::Explicit { value, .. } => {
                        Some(dependency_entity(expression_entity(*value)))
                    }
                    SemanticCallParameterBindingKind::Omitted => None,
                },
            ));
            references.extend(
                context_argument
                    .iter()
                    .map(|argument| argument.value)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::CalledCallable,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Materialize { materialization } => {
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticMaterialization,
                materialization.as_usize(),
            )));
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Draining { input } => {
            references.push(dependency_entity(expression_entity(*input)));
            (
                SemanticDependencyChannelV1::MigrationPredecessor,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Source { .. } => (
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            Vec::new(),
        ),
        SemanticExpressionKind::Hold {
            initial, updates, ..
        } => {
            references.push(dependency_entity(expression_entity(*initial)));
            references.extend(
                updates
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Latest { branches } => {
            references.extend(
                branches
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
                Vec::new(),
            )
        }
        SemanticExpressionKind::When { input, arms, .. } => {
            references.push(dependency_entity(expression_entity(*input)));
            references.extend(
                arms.iter()
                    .map(|arm| arm.output)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Then { input, output } => {
            references.push(dependency_entity(expression_entity(*input)));
            references.extend(output.map(expression_entity).map(dependency_entity));
            (
                SemanticDependencyChannelV1::ResourceBehavior,
                vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Infix { left, right, .. } => {
            references.push(dependency_entity(expression_entity(*left)));
            references.push(dependency_entity(expression_entity(*right)));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::MapEntry { key, value } => {
            references.push(dependency_entity(expression_entity(*key)));
            references.push(dependency_entity(expression_entity(*value)));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::MatchArm { output, .. } => {
            references.extend(output.map(expression_entity).map(dependency_entity));
            (
                SemanticDependencyChannelV1::CoverageRouting,
                vec![SemanticDependencyRoleV1::CoverageOrRouting],
                Vec::new(),
            )
        }
        SemanticExpressionKind::TaggedObject { fields, .. }
        | SemanticExpressionKind::Object(fields) => {
            references.extend(
                fields
                    .iter()
                    .map(|field| field.value)
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::TextTemplate { segments } => {
            references.extend(segments.iter().filter_map(|segment| match segment {
                SemanticTextSegment::Static { .. } => None,
                SemanticTextSegment::Dynamic { value } => {
                    Some(dependency_entity(expression_entity(*value)))
                }
            }));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Block { bindings, result } => {
            references.extend(
                bindings
                    .iter()
                    .map(|binding| binding.value)
                    .chain(std::iter::once(*result))
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::LocalFact,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::List { items, .. }
        | SemanticExpressionKind::Bytes { items, .. }
        | SemanticExpressionKind::Map { entries: items }
        | SemanticExpressionKind::Set { items } => {
            references.extend(
                items
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                Vec::new(),
            )
        }
        SemanticExpressionKind::Project { input, fields } => {
            references.push(dependency_entity(expression_entity(*input)));
            (
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![SemanticDependencyRoleV1::FixedDefinition],
                fields.clone(),
            )
        }
        SemanticExpressionKind::MaterializationLocal {
            owner, projection, ..
        } => {
            references.push(dependency_entity(static_owner_entity(*owner)));
            (
                SemanticDependencyChannelV1::LocalFact,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::FunctionParameter {
            parameter,
            projection,
        } => {
            let owner = SemanticDependencyOwnerV1::Callable {
                callable: parameter.callable,
            };
            if !owners.callable_by_checked.values().any(|callable| {
                SemanticDependencyOwnerV1::Callable {
                    callable: *callable,
                } == owner
            }) {
                return Err(CallableDependencyManifestError::new(format!(
                    "semantic expression {} references missing function parameter owner {}",
                    expression.id, parameter.callable
                )));
            }
            references.push(dependency_owner(owner));
            (
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                projection.clone(),
            )
        }
        SemanticExpressionKind::Text(_)
        | SemanticExpressionKind::Number(_)
        | SemanticExpressionKind::Bits(_)
        | SemanticExpressionKind::BytesByte(_)
        | SemanticExpressionKind::Absent
        | SemanticExpressionKind::Flush { .. }
        | SemanticExpressionKind::FlushBoundary { .. }
        | SemanticExpressionKind::Tag(_)
        | SemanticExpressionKind::Delimiter => (
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            Vec::new(),
        ),
    };
    Ok((channel, roles, projection, references))
}

fn semantic_expression_phase(kind: &SemanticExpressionKind) -> SemanticDependencyPhaseV1 {
    match kind {
        SemanticExpressionKind::Drain { .. } | SemanticExpressionKind::Draining { .. } => {
            SemanticDependencyPhaseV1::PreviousCommittedValue
        }
        SemanticExpressionKind::Source { .. } => SemanticDependencyPhaseV1::EventPayload,
        SemanticExpressionKind::Hold { .. } => SemanticDependencyPhaseV1::CandidateWrite,
        _ => SemanticDependencyPhaseV1::CurrentValue,
    }
}

fn semantic_expression_multiplicity(
    kind: &SemanticExpressionKind,
) -> SemanticDependencyMultiplicityV1 {
    match kind {
        SemanticExpressionKind::Source { .. } => SemanticDependencyMultiplicityV1::PerEvent,
        SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::MaterializationLocal { .. } => {
            SemanticDependencyMultiplicityV1::PerRowMaterialization
        }
        SemanticExpressionKind::Hold { .. } => SemanticDependencyMultiplicityV1::PerTransition,
        _ => SemanticDependencyMultiplicityV1::PerTick,
    }
}

fn semantic_expression_lifetime(kind: &SemanticExpressionKind) -> SemanticDependencyLifetimeV1 {
    match kind {
        SemanticExpressionKind::Source { .. } => SemanticDependencyLifetimeV1::Event,
        SemanticExpressionKind::Materialize { .. }
        | SemanticExpressionKind::MaterializationLocal { .. } => SemanticDependencyLifetimeV1::Row,
        SemanticExpressionKind::Hold { .. }
        | SemanticExpressionKind::Drain { .. }
        | SemanticExpressionKind::Draining { .. } => SemanticDependencyLifetimeV1::Snapshot,
        SemanticExpressionKind::Call { .. } | SemanticExpressionKind::FunctionParameter { .. } => {
            SemanticDependencyLifetimeV1::Call
        }
        _ => SemanticDependencyLifetimeV1::Definition,
    }
}

fn inventory_resources(
    resources: &SemanticResourceGraphV1,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ResourceGraph,
            SemanticDependencyEntityV1::Program,
        ),
        resources,
    )?;

    for row_scope in &resources.row_scopes {
        let owner = owners.list(row_scope.list)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![SemanticDependencyRoleV1::CoverageOrRouting],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceRowScope,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticRowScope,
                    row_scope.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                row: Some(SemanticRowBinding {
                    list: row_scope.list,
                    scope: row_scope.id,
                }),
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            row_scope,
            vec![dependency_entity(list_entity(row_scope.list))],
        )?;
    }

    for list in &resources.lists {
        let owner = owners.list(list.id)?;
        let mut references = vec![
            dependency_entity(statement_entity(list.statement)),
            dependency_entity(expression_entity(list.producer)),
        ];
        if let SemanticListResourceOriginV1::CheckedLiteral { checked_list } = &list.origin {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedList,
                checked_list.0,
            )));
        }
        references.extend(resource_initializer_expressions(&list.initializer));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceList,
                list_entity(list.id),
            ),
            SemanticDependencySemanticsV1 {
                row: Some(SemanticRowBinding {
                    list: list.id,
                    scope: list.row_scope,
                }),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            list,
            references,
        )?;
    }

    for authority in &resources.value_list_authorities {
        let owner = owners.value_list(authority.id)?;
        let mut references = vec![
            dependency_entity(statement_entity(authority.statement)),
            dependency_entity(expression_entity(authority.producer)),
        ];
        if let SemanticListResourceOriginV1::CheckedLiteral { checked_list } = &authority.origin {
            references.push(dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedList,
                checked_list.0,
            )));
        }
        references.extend(resource_initializer_expressions(&authority.initializer));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceValueListAuthority,
                value_list_entity(authority.id),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            authority,
            references,
        )?;
    }

    for source in &resources.sources {
        let primary = owners.source(source.id)?;
        let owner = if let Some(static_owner) = source.owner {
            exact_owner(
                vec![primary, owners.static_owner(static_owner)?],
                &format!("resource source {}", source.id),
            )?
        } else {
            primary
        };
        let mut references = vec![
            dependency_entity(statement_entity(source.statement)),
            dependency_entity(expression_entity(source.expression)),
        ];
        references.extend(source.target_list.map(list_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceSource,
                source_entity(source.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: source.owner,
                row: source
                    .target_list
                    .zip(source.row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            references,
        )?;
    }

    for state in &resources.states {
        let primary = owners.state(state.id)?;
        let owner = if let Some(static_owner) = state.owner {
            exact_owner(
                vec![primary, owners.static_owner(static_owner)?],
                &format!("resource state {}", state.id),
            )?
        } else {
            primary
        };
        let mut references = vec![
            dependency_entity(statement_entity(state.statement)),
            dependency_entity(expression_entity(state.expression)),
            dependency_entity(expression_entity(state.initial)),
        ];
        references.extend(
            state
                .expression_members
                .iter()
                .copied()
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(state.target_list.map(list_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceState,
                state_entity(state.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(state.flow_type.clone()),
                static_owner: state.owner,
                row: state
                    .target_list
                    .zip(state.row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::Commit,
                ..SemanticDependencySemanticsV1::default()
            },
            state,
            references,
        )?;
    }

    for (index, alias) in resources.aliases.iter().enumerate() {
        let target_owner = match alias.target {
            SemanticResourceAliasTargetV1::Source(source) => owners.source(source)?,
            SemanticResourceAliasTargetV1::State(state) => owners.state(state)?,
        };
        let owner = if let Some(static_owner) = alias.owner {
            exact_owner(
                vec![target_owner, owners.static_owner(static_owner)?],
                &format!("resource alias {index}"),
            )?
        } else {
            target_owner
        };
        let target = match alias.target {
            SemanticResourceAliasTargetV1::Source(source) => source_entity(source),
            SemanticResourceAliasTargetV1::State(state) => state_entity(state),
        };
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::LexicalCapture,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceAlias,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticResourceAlias,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: alias.owner,
                ..SemanticDependencySemanticsV1::default()
            },
            alias,
            vec![dependency_entity(target)],
        )?;
    }

    for binding in &resources.materialization_bindings {
        let owner = owners.static_owner(binding.owner)?;
        let mut references = vec![dependency_entity(indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMaterialization,
            binding.materialization.as_usize(),
        ))];
        references.extend(
            binding
                .source
                .into_iter()
                .chain(binding.target)
                .map(|row| list_entity(row.list))
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceMaterializationBinding,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMaterializationBinding,
                    binding.materialization.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(binding.owner),
                row: binding.target.or(binding.source),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            references,
        )?;
    }

    for (index, projection) in resources.list_projections.iter().enumerate() {
        let owner = owners.list(projection.target)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceListProjection,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticListProjection,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            projection,
            vec![
                dependency_entity(list_entity(projection.target)),
                dependency_entity(list_entity(projection.source)),
            ],
        )?;
    }

    for producer in &resources.producer_resources {
        let owner = owners.static_owner(producer.owner)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ResourceProducer,
                SemanticDependencyEntityV1::Digest {
                    domain: SemanticDependencyEntityDomainV1::SemanticProducerResource,
                    digest: producer.identity,
                },
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(producer.owner),
                call_instance: Some(producer.root_call),
                ..SemanticDependencySemanticsV1::default()
            },
            producer,
            vec![
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: producer.callable,
                }),
                dependency_entity(statement_entity(producer.result_statement)),
            ],
        )?;
    }
    let _ = execution;
    Ok(())
}

fn resource_initializer_expressions(
    initializer: &SemanticListInitializerV1,
) -> Vec<PendingDependencyReference> {
    let mut expressions = Vec::new();
    match initializer {
        SemanticListInitializerV1::Empty => {}
        SemanticListInitializerV1::RecordLiteral {
            authority_root,
            rows,
        } => {
            expressions.push(*authority_root);
            for row in rows {
                expressions.push(row.expression);
                for field in &row.fields {
                    expressions.extend(field.expression);
                    expressions.extend(field.spread_origin);
                }
            }
        }
        SemanticListInitializerV1::ValueLiteral {
            authority_root,
            values,
        } => {
            expressions.push(*authority_root);
            expressions.extend(values.iter().map(|value| value.expression));
        }
        SemanticListInitializerV1::Range {
            authority_root,
            from_expression,
            to_expression,
            ..
        } => {
            expressions.push(*authority_root);
            expressions.push(*from_expression);
            expressions.push(*to_expression);
        }
    }
    expressions.sort();
    expressions.dedup();
    expressions
        .into_iter()
        .map(expression_entity)
        .map(dependency_entity)
        .collect()
}

fn inventory_reactive(
    reactive: &SemanticReactiveGraphV1,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<[u8; 32], CallableDependencyManifestError> {
    let component_digest = collector.structural_with_component_digest(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ReactiveGraph,
            SemanticDependencyEntityV1::Program,
        ),
        reactive,
    )?;

    for producer in &reactive.producer_instances {
        let owner = owners.static_owner(producer.owner)?;
        let entity = SemanticDependencyEntityV1::Digest {
            domain: SemanticDependencyEntityDomainV1::SemanticProducerInstance,
            digest: producer.identity,
        };
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveProducerInstance,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(producer.owner),
                call_instance: Some(producer.root_call),
                multiplicity: match producer.mode {
                    ProducerMaterializationMode::Current => {
                        SemanticDependencyMultiplicityV1::PerTick
                    }
                    ProducerMaterializationMode::Invocation => {
                        SemanticDependencyMultiplicityV1::PerEvent
                    }
                },
                ..SemanticDependencySemanticsV1::default()
            },
            producer,
            vec![
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: producer.callable,
                }),
                dependency_entity(expression_entity(producer.root_expression)),
                dependency_entity(statement_entity(producer.result_statement)),
            ],
        )?;
        for (ordinal, parameter) in producer.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::ParentValueFormal,
                vec![SemanticDependencyRoleV1::FormulaBinder],
                child_subject(
                    SemanticDependencySubjectKindV1::ReactiveProducerParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    lifetime: SemanticDependencyLifetimeV1::Call,
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                parameter
                    .input_expressions
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity)
                    .collect(),
            )?;
        }
    }

    for field in &reactive.fields {
        let owner = owner_for_expression_and_static(
            owners,
            field.producer,
            field.owner,
            &format!("reactive field {}", field.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveField,
                field_entity(field.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(field.flow_type.clone()),
                static_owner: field.owner,
                row: field.row,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            field,
            vec![
                dependency_entity(statement_entity(field.statement)),
                dependency_entity(expression_entity(field.producer)),
            ],
        )?;
    }

    for binding in &reactive.bindings {
        let owner = owner_for_expression_and_static(
            owners,
            binding.producer,
            binding.owner,
            &format!("reactive binding {}", binding.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveBinding,
                binding_entity(binding.id),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(binding.flow_type.clone()),
                static_owner: binding.owner,
                call_instance: binding.call_instance,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            vec![
                dependency_entity(statement_entity(binding.statement)),
                dependency_entity(expression_entity(binding.producer)),
                dependency_entity(binding_target_entity(binding.target)),
            ],
        )?;
    }

    for read in &reactive.reads {
        let owner = owners.expression(read.expression)?;
        let (channel, projection, references) = reactive_read_dependency(&read.target);
        collect_dependency!(
            collector,
            owner,
            channel,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveRead,
                read_entity(read.id),
            ),
            SemanticDependencySemanticsV1 {
                projection,
                phase: SemanticDependencyPhaseV1::CurrentValue,
                ..SemanticDependencySemanticsV1::default()
            },
            read,
            references,
        )?;
    }

    for dependency in &reactive.dependency_uses {
        let owner = exact_owner(
            vec![
                owners.binding(dependency.dependent)?,
                owners.expression(dependency.expression)?,
            ],
            &format!("reactive dependency use {}", dependency.id),
        )?;
        let mut references = vec![dependency_entity(binding_entity(dependency.dependent))];
        match &dependency.target {
            SemanticDependencyTargetV1::ExternalRead { read } => {
                references.push(dependency_entity(read_entity(*read)));
            }
            SemanticDependencyTargetV1::ExternalCall { call, expression } => {
                references.push(dependency_entity(call_entity(*call)));
                references.push(dependency_entity(expression_entity(*expression)));
            }
        }
        if let SemanticDependencyTimingV1::After { boundaries } = &dependency.timing {
            references.extend(
                boundaries
                    .iter()
                    .copied()
                    .map(event_cause_entity)
                    .map(dependency_entity),
            );
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveDependencyUse,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticDependencyUse,
                    dependency.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            dependency,
            references,
        )?;
    }

    for schedule in &reactive.call_invocations {
        let owner = owners.expression(schedule.expression)?;
        let mut references = vec![
            dependency_entity(expression_entity(schedule.expression)),
            dependency_entity(call_entity(schedule.call)),
        ];
        references.extend(
            schedule
                .dependent_bindings
                .iter()
                .copied()
                .map(binding_entity)
                .map(dependency_entity),
        );
        references.extend(schedule.invocation_arms.iter().copied().map(|arm| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                arm.as_usize(),
            ))
        }));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveCallSchedule,
                expression_entity(schedule.expression),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: if schedule.current_capable {
                    SemanticDependencyMultiplicityV1::PerTick
                } else {
                    SemanticDependencyMultiplicityV1::PerEvent
                },
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            schedule,
            references,
        )?;
    }

    for activation in &reactive.activations {
        let owner = owners.expression(activation.then_expression)?;
        let mut references = vec![
            dependency_entity(expression_entity(activation.then_expression)),
            dependency_entity(expression_entity(activation.input_expression)),
            dependency_entity(expression_entity(activation.output_expression)),
        ];
        references.extend(
            activation
                .states
                .iter()
                .copied()
                .map(state_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveActivation,
                activation_entity(activation.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: activation.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            activation,
            references,
        )?;
    }

    for pulse in &reactive.pulse_batches {
        let owner = owners.expression(pulse.call_expression)?;
        let mut references = vec![
            dependency_entity(call_entity(pulse.call)),
            dependency_entity(expression_entity(pulse.call_expression)),
            dependency_entity(expression_entity(pulse.count_expression)),
        ];
        references.extend(
            pulse
                .enclosing_activation
                .map(activation_entity)
                .map(dependency_entity),
        );
        references.extend(pulse.state.map(state_entity).map(dependency_entity));
        references.extend(
            pulse
                .hold_expression
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(
            pulse
                .transition_expression
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(
            pulse
                .transition_output
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(pulse.trigger_arms.iter().copied().map(|arm| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                arm.as_usize(),
            ))
        }));
        references.extend(pulse.state_update_arms.iter().copied().map(|arm| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStateUpdateArm,
                arm.as_usize(),
            ))
        }));
        references.extend(pulse.list_mutations.iter().copied().map(|mutation| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticListMutation,
                mutation.as_usize(),
            ))
        }));
        references.extend(pulse.derived_values.iter().copied().map(|derived| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticDerivedValue,
                derived.as_usize(),
            ))
        }));
        references.extend(pulse.host_effect_schedules.iter().copied().map(|effect| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticHostEffect,
                effect.as_usize(),
            ))
        }));
        references.extend(
            pulse
                .flush_roots
                .iter()
                .copied()
                .map(expression_entity)
                .map(dependency_entity),
        );
        for route in &pulse.emission_routes {
            references.extend(route.consumer.map(expression_entity).map(dependency_entity));
            if let SemanticPulseEmissionFilterV1::Skip {
                call,
                expression,
                count_expression,
                ..
            } = &route.filter
            {
                references.push(dependency_entity(call_entity(*call)));
                references.push(dependency_entity(expression_entity(*expression)));
                references.push(dependency_entity(expression_entity(*count_expression)));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::RuntimeIntrinsicOrHostEffect,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactivePulseBatch,
                pulse_batch_entity(pulse.id),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::Commit,
                ..SemanticDependencySemanticsV1::default()
            },
            pulse,
            references,
        )?;
    }

    for derived in &reactive.derived_values {
        let owner = exact_owner(
            vec![
                owners.binding(derived.binding)?,
                owners.expression(derived.producer)?,
            ],
            &format!("reactive derived value {}", derived.id),
        )?;
        let mut references = vec![
            dependency_entity(binding_entity(derived.binding)),
            dependency_entity(field_entity(derived.field)),
            dependency_entity(statement_entity(derived.statement)),
            dependency_entity(expression_entity(derived.producer)),
        ];
        references.extend(
            derived
                .materialized_list
                .map(list_entity)
                .map(dependency_entity),
        );
        references.extend(
            derived
                .causes
                .iter()
                .copied()
                .map(event_cause_entity)
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveDerivedValue,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticDerivedValue,
                    derived.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                row: derived
                    .materialized_list
                    .zip(derived.materialized_row_scope)
                    .map(|(list, scope)| SemanticRowBinding { list, scope }),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            derived,
            references,
        )?;
    }

    for arm in &reactive.trigger_arms {
        let owner = owner_for_expression_and_static(
            owners,
            arm.gate_expression,
            arm.owner,
            &format!("reactive trigger arm {}", arm.id),
        )?;
        let owner = exact_owner(
            vec![owner, owners.expression(arm.output_expression)?],
            &format!("reactive trigger arm {}", arm.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveTriggerArm,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                    arm.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: arm.owner,
                row: None,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                phase: SemanticDependencyPhaseV1::EventPayload,
                ..SemanticDependencySemanticsV1::default()
            },
            arm,
            vec![
                dependency_entity(event_cause_entity(arm.cause)),
                dependency_entity(expression_entity(arm.gate_expression)),
                dependency_entity(expression_entity(arm.output_expression)),
            ],
        )?;
    }

    for arm in &reactive.state_update_arms {
        let owner = owners.state(arm.state)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveStateUpdateArm,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStateUpdateArm,
                    arm.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            arm,
            vec![
                dependency_entity(state_entity(arm.state)),
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticTriggerArm,
                    arm.trigger.as_usize(),
                )),
            ],
        )?;
    }

    for mutation in &reactive.list_mutations {
        let owner = owner_for_expression_and_static(
            owners,
            mutation.site,
            mutation.owner,
            &format!("reactive list mutation {}", mutation.id),
        )?;
        let mut references = vec![
            dependency_entity(list_entity(mutation.list)),
            dependency_entity(expression_entity(mutation.site)),
            dependency_entity(event_cause_entity(mutation.cause)),
        ];
        match &mutation.kind {
            SemanticListMutationKindV1::Append { gate, item, .. } => {
                references.push(dependency_entity(expression_entity(*gate)));
                references.push(dependency_entity(expression_entity(*item)));
            }
            SemanticListMutationKindV1::Remove {
                materialization,
                gate,
                predicate,
                owner,
                ..
            } => {
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMaterialization,
                    materialization.as_usize(),
                )));
                references.push(dependency_entity(expression_entity(*gate)));
                references.push(dependency_entity(expression_entity(*predicate)));
                references.push(dependency_entity(static_owner_entity(*owner)));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceWrite,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveListMutation,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticListMutation,
                    mutation.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: mutation.owner,
                row: mutation.row_scope.map(|scope| SemanticRowBinding {
                    list: mutation.list,
                    scope,
                }),
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Row,
                phase: SemanticDependencyPhaseV1::CandidateWrite,
                ..SemanticDependencySemanticsV1::default()
            },
            mutation,
            references,
        )?;
    }

    for edge in &reactive.dependencies {
        let owner = owners.state(edge.to)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveDependencyEdge,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticExternalDependency,
                    edge.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                multiplicity: SemanticDependencyMultiplicityV1::PerTransition,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            edge,
            vec![
                dependency_entity(event_cause_entity(edge.from)),
                dependency_entity(state_entity(edge.to)),
            ],
        )?;
    }

    for causes in &reactive.possible_causes {
        let owner = owners.state(causes.state)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactivePossibleCauses,
                state_entity(causes.state),
            ),
            SemanticDependencySemanticsV1::default(),
            causes,
            causes
                .causes
                .iter()
                .copied()
                .map(event_cause_entity)
                .map(dependency_entity)
                .collect(),
        )?;
    }

    for effect in &reactive.host_effect_schedules {
        let owner = owner_for_expression_and_static(
            owners,
            effect.expression,
            effect.owner,
            &format!("reactive host effect {}", effect.id),
        )?;
        let mut references = vec![
            dependency_entity(expression_entity(effect.expression)),
            dependency_entity(call_entity(effect.call)),
        ];
        references.extend(effect.state_update_arms.iter().copied().map(|arm| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStateUpdateArm,
                arm.as_usize(),
            ))
        }));
        references.extend(effect.transient_result.map(|derived| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticDerivedValue,
                derived.as_usize(),
            ))
        }));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::RuntimeIntrinsicOrHostEffect,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveHostEffect,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticHostEffect,
                    effect.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: effect.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::EffectCompletion,
                ..SemanticDependencySemanticsV1::default()
            },
            effect,
            references,
        )?;
    }

    for output in &reactive.output_values {
        let owner = exact_owner(
            vec![
                owners.expression(output.expression)?,
                owners.statement(output.statement)?,
            ],
            &format!("reactive output {}", output.ordinal),
        )?;
        let mut references = vec![
            dependency_entity(expression_entity(output.expression)),
            dependency_entity(statement_entity(output.statement)),
        ];
        references.extend(output.field.map(field_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::ReactiveOutput,
                SemanticDependencyEntityV1::Program,
                output.ordinal,
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            output,
            references,
        )?;
    }

    for capture in &reactive.view_captures {
        let owner = owners.expression(capture.expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::LexicalCapture,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveViewCapture,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticCapture,
                    capture.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            capture,
            vec![
                dependency_entity(expression_entity(capture.expression)),
                dependency_entity(view_capture_target_entity(capture.target)),
            ],
        )?;
    }

    for migration in &reactive.migration_inputs {
        let owner = owner_for_expression_and_static(
            owners,
            migration.marker,
            migration.owner,
            &format!("reactive migration input {}", migration.id),
        )?;
        let owner = exact_owner(
            vec![owner, owners.expression(migration.input)?],
            &format!("reactive migration input {}", migration.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::MigrationPredecessor,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ReactiveMigrationInput,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMigrationInput,
                    migration.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: migration.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::PreviousCommittedValue,
                ..SemanticDependencySemanticsV1::default()
            },
            migration,
            vec![
                dependency_entity(expression_entity(migration.marker)),
                dependency_entity(expression_entity(migration.input)),
            ],
        )?;
    }
    let _ = execution;
    Ok(component_digest)
}

fn owner_for_expression_and_static(
    owners: &DependencyOwnerIndex,
    expression: SemanticExprId,
    static_owner: Option<StaticOwnerId>,
    context: &str,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let primary = owners.expression(expression)?;
    if let Some(static_owner) = static_owner {
        exact_owner(vec![primary, owners.static_owner(static_owner)?], context)
    } else {
        Ok(primary)
    }
}

fn binding_target_entity(target: SemanticBindingTargetV1) -> SemanticDependencyEntityV1 {
    match target {
        SemanticBindingTargetV1::Field { field } => field_entity(field),
        SemanticBindingTargetV1::Source { source } => source_entity(source),
        SemanticBindingTargetV1::State { state } => state_entity(state),
        SemanticBindingTargetV1::List { list } => list_entity(list),
    }
}

fn reactive_read_dependency(
    target: &SemanticReadTargetV1,
) -> (
    SemanticDependencyChannelV1,
    Vec<String>,
    Vec<PendingDependencyReference>,
) {
    match target {
        SemanticReadTargetV1::Binding {
            binding,
            projection,
        } => (
            SemanticDependencyChannelV1::ResourceRead,
            projection.clone(),
            vec![dependency_entity(binding_entity(*binding))],
        ),
        SemanticReadTargetV1::SourcePayload {
            binding,
            source,
            payload_projection,
            projection,
        } => {
            let mut combined = payload_projection.clone();
            combined.extend(projection.iter().cloned());
            (
                SemanticDependencyChannelV1::ResourceRead,
                combined,
                vec![
                    dependency_entity(binding_entity(*binding)),
                    dependency_entity(source_entity(*source)),
                ],
            )
        }
        SemanticReadTargetV1::StateProjection {
            binding,
            state,
            projection,
        } => (
            SemanticDependencyChannelV1::ResourceRead,
            projection.clone(),
            vec![
                dependency_entity(binding_entity(*binding)),
                dependency_entity(state_entity(*state)),
            ],
        ),
        SemanticReadTargetV1::Local {
            producer,
            projection,
            ..
        } => (
            SemanticDependencyChannelV1::LocalFact,
            projection.clone(),
            vec![dependency_entity(expression_entity(*producer))],
        ),
        SemanticReadTargetV1::External { .. } => (
            SemanticDependencyChannelV1::ExternalValueOrCall,
            Vec::new(),
            Vec::new(),
        ),
        SemanticReadTargetV1::ElementState { projection, .. } => (
            SemanticDependencyChannelV1::CompilerContext,
            projection.clone(),
            Vec::new(),
        ),
        SemanticReadTargetV1::MaterializationLocal {
            owner, projection, ..
        } => (
            SemanticDependencyChannelV1::LocalFact,
            projection.clone(),
            vec![dependency_entity(static_owner_entity(*owner))],
        ),
        SemanticReadTargetV1::FunctionParameter {
            parameter,
            projection,
        } => (
            SemanticDependencyChannelV1::ParentValueFormal,
            projection.clone(),
            vec![dependency_owner(SemanticDependencyOwnerV1::Callable {
                callable: parameter.callable,
            })],
        ),
    }
}

fn event_cause_entity(cause: SemanticEventCauseV1) -> SemanticDependencyEntityV1 {
    match cause {
        SemanticEventCauseV1::Source(source) => source_entity(source),
        SemanticEventCauseV1::State(state) => state_entity(state),
        SemanticEventCauseV1::Pulse(pulse) => pulse_batch_entity(pulse),
        SemanticEventCauseV1::ExternalRead(expression) => expression_entity(expression),
    }
}

fn view_capture_target_entity(target: SemanticViewCaptureTargetV1) -> SemanticDependencyEntityV1 {
    match target {
        SemanticViewCaptureTargetV1::Read { read } => read_entity(read),
        SemanticViewCaptureTargetV1::Source { source } => source_entity(source),
        SemanticViewCaptureTargetV1::Field { field } => field_entity(field),
    }
}

fn inventory_lowering(
    lowering: &SemanticLoweringContractV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::LoweringContract,
            SemanticDependencyEntityV1::Program,
        ),
        lowering,
    )?;
    collect_dependency!(
        collector,
        SemanticDependencyOwnerV1::ProgramRoot,
        SemanticDependencyChannelV1::TypeAndFlowInstance,
        vec![
            SemanticDependencyRoleV1::FixedDefinition,
            SemanticDependencyRoleV1::AssuranceOrActivation,
        ],
        top_subject(
            SemanticDependencySubjectKindV1::LoweringMetadata,
            indexed_entity(SemanticDependencyEntityDomainV1::Diagnostic, 1),
        ),
        SemanticDependencySemanticsV1::default(),
        &lowering.metadata,
        Vec::new(),
    )?;

    for unit in &lowering.metadata.source_units {
        collector.structural(
            SemanticDependencyOwnerV1::ProgramRoot,
            top_subject(
                SemanticDependencySubjectKindV1::LoweringSourceUnit,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SourceUnit,
                    unit.id.as_usize(),
                ),
            ),
            unit,
        )?;
    }

    for source_expression in &lowering.metadata.expression_types {
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SourceExpression,
            source_expression.id.as_usize(),
        );
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::ProgramRoot,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringExpressionType,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(source_expression.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            source_expression,
            Vec::new(),
        )?;
        for (ordinal, occurrence) in source_expression.occurrences.iter().enumerate() {
            collect_dependency!(
                collector,
                owners.expression(occurrence.expression)?,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringExpressionOccurrence,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(occurrence.flow_type.clone()),
                    ..SemanticDependencySemanticsV1::default()
                },
                occurrence,
                vec![dependency_entity(expression_entity(occurrence.expression))],
            )?;
        }
    }

    for function in &lowering.metadata.function_types {
        let owner = SemanticDependencyOwnerV1::Callable {
            callable: function.callable,
        };
        let entity = callable_entity(function.callable);
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringFunctionType,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(function.result.clone()),
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            function,
            vec![dependency_entity(callable_entity(function.callable))],
        )?;
        for (ordinal, parameter) in function.parameters.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringFunctionParameter,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(parameter.flow_type.clone()),
                    visibility: SemanticDependencyVisibilityV1::Public,
                    ..SemanticDependencySemanticsV1::default()
                },
                parameter,
                Vec::new(),
            )?;
        }
    }

    for named in &lowering.metadata.named_value_types {
        let owner = owners.checked_statement(named.checked_statement)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticNamedValue,
            named.id.as_usize(),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringNamedValue,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(named.flow_type.clone()),
                ..SemanticDependencySemanticsV1::default()
            },
            named,
            vec![dependency_entity(SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedStatement,
                named.checked_statement.0,
            ))],
        )?;
        for (ordinal, origin) in named.origins.iter().enumerate() {
            let mut references = Vec::new();
            references.extend(
                origin
                    .statements
                    .iter()
                    .copied()
                    .map(statement_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .expressions
                    .iter()
                    .copied()
                    .map(expression_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .bindings
                    .iter()
                    .copied()
                    .map(binding_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .sources
                    .iter()
                    .copied()
                    .map(source_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .states
                    .iter()
                    .copied()
                    .map(state_entity)
                    .map(dependency_entity),
            );
            references.extend(
                origin
                    .lists
                    .iter()
                    .copied()
                    .map(list_entity)
                    .map(dependency_entity),
            );
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringNamedValue,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    flow_type: Some(named.flow_type.clone()),
                    ..SemanticDependencySemanticsV1::default()
                },
                origin,
                references,
            )?;
        }
    }

    for slot in &lowering.metadata.render_slots {
        let mut candidates = vec![owners.statement(slot.statement)?];
        if let Some(expression) = slot.value {
            candidates.push(owners.expression(expression)?);
        }
        let owner = exact_owner(candidates, &format!("lowering render slot {}", slot.id))?;
        let mut references = vec![dependency_entity(statement_entity(slot.statement))];
        references.extend(slot.value.map(expression_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringRenderSlot,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::RenderSlot,
                    slot.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            slot,
            references,
        )?;
    }

    for payload in &lowering.metadata.source_payload_shapes {
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SourcePayloadShape,
            payload.id.as_usize(),
        );
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::ProgramRoot,
            SemanticDependencyChannelV1::TypeAndFlowInstance,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringSourcePayload,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1::default(),
            payload,
            payload
                .sources
                .iter()
                .copied()
                .map(source_entity)
                .map(dependency_entity)
                .collect(),
        )?;
        for (ordinal, field) in payload.fields.iter().enumerate() {
            collector.structural(
                SemanticDependencyOwnerV1::ProgramRoot,
                child_subject(
                    SemanticDependencySubjectKindV1::LoweringSourcePayloadField,
                    entity.clone(),
                    ordinal,
                ),
                field,
            )?;
        }
    }

    for diagnostic in &lowering.metadata.diagnostics {
        collector.diagnostic(
            SemanticDependencyOwnerV1::ProgramRoot,
            top_subject(
                SemanticDependencySubjectKindV1::LoweringDiagnostic,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::Diagnostic,
                    diagnostic.id.as_usize(),
                ),
            ),
            diagnostic,
        )?;
    }

    for output in &lowering.output_contracts {
        let owner = exact_owner(
            vec![
                owners.statement(output.statement)?,
                owners.expression(output.expression)?,
                owners.binding(output.binding)?,
            ],
            &format!("lowering output contract {}", output.id),
        )?;
        let mut references = vec![
            dependency_entity(statement_entity(output.statement)),
            dependency_entity(expression_entity(output.expression)),
            dependency_entity(binding_entity(output.binding)),
        ];
        references.extend(output.field.map(field_entity).map(dependency_entity));
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringOutputContract,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    output.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            output,
            references,
        )?;
    }

    for port in &lowering.host_ports {
        let mut references = Vec::new();
        match &port.kind {
            SemanticHostPortKindV1::HttpServer {
                request,
                disconnect,
                response,
            } => {
                references.push(dependency_entity(source_entity(request.source)));
                references.extend(
                    disconnect
                        .as_ref()
                        .map(|binding| source_entity(binding.source))
                        .map(dependency_entity),
                );
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    response.output.as_usize(),
                )));
            }
            SemanticHostPortKindV1::WebSocketServer {
                open,
                message,
                close,
                error,
                actions,
            } => {
                references.extend(
                    [open.source, message.source, close.source, error.source]
                        .into_iter()
                        .map(source_entity)
                        .map(dependency_entity),
                );
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    actions.output.as_usize(),
                )));
            }
        }
        collect_dependency!(
            collector,
            SemanticDependencyOwnerV1::ProgramRoot,
            SemanticDependencyChannelV1::RuntimeIntrinsicOrHostEffect,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::LoweringHostPort,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::HostPort,
                    port.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            port,
            references,
        )?;
    }
    let _ = (execution, resources);
    Ok(())
}

fn inventory_view(
    view: &SemanticViewBindingGraphV1,
    execution: &SemanticExecutionGraphV1,
    reactive: &SemanticReactiveGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::ViewBindingGraph,
            SemanticDependencyEntityV1::Program,
        ),
        view,
    )?;

    for root in &view.roots {
        let owner = view_root_owner(view, root.id, execution, owners)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ViewRoot,
                view_root_entity(root.id),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            root,
            vec![
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::OutputContract,
                    root.output.as_usize(),
                )),
                dependency_entity(statement_entity(root.statement)),
                dependency_entity(expression_entity(root.expression)),
                dependency_entity(binding_entity(root.binding)),
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticScope,
                    root.route_scope.as_usize(),
                )),
            ],
        )?;
    }

    for node in &view.nodes {
        let owner = exact_owner(
            vec![
                view_root_owner(view, node.root, execution, owners)?,
                owners.expression(node.expression)?,
            ],
            &format!("view node {}", node.id),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ViewNode,
                view_node_entity(node.id),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            &(
                node.id,
                node.root,
                node.expression,
                node.value,
                node.call,
                node.callable,
            ),
            vec![
                dependency_entity(view_root_entity(node.root)),
                dependency_entity(expression_entity(node.expression)),
                dependency_entity(call_entity(node.call)),
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: node.callable,
                }),
            ],
        )?;
    }

    for argument in &view.arguments {
        let owner = exact_owner(
            vec![
                view_root_owner(view, argument.root, execution, owners)?,
                view_node_owner(view, argument.node, execution, owners)?,
                owners.expression(argument.expression)?,
            ],
            &format!("view argument {}", argument.id),
        )?;
        let channel = match argument.kind {
            SemanticViewArgumentKindV1::RenderTree => {
                SemanticDependencyChannelV1::StructuralRepresentation
            }
            SemanticViewArgumentKindV1::BindingInput => SemanticDependencyChannelV1::ResourceRead,
        };
        collect_dependency!(
            collector,
            owner,
            channel,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::ViewArgument,
                view_argument_entity(argument.id),
            ),
            SemanticDependencySemanticsV1 {
                visibility: SemanticDependencyVisibilityV1::Public,
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            &(
                argument.id,
                argument.root,
                argument.node,
                argument.call,
                argument.callable,
                argument.formal,
                argument.ordinal,
                argument.expression,
                argument.value,
                argument.kind,
            ),
            vec![
                dependency_entity(view_root_entity(argument.root)),
                dependency_entity(view_node_entity(argument.node)),
                dependency_entity(call_entity(argument.call)),
                dependency_owner(SemanticDependencyOwnerV1::Callable {
                    callable: argument.callable,
                }),
                dependency_entity(SemanticDependencyEntityV1::checked(
                    SemanticDependencyEntityDomainV1::CheckedDeclaration,
                    argument.formal.0,
                )),
                dependency_entity(expression_entity(argument.expression)),
            ],
        )?;
    }

    for binding in &view.bindings {
        let _capture = reactive
            .view_captures
            .get(binding.capture.as_usize())
            .filter(|capture| capture.id == binding.capture)
            .ok_or_else(|| {
                CallableDependencyManifestError::new(format!(
                    "view binding {} references missing reactive capture {}",
                    binding.id, binding.capture
                ))
            })?;
        let owner_candidates = vec![
            view_root_owner(view, binding.root, execution, owners)?,
            view_node_owner(view, binding.node, execution, owners)?,
            view_argument_owner(view, binding.argument, execution, owners)?,
        ];
        let target = match binding.target {
            SemanticViewBindingTargetV1::Data { read } => {
                let read = reactive
                    .reads
                    .get(read.as_usize())
                    .filter(|candidate| candidate.id == read)
                    .ok_or_else(|| {
                        CallableDependencyManifestError::new(format!(
                            "view binding {} references missing reactive read {read}",
                            binding.id
                        ))
                    })?;
                read_entity(read.id)
            }
            SemanticViewBindingTargetV1::Event { source } => source_entity(source),
        };
        let owner = exact_owner(owner_candidates, &format!("view binding {}", binding.id))?;
        let entity = view_binding_entity(binding.id);
        let mut references = vec![
            dependency_entity(view_root_entity(binding.root)),
            dependency_entity(view_node_entity(binding.node)),
            dependency_entity(view_argument_entity(binding.argument)),
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticCapture,
                binding.capture.as_usize(),
            )),
            dependency_entity(expression_entity(binding.expression)),
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticScope,
                binding.route_scope.as_usize(),
            )),
            dependency_entity(target.clone()),
        ];
        references.extend(
            binding
                .row
                .map(|row| list_entity(row.list))
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            match binding.target {
                SemanticViewBindingTargetV1::Data { .. } => {
                    SemanticDependencyChannelV1::ResourceRead
                }
                SemanticViewBindingTargetV1::Event { .. } => {
                    SemanticDependencyChannelV1::ResourceBehavior
                }
            },
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(SemanticDependencySubjectKindV1::ViewBinding, entity.clone()),
            SemanticDependencySemanticsV1 {
                row: binding.row,
                visibility: SemanticDependencyVisibilityV1::Public,
                multiplicity: if matches!(binding.target, SemanticViewBindingTargetV1::Event { .. })
                {
                    SemanticDependencyMultiplicityV1::PerEvent
                } else {
                    SemanticDependencyMultiplicityV1::PerTick
                },
                lifetime: SemanticDependencyLifetimeV1::Snapshot,
                ..SemanticDependencySemanticsV1::default()
            },
            &(
                binding.id,
                binding.root,
                binding.node,
                binding.argument,
                binding.capture,
                binding.expression,
                binding.value,
                binding.target,
                binding.route_scope,
                binding.row,
            ),
            references,
        )?;
        collect_dependency!(
            collector,
            owner,
            match binding.target {
                SemanticViewBindingTargetV1::Data { .. } => {
                    SemanticDependencyChannelV1::ResourceRead
                }
                SemanticViewBindingTargetV1::Event { .. } => {
                    SemanticDependencyChannelV1::ResourceBehavior
                }
            },
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::ViewBindingTarget,
                entity,
                0,
            ),
            SemanticDependencySemanticsV1 {
                row: binding.row,
                visibility: SemanticDependencyVisibilityV1::Public,
                ..SemanticDependencySemanticsV1::default()
            },
            &binding.target,
            vec![dependency_entity(target)],
        )?;
    }
    Ok(())
}

fn view_root_owner(
    view: &SemanticViewBindingGraphV1,
    root: SemanticViewRootId,
    _execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let root = view
        .roots
        .get(root.as_usize())
        .filter(|candidate| candidate.id == root)
        .ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references missing view root {root}"
            ))
        })?;
    // `route_scope` is a lexical definition coordinate and remains a
    // dependency reference. The retained root itself is an executable
    // occurrence, so only its statement, expression, and binding determine
    // primary ownership.
    exact_owner(
        vec![
            owners.statement(root.statement)?,
            owners.expression(root.expression)?,
            owners.binding(root.binding)?,
        ],
        &format!("view root {}", root.id),
    )
}

fn view_node_owner(
    view: &SemanticViewBindingGraphV1,
    node: SemanticViewNodeId,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let node = view
        .nodes
        .get(node.as_usize())
        .filter(|candidate| candidate.id == node)
        .ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references missing view node {node}"
            ))
        })?;
    exact_owner(
        vec![
            view_root_owner(view, node.root, execution, owners)?,
            owners.expression(node.expression)?,
        ],
        &format!("view node {}", node.id),
    )
}

fn view_argument_owner(
    view: &SemanticViewBindingGraphV1,
    argument: SemanticViewArgumentId,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
) -> Result<SemanticDependencyOwnerV1, CallableDependencyManifestError> {
    let argument = view
        .arguments
        .get(argument.as_usize())
        .filter(|candidate| candidate.id == argument)
        .ok_or_else(|| {
            CallableDependencyManifestError::new(format!(
                "dependency ownership references missing view argument {argument}"
            ))
        })?;
    exact_owner(
        vec![
            view_root_owner(view, argument.root, execution, owners)?,
            view_node_owner(view, argument.node, execution, owners)?,
            owners.expression(argument.expression)?,
        ],
        &format!("view argument {}", argument.id),
    )
}

fn inventory_storage(
    storage: &SemanticScopeStorageGraphV1,
    execution: &SemanticExecutionGraphV1,
    resources: &SemanticResourceGraphV1,
    reactive: &SemanticReactiveGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::StorageGraph,
            SemanticDependencyEntityV1::Program,
        ),
        storage,
    )?;

    for storage_owner in &storage.owners {
        let owner = owners.static_owner(storage_owner.id)?;
        let mut references = Vec::new();
        references.extend(
            storage_owner
                .parent
                .map(static_owner_entity)
                .map(dependency_entity),
        );
        references.extend(
            [
                storage_owner.source_row,
                storage_owner.target_row,
                storage_owner.authority_row,
            ]
            .into_iter()
            .flatten()
            .map(|row| list_entity(row.list))
            .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::CoverageRouting,
            vec![
                SemanticDependencyRoleV1::CoverageOrRouting,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageOwner,
                static_owner_entity(storage_owner.id),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(storage_owner.id),
                row: storage_owner
                    .authority_row
                    .or(storage_owner.target_row)
                    .or(storage_owner.source_row),
                ..SemanticDependencySemanticsV1::default()
            },
            storage_owner,
            references,
        )?;
    }

    for local in &storage.locals {
        let owner = owners.static_owner(local.owner)?;
        let entity = storage_local_entity(local.owner, local.local);
        let mut references = vec![
            dependency_entity(static_owner_entity(local.owner)),
            dependency_entity(expression_entity(local.source)),
        ];
        references.extend(
            local
                .row
                .map(|row| list_entity(row.list))
                .map(dependency_entity),
        );
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::LocalFact,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::CoverageOrRouting,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageLocal,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: Some(local.owner),
                row: local.row,
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            local,
            references,
        )?;
        for (ordinal, member) in local.members.iter().enumerate() {
            let mut references = storage_local_member_target_entities(&member.target)
                .into_iter()
                .map(dependency_entity)
                .collect::<Vec<_>>();
            if let Some(forwarding) = &member.forwarded_from {
                match forwarding {
                    SemanticStorageLocalMemberForwardingV1::Local { owner, local, .. } => {
                        references.push(dependency_entity(storage_local_entity(*owner, *local)))
                    }
                    SemanticStorageLocalMemberForwardingV1::Row { row, .. } => {
                        references.push(dependency_entity(list_entity(row.list)));
                    }
                }
            }
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::StructuralRepresentation,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::FixedDefinition,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::StorageLocalMember,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    projection: member.path.clone(),
                    static_owner: Some(local.owner),
                    row: local.row,
                    multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                    lifetime: SemanticDependencyLifetimeV1::Row,
                    ..SemanticDependencySemanticsV1::default()
                },
                member,
                references,
            )?;
        }
        for capture in &local.captures {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::LexicalCapture,
                vec![
                    SemanticDependencyRoleV1::FormulaBinder,
                    SemanticDependencyRoleV1::CoverageOrRouting,
                ],
                top_subject(
                    SemanticDependencySubjectKindV1::StorageCapture,
                    indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticStorageCapture,
                        capture.id.as_usize(),
                    ),
                ),
                SemanticDependencySemanticsV1 {
                    projection: capture.projection.clone(),
                    static_owner: Some(local.owner),
                    row: local.row,
                    multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                    lifetime: SemanticDependencyLifetimeV1::Row,
                    ..SemanticDependencySemanticsV1::default()
                },
                capture,
                vec![
                    dependency_entity(storage_local_entity(
                        capture.source_owner,
                        capture.source_local,
                    )),
                    dependency_entity(indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticStorageField,
                        capture.field.as_usize(),
                    )),
                ],
            )?;
        }
    }

    for field in &storage.fields {
        let owner = owners.storage_field(field.id)?;
        let mut references = Vec::new();
        references.extend(
            field
                .parent
                .map(|parent| {
                    indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticStorageField,
                        parent.as_usize(),
                    )
                })
                .map(dependency_entity),
        );
        references.extend(field.producer.map(expression_entity).map(dependency_entity));
        references.extend(
            field
                .reactive_field
                .map(field_entity)
                .map(dependency_entity),
        );
        match &field.origin {
            SemanticStorageFieldOriginV1::Reactive {
                field: reactive_field,
            } => references.push(dependency_entity(field_entity(*reactive_field))),
            SemanticStorageFieldOriginV1::StateAuthority { state } => {
                references.push(dependency_entity(state_entity(*state)));
            }
            SemanticStorageFieldOriginV1::ListAuthority { list, .. } => {
                references.push(dependency_entity(list_entity(*list)));
            }
            SemanticStorageFieldOriginV1::ValueListAuthority { authority, .. } => {
                references.push(dependency_entity(value_list_entity(*authority)));
            }
            SemanticStorageFieldOriginV1::RecordProjection {
                parent, expression, ..
            } => {
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    parent.as_usize(),
                )));
                references.push(dependency_entity(expression_entity(*expression)));
            }
            SemanticStorageFieldOriginV1::DetachedCapture {
                capture,
                target_owner,
                target_local,
            } => {
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageCapture,
                    capture.as_usize(),
                )));
                references.push(dependency_entity(storage_local_entity(
                    *target_owner,
                    *target_local,
                )));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageField,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    field.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(field.flow_type.clone()),
                static_owner: field.owner,
                row: field.row,
                lifetime: if field.row.is_some() {
                    SemanticDependencyLifetimeV1::Row
                } else {
                    SemanticDependencyLifetimeV1::Snapshot
                },
                ..SemanticDependencySemanticsV1::default()
            },
            field,
            references,
        )?;
    }

    for binding in &storage.bindings {
        let owner = owners.binding(binding.binding)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageBinding,
                binding_entity(binding.binding),
            ),
            SemanticDependencySemanticsV1 {
                row: storage_binding_row(&binding.target),
                ..SemanticDependencySemanticsV1::default()
            },
            binding,
            vec![
                dependency_entity(binding_entity(binding.binding)),
                dependency_entity(storage_binding_target_entity(&binding.target)),
            ],
        )?;
    }

    for source in &storage.sources {
        let owner = exact_owner(
            vec![
                owners.source(source.source)?,
                owners.binding(source.binding)?,
            ],
            &format!("storage source {}", source.source),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceBehavior,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageSource,
                source_entity(source.source),
            ),
            SemanticDependencySemanticsV1 {
                static_owner: source.owner,
                multiplicity: SemanticDependencyMultiplicityV1::PerEvent,
                lifetime: SemanticDependencyLifetimeV1::Event,
                ..SemanticDependencySemanticsV1::default()
            },
            source,
            vec![
                dependency_entity(source_entity(source.source)),
                dependency_entity(binding_entity(source.binding)),
            ],
        )?;
    }

    for (index, row_value) in storage.row_values.iter().enumerate() {
        let owner = owners.expression(row_value.expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageRowValue,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageRowValue,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: row_value.projection.clone(),
                row: Some(row_value.row),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            row_value,
            vec![
                dependency_entity(expression_entity(row_value.expression)),
                dependency_entity(list_entity(row_value.row.list)),
            ],
        )?;
    }

    for (index, projection) in storage.row_source_projections.iter().enumerate() {
        let owner = owners.source(projection.source)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ResourceRead,
            vec![
                SemanticDependencyRoleV1::FormulaBinder,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageRowSourceProjection,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageRowSourceProjection,
                    index,
                ),
            ),
            SemanticDependencySemanticsV1 {
                projection: projection.path.clone(),
                row: Some(projection.row),
                multiplicity: SemanticDependencyMultiplicityV1::PerRowMaterialization,
                lifetime: SemanticDependencyLifetimeV1::Row,
                ..SemanticDependencySemanticsV1::default()
            },
            projection,
            vec![
                dependency_entity(source_entity(projection.source)),
                dependency_entity(list_entity(projection.row.list)),
            ],
        )?;
    }

    for reference in &storage.external_references {
        let (expression, target) = match reference.kind {
            SemanticStorageExternalReferenceKindV1::Read { read, expression } => {
                (expression, read_entity(read))
            }
            SemanticStorageExternalReferenceKindV1::Call { call, expression } => {
                (expression, call_entity(call))
            }
        };
        let owner = owners.expression(expression)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::ExternalValueOrCall,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageExternalReference,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageExternalReference,
                    reference.id.as_usize(),
                ),
            ),
            SemanticDependencySemanticsV1 {
                lifetime: SemanticDependencyLifetimeV1::Activation,
                ..SemanticDependencySemanticsV1::default()
            },
            reference,
            vec![
                dependency_entity(expression_entity(expression)),
                dependency_entity(target),
            ],
        )?;
    }

    for producer in &storage.producer_result_fields {
        let owner = exact_owner(
            vec![
                owners.binding(producer.binding)?,
                owners.storage_field(producer.storage_field)?,
            ],
            &format!("producer result storage {:?}", producer.identity),
        )?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::StorageProducerResult,
                SemanticDependencyEntityV1::Digest {
                    domain: SemanticDependencyEntityDomainV1::SemanticProducerResult,
                    digest: producer.identity,
                },
            ),
            SemanticDependencySemanticsV1::default(),
            producer,
            vec![
                dependency_entity(binding_entity(producer.binding)),
                dependency_entity(field_entity(producer.reactive_field)),
                dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    producer.storage_field.as_usize(),
                )),
            ],
        )?;
    }

    for (index, named) in storage.named_values.iter().enumerate() {
        let (owner, references) = storage_named_value_owner_and_references(named, owners)?;
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![
                SemanticDependencyRoleV1::FixedDefinition,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            child_subject(
                SemanticDependencySubjectKindV1::StorageNamedValue,
                indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticNamedValue,
                    named.named_value.as_usize(),
                ),
                index,
            ),
            SemanticDependencySemanticsV1 {
                flow_type: Some(named.flow_type.clone()),
                projection: named
                    .projection
                    .iter()
                    .map(|step| step.selector.clone())
                    .collect(),
                ..SemanticDependencySemanticsV1::default()
            },
            named,
            references,
        )?;
    }
    let _ = (execution, resources, reactive);
    Ok(())
}

fn storage_local_member_target_entities(
    target: &SemanticStorageLocalMemberTargetV1,
) -> Vec<SemanticDependencyEntityV1> {
    match target {
        SemanticStorageLocalMemberTargetV1::Field(field) => vec![indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticStorageField,
            field.as_usize(),
        )],
        SemanticStorageLocalMemberTargetV1::Sources(sources) => {
            sources.iter().copied().map(source_entity).collect()
        }
        SemanticStorageLocalMemberTargetV1::State(state) => vec![state_entity(*state)],
    }
}

fn storage_binding_target_entity(
    target: &SemanticStorageBindingTargetV1,
) -> SemanticDependencyEntityV1 {
    match target {
        SemanticStorageBindingTargetV1::Value { field, .. }
        | SemanticStorageBindingTargetV1::List { field, .. } => indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticStorageField,
            field.as_usize(),
        ),
        SemanticStorageBindingTargetV1::Source { source } => source_entity(*source),
        SemanticStorageBindingTargetV1::State { state, .. } => state_entity(*state),
    }
}

fn storage_binding_row(target: &SemanticStorageBindingTargetV1) -> Option<SemanticRowBinding> {
    match target {
        SemanticStorageBindingTargetV1::Value { row, .. }
        | SemanticStorageBindingTargetV1::State { row, .. } => *row,
        SemanticStorageBindingTargetV1::List { row, .. } => Some(*row),
        SemanticStorageBindingTargetV1::Source { .. } => None,
    }
}

fn storage_named_value_owner_and_references(
    named: &SemanticNamedValueStorageV1,
    owners: &DependencyOwnerIndex,
) -> Result<
    (SemanticDependencyOwnerV1, Vec<PendingDependencyReference>),
    CallableDependencyManifestError,
> {
    let mut candidates = Vec::new();
    let mut references = Vec::new();
    match &named.target {
        SemanticNamedValueStorageTargetV1::Field { binding, field } => {
            candidates.push(owners.storage_field(*field)?);
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                field.as_usize(),
            )));
            if let Some(binding) = binding {
                candidates.push(owners.binding(*binding)?);
                references.push(dependency_entity(binding_entity(*binding)));
            }
        }
        SemanticNamedValueStorageTargetV1::Source { binding, source } => {
            candidates.push(owners.binding(*binding)?);
            candidates.push(owners.source(*source)?);
            references.push(dependency_entity(binding_entity(*binding)));
            references.push(dependency_entity(source_entity(*source)));
        }
        SemanticNamedValueStorageTargetV1::State {
            binding,
            state,
            field,
        } => {
            candidates.push(owners.binding(*binding)?);
            candidates.push(owners.state(*state)?);
            references.push(dependency_entity(binding_entity(*binding)));
            references.push(dependency_entity(state_entity(*state)));
            if let Some(field) = field {
                candidates.push(owners.storage_field(*field)?);
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    field.as_usize(),
                )));
            }
        }
        SemanticNamedValueStorageTargetV1::List {
            binding,
            list,
            field,
            ..
        } => {
            candidates.push(owners.binding(*binding)?);
            candidates.push(owners.list(*list)?);
            candidates.push(owners.storage_field(*field)?);
            references.push(dependency_entity(binding_entity(*binding)));
            references.push(dependency_entity(list_entity(*list)));
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                field.as_usize(),
            )));
        }
        SemanticNamedValueStorageTargetV1::Value {
            expression, field, ..
        } => {
            candidates.push(owners.expression(*expression)?);
            references.push(dependency_entity(expression_entity(*expression)));
            if let Some(field) = field {
                candidates.push(owners.storage_field(*field)?);
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    field.as_usize(),
                )));
            }
        }
        SemanticNamedValueStorageTargetV1::DiagnosticOnly { .. } => {
            candidates.push(SemanticDependencyOwnerV1::ProgramRoot);
        }
    }
    for step in &named.projection {
        references.extend(
            step.expression
                .map(expression_entity)
                .map(dependency_entity),
        );
        references.extend(step.storage_field.map(|field| {
            dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticStorageField,
                field.as_usize(),
            ))
        }));
    }
    Ok((
        exact_owner(
            candidates,
            &format!(
                "storage named value {} origin {} target {}",
                named.named_value, named.origin_ordinal, named.target_ordinal
            ),
        )?,
        references,
    ))
}

fn inventory_memory(
    memory: &SemanticMemoryGraphV1,
    execution: &SemanticExecutionGraphV1,
    owners: &DependencyOwnerIndex,
    collector: &mut DependencyCollector,
) -> Result<(), CallableDependencyManifestError> {
    collector.structural(
        SemanticDependencyOwnerV1::ProgramRoot,
        top_subject(
            SemanticDependencySubjectKindV1::MemoryGraph,
            SemanticDependencyEntityV1::Program,
        ),
        memory,
    )?;

    for region in &memory.memories {
        let owner = owners.memory(region.id)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMemory,
            region.id.as_usize(),
        );
        let mut references = Vec::new();
        match region.backing {
            SemanticMemoryBackingV1::State {
                binding,
                storage_field,
                state,
                ..
            } => {
                references.push(dependency_entity(binding_entity(binding)));
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    storage_field.as_usize(),
                )));
                references.push(dependency_entity(state_entity(state)));
            }
            SemanticMemoryBackingV1::List {
                binding,
                storage_field,
                list,
                ..
            } => {
                references.push(dependency_entity(binding_entity(binding)));
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticStorageField,
                    storage_field.as_usize(),
                )));
                references.push(dependency_entity(list_entity(list)));
            }
            SemanticMemoryBackingV1::Collection { expression, .. } => {
                references.push(dependency_entity(expression_entity(expression)));
            }
        }
        if let SemanticMemoryStatusV1::Draining { marker } = region.status {
            references.push(dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticMigrationInput,
                marker.as_usize(),
            )));
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::PersistenceActivation,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(SemanticDependencySubjectKindV1::Memory, entity.clone()),
            SemanticDependencySemanticsV1 {
                row: region.backing.row(),
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::PersistenceActivation,
                ..SemanticDependencySemanticsV1::default()
            },
            region,
            references,
        )?;
        for (ordinal, leaf) in region.leaves.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::TypeAndFlowInstance,
                vec![
                    SemanticDependencyRoleV1::FixedDefinition,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::MemoryLeaf,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    projection: leaf.projection.clone(),
                    row: region.backing.row(),
                    lifetime: SemanticDependencyLifetimeV1::Activation,
                    phase: SemanticDependencyPhaseV1::PersistenceActivation,
                    ..SemanticDependencySemanticsV1::default()
                },
                leaf,
                Vec::new(),
            )?;
        }
    }

    for edge in &memory.migration_edges {
        let owner = owners.memory(edge.destination.memory)?;
        let entity = indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMigrationEdge,
            edge.id.as_usize(),
        );
        let mut references = vec![dependency_entity(indexed_entity(
            SemanticDependencyEntityDomainV1::SemanticMemory,
            edge.destination.memory.as_usize(),
        ))];
        match edge.initializer {
            SemanticMigrationInitializerV1::State { state, root } => {
                references.push(dependency_entity(state_entity(state)));
                references.push(dependency_entity(expression_entity(root)));
            }
            SemanticMigrationInitializerV1::ListMaterialization {
                list,
                materialization,
                source_root,
            } => {
                references.push(dependency_entity(list_entity(list)));
                references.push(dependency_entity(indexed_entity(
                    SemanticDependencyEntityDomainV1::SemanticMaterialization,
                    materialization.as_usize(),
                )));
                references.push(dependency_entity(expression_entity(source_root)));
            }
        }
        match edge.transform {
            SemanticMigrationTransformV1::Identity { input } => {
                references.push(dependency_entity(expression_entity(input)));
            }
            SemanticMigrationTransformV1::PureExpression { root } => {
                references.push(dependency_entity(expression_entity(root)));
            }
        }
        collect_dependency!(
            collector,
            owner,
            SemanticDependencyChannelV1::PersistenceActivation,
            vec![
                SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                SemanticDependencyRoleV1::AssuranceOrActivation,
            ],
            top_subject(
                SemanticDependencySubjectKindV1::MigrationEdge,
                entity.clone(),
            ),
            SemanticDependencySemanticsV1 {
                projection: edge.destination.projection.clone(),
                multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                lifetime: SemanticDependencyLifetimeV1::Activation,
                phase: SemanticDependencyPhaseV1::PersistenceActivation,
                ..SemanticDependencySemanticsV1::default()
            },
            edge,
            references,
        )?;
        for (ordinal, input) in edge.inputs.iter().enumerate() {
            collect_dependency!(
                collector,
                owner,
                SemanticDependencyChannelV1::MigrationPredecessor,
                vec![
                    SemanticDependencyRoleV1::ResourceOrProviderBehavior,
                    SemanticDependencyRoleV1::AssuranceOrActivation,
                ],
                child_subject(
                    SemanticDependencySubjectKindV1::MigrationInput,
                    entity.clone(),
                    ordinal,
                ),
                SemanticDependencySemanticsV1 {
                    projection: input.source.projection.clone(),
                    multiplicity: SemanticDependencyMultiplicityV1::PerActivation,
                    lifetime: SemanticDependencyLifetimeV1::Activation,
                    phase: SemanticDependencyPhaseV1::PreviousCommittedValue,
                    ..SemanticDependencySemanticsV1::default()
                },
                input,
                vec![
                    dependency_entity(expression_entity(input.expression)),
                    dependency_entity(indexed_entity(
                        SemanticDependencyEntityDomainV1::SemanticMemory,
                        input.source.memory.as_usize(),
                    )),
                ],
            )?;
        }
    }
    let _ = execution;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_fixture(
        name: &str,
        source: &str,
        role: ProgramRole,
    ) -> boon_checked::CheckedProgram {
        let parsed = boon_parser::parse_source(name, source).expect("fixture parses");
        let (output, _) = boon_typecheck::check_program_profiled_with_external_types(
            &parsed,
            &boon_checked::ExternalTypeEnvironment::empty(role),
        );
        assert!(
            !output.report.has_errors(),
            "fixture diagnostics: {:#?}",
            output.report.diagnostics
        );
        output.program.expect("fixture checks")
    }

    fn semantic_program_fixture() -> SemanticProgram {
        let checked = checked_fixture(
            "dependency-manifest.bn",
            r#"
store: [
    count: 1
]

FUNCTION add(value) {
    value + 1
}
"#,
            ProgramRole::Session,
        );
        elaborate(checked, &[]).expect("dependency manifest fixture elaborates")
    }

    fn proof_manifest(program: &SemanticProgram) -> CallableDependencyProofManifestV3 {
        build_callable_dependency_proof_manifest(
            DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
            &program.checked_program,
            &program.producer_materializations,
            &program.resolved_out_graph,
            &program.execution_graph,
            &program.resource_graph,
            &program.reactive_graph,
            &program.lowering_contract,
            &program.view_binding_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        )
        .expect("dependency proof rederives")
        .manifest
    }

    fn materialize_v4_projection_proof_from_v3(
        program: &SemanticProgram,
        proof: &CallableDependencyProofManifestV3,
    ) -> ValidatedCompactDependencyCollectionV4 {
        let mut owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot]);
        owners.extend(program.execution_graph.callables.iter().map(|callable| {
            SemanticDependencyOwnerV1::Callable {
                callable: callable.id,
            }
        }));
        let stable_owners = stable_dependency_owner_index_v4(
            &program.checked_program,
            &program.execution_graph,
            &owners,
        )
        .expect("V3 oracle owners have stable V4 identities");
        let stable_projection = |owner, subject_kind, class| SemanticDependencyProjectionKeyV4 {
            owner: stable_owners[&owner],
            subject_kind,
            class,
        };
        let record_projection = |record: &SemanticDependencyRecordV1| {
            stable_projection(
                record.owner,
                record.subject.kind,
                SemanticDependencyProjectionClassV4::Dependency {
                    channel: record.channel,
                },
            )
        };

        let mut scratch = Vec::new();
        let mut receipt_hash_cache = CompactReceiptHashCacheV4::default();
        let mut coverage_rows = Vec::with_capacity(proof.coverage.len());
        let mut dependency_rows = vec![None; proof.dependencies.len()];
        let mut projection_rows =
            BTreeMap::<SemanticDependencyProjectionKeyV4, Vec<[u8; 32]>>::new();
        let mut projection_targets = BTreeMap::<
            SemanticDependencyProjectionKeyV4,
            BTreeSet<DependencyProjectionNodeV4>,
        >::new();
        for coverage in &proof.coverage {
            let (projection, local_digest, targets) = match &coverage.disposition {
                SemanticDependencyCoverageDispositionV1::Dependency { dependency } => {
                    let record = proof
                        .dependencies
                        .get(dependency.as_usize())
                        .filter(|record| record.id == *dependency)
                        .expect("V3 coverage references an exact dependency record");
                    assert_eq!(record.owner, coverage.primary_owner);
                    assert_eq!(record.subject, coverage.subject);
                    let projection = record_projection(record);
                    let local_digest = compact_local_dependency_digest_v4(
                        &record.roles,
                        &record.subject,
                        &record.semantics,
                        record.payload_digest,
                        &mut scratch,
                    )
                    .expect("V3 dependency maps to a V4 local receipt");
                    let mut targets =
                        record
                            .referenced_dependencies
                            .iter()
                            .map(|target| {
                                DependencyProjectionNodeV4::Projection(record_projection(
                                    &proof.dependencies[target.as_usize()],
                                ))
                            })
                            .chain(record.referenced_owners.iter().map(|owner| {
                                DependencyProjectionNodeV4::Owner(stable_owners[owner])
                            }))
                            .collect::<Vec<_>>();
                    targets.sort();
                    targets.dedup();
                    (projection, local_digest, targets)
                }
                SemanticDependencyCoverageDispositionV1::Structural { payload_digest } => (
                    stable_projection(
                        coverage.primary_owner,
                        coverage.subject.kind,
                        SemanticDependencyProjectionClassV4::Structural,
                    ),
                    compact_local_classification_digest_v4(
                        &coverage.subject,
                        *payload_digest,
                        &mut scratch,
                    )
                    .expect("V3 structural row maps to a V4 local receipt"),
                    Vec::new(),
                ),
                SemanticDependencyCoverageDispositionV1::Diagnostic { payload_digest } => (
                    stable_projection(
                        coverage.primary_owner,
                        coverage.subject.kind,
                        SemanticDependencyProjectionClassV4::Diagnostic,
                    ),
                    compact_local_classification_digest_v4(
                        &coverage.subject,
                        *payload_digest,
                        &mut scratch,
                    )
                    .expect("V3 diagnostic row maps to a V4 local receipt"),
                    Vec::new(),
                ),
                SemanticDependencyCoverageDispositionV1::IntentionallyNonsemantic {
                    payload_digest,
                } => (
                    stable_projection(
                        coverage.primary_owner,
                        coverage.subject.kind,
                        SemanticDependencyProjectionClassV4::IntentionallyNonsemantic,
                    ),
                    compact_local_classification_digest_v4(
                        &coverage.subject,
                        *payload_digest,
                        &mut scratch,
                    )
                    .expect("V3 nonsemantic row maps to a V4 local receipt"),
                    Vec::new(),
                ),
            };
            let row_digest = compact_projection_row_digest_v4(
                projection,
                local_digest,
                &targets,
                &mut receipt_hash_cache,
            )
            .expect("V3 row maps to a final V4 receipt");
            if let SemanticDependencyCoverageDispositionV1::Dependency { dependency } =
                coverage.disposition
            {
                dependency_rows[dependency.as_usize()] = Some(row_digest);
            }
            coverage_rows.push(row_digest);
            projection_rows
                .entry(projection)
                .or_default()
                .push(row_digest);
            projection_targets
                .entry(projection)
                .or_default()
                .extend(targets);
        }
        let dependency_rows = dependency_rows
            .into_iter()
            .map(|row| row.expect("every V3 dependency has one coverage row"))
            .collect::<Vec<_>>();
        let mut projection_receipts = BTreeMap::new();
        for (projection, rows) in projection_rows {
            projection_receipts.insert(
                projection,
                compact_projection_receipt_digest_v4(projection, &rows)
                    .expect("V3 projection maps to one V4 receipt"),
            );
        }
        let stable_owner_set = stable_owners.values().copied().collect::<BTreeSet<_>>();
        let (stable_implementation_digests, stats) = build_dependency_projection_graph_digests_v4(
            &stable_owner_set,
            &projection_receipts,
            &projection_targets,
        )
        .expect("V3 projection materializer builds the V4 proof graph");
        ValidatedCompactDependencyCollectionV4 {
            implementation_digests: stable_owners
                .iter()
                .map(|(dense, stable)| (*dense, stable_implementation_digests[stable]))
                .collect(),
            dependency_rows_digest: compact_digest_sequence_v4(
                DEPENDENCY_ROW_RECEIPT_SET_DOMAIN_V4,
                &dependency_rows,
            )
            .expect("V3 dependency receipts fold deterministically"),
            coverage_receipts_digest: compact_digest_sequence_v4(
                DEPENDENCY_COVERAGE_RECEIPT_SET_DOMAIN_V4,
                &coverage_rows,
            )
            .expect("V3 coverage receipts fold deterministically"),
            projection_receipts_digest: compact_digest_sequence_v4(
                DEPENDENCY_PROJECTION_RECEIPT_SET_DOMAIN_V4,
                &projection_receipts.values().copied().collect::<Vec<_>>(),
            )
            .expect("V3 projection receipts fold deterministically"),
            dependency_record_count: dependency_rows.len(),
            coverage_record_count: coverage_rows.len(),
            projection_count: projection_receipts.len(),
            projection_edge_count: stats.edges,
        }
    }

    fn manifest_record(
        manifest: &CallableDependencyProofManifestV3,
        kind: SemanticDependencySubjectKindV1,
        identity: SemanticDependencyEntityV1,
    ) -> &SemanticDependencyRecordV1 {
        let matches = manifest
            .dependencies
            .iter()
            .filter(|record| record.subject.kind == kind && record.subject.identity == identity)
            .collect::<Vec<_>>();
        let [record] = matches.as_slice() else {
            panic!(
                "manifest subject {kind:?}/{identity:?} resolves to {} records",
                matches.len()
            );
        };
        record
    }

    fn assert_manifest_mutation_rejected(
        program: &SemanticProgram,
        context: &str,
        mutate: impl FnOnce(&mut CallableDependencyManifestV4),
    ) {
        let mut manifest = program.dependency_manifest.clone();
        mutate(&mut manifest);
        let error = match manifest.validate_against(
            DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
            &program.checked_program,
            &program.producer_materializations,
            &program.resolved_out_graph,
            &program.execution_graph,
            &program.resource_graph,
            &program.reactive_graph,
            &program.lowering_contract,
            &program.view_binding_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        ) {
            Ok(()) => panic!("{context} mutation must be rejected"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("differs from its deterministic checked+semantic rederivation"),
            "{context}: {error}"
        );
    }

    fn owner(callable: usize) -> SemanticDependencyOwnerV1 {
        SemanticDependencyOwnerV1::Callable {
            callable: SemanticCallableId(callable),
        }
    }

    fn test_subject(index: usize) -> SemanticDependencySubjectV1 {
        top_subject(
            SemanticDependencySubjectKindV1::ExecutionExpression,
            indexed_entity(SemanticDependencyEntityDomainV1::SemanticExpression, index),
        )
    }

    fn test_dependency_graph_digests(
        owners: &BTreeSet<SemanticDependencyOwnerV1>,
        records: &[SemanticDependencyRecordV1],
        direct: &BTreeMap<SemanticDependencyOwnerV1, Vec<SemanticDependencyRecordId>>,
    ) -> DependencyGraphDigestMap {
        let owner_list = owners.iter().copied().collect::<Vec<_>>();
        let graph = DependencyProofGraph::new(&owner_list, records, direct)
            .expect("reference dependency proof graph");
        let (component_by_node, components) =
            dependency_proof_components(&graph).expect("reference proof components");
        let (reference_hashers, streamed_record_set) =
            reference_dependency_proof_component_local_hashers(
                &graph,
                &components,
                &component_by_node,
            )
            .expect("streamed component prefixes");
        assert_eq!(
            streamed_record_set,
            reference_dependency_record_set_digest(records).expect("reference record set")
        );
        let representatives = components
            .iter()
            .map(|members| members[0])
            .collect::<Vec<_>>();
        let (outgoing, graph_edge_count) = build_dependency_proof_outgoing_csr(
            &graph,
            &component_by_node,
            components.len(),
            &representatives,
        )
        .expect("flat outgoing component graph");
        let mut reference_outgoing = vec![Vec::new(); components.len()];
        let node_count = graph.node_count().expect("reference graph node count");
        let mut reference_graph_edge_count = 0usize;
        for source in 0..node_count {
            let degree = graph.out_degree(source).expect("reference source degree");
            reference_graph_edge_count += degree;
            for edge in 0..degree {
                let target = graph
                    .edge_target(source, edge)
                    .expect("reference edge target");
                let source_component = component_by_node[source];
                let target_component = component_by_node[target];
                if source_component != target_component {
                    reference_outgoing[source_component].push(target_component);
                }
            }
        }
        for dependencies in &mut reference_outgoing {
            dependencies.sort_unstable_by_key(|component| representatives[*component]);
            dependencies.dedup();
        }
        assert_eq!(graph_edge_count, reference_graph_edge_count);
        assert_eq!(outgoing.edge_offsets.len(), components.len() + 1);
        assert_eq!(outgoing.edge_offsets[0], 0);
        assert_eq!(
            outgoing.edge_offsets[components.len()],
            outgoing.edge_count()
        );
        for (component, reference) in reference_outgoing.iter().enumerate() {
            assert_eq!(
                outgoing.edges(component).expect("flat outgoing slice"),
                reference,
                "flat outgoing CSR changed canonical component edge order"
            );
        }
        let parents = outgoing.reverse().expect("flat reverse component graph");
        let mut reference_parents = vec![Vec::new(); components.len()];
        for (component, dependencies) in reference_outgoing.iter().enumerate() {
            for dependency in dependencies {
                reference_parents[*dependency].push(component);
            }
        }
        for (component, reference) in reference_parents.iter().enumerate() {
            assert_eq!(
                parents.edges(component).expect("flat parent slice"),
                reference,
                "flat reverse CSR changed ready-queue parent order"
            );
        }
        let mut scratch = Vec::new();
        let reference_leaves = records
            .iter()
            .map(|record| {
                canonical_dependency_hash_with_buffer(
                    DEPENDENCY_RECORD_DIGEST_DOMAIN,
                    record,
                    &mut scratch,
                )
                .expect("reference dependency leaf")
            })
            .collect::<Vec<_>>();
        let (record_leaves, dense_record_set) =
            dependency_proof_record_leaf_arena(&graph).expect("dense record-leaf arena");
        assert_eq!(dense_record_set, streamed_record_set);
        assert_eq!(record_leaves, reference_leaves);
        for (component, members) in components.iter().enumerate() {
            let mut reference = Sha256::new();
            reference.update(DEPENDENCY_IMPLEMENTATION_COMPONENT_DOMAIN);
            dependency_proof_update_usize(
                &mut reference,
                members.len(),
                "reference component member count",
            )
            .expect("reference member count");
            for node in members.iter().copied() {
                let identity = graph.node_identity(node).expect("reference node identity");
                dependency_proof_update_node(&mut reference, identity)
                    .expect("reference node encoding");
                match identity {
                    DependencyProofNodeV3::Owner { owner } => {
                        let ordinal = graph.owner_ordinals[&owner];
                        reference.update(
                            dependency_proof_owner_digest(owner, graph.direct_by_owner[ordinal])
                                .expect("reference owner digest"),
                        );
                    }
                    DependencyProofNodeV3::Record { record } => {
                        reference.update(reference_leaves[record.as_usize()]);
                    }
                }
            }
            assert_eq!(
                reference_hashers[component].clone().finalize(),
                reference.finalize(),
                "streamed component prefix changed the V3 proof bytes"
            );
        }
        let production_component_digests = dependency_proof_component_digests(
            &graph,
            &components,
            &record_leaves,
            &outgoing,
            &parents,
            &representatives,
        )
        .expect("dense-leaf component digests");
        let reference_component_digests = reference_dependency_proof_component_digests(
            &graph,
            &components,
            reference_hashers,
            &outgoing,
            &parents,
            &representatives,
        )
        .expect("reference component digests");
        assert_eq!(
            production_component_digests, reference_component_digests,
            "dense record leaves changed a component commitment"
        );
        let production_owner_digests = dependency_proof_owner_implementation_digests(
            &graph,
            &component_by_node,
            &representatives,
            &production_component_digests,
        )
        .expect("dense-leaf owner digests");
        let reference_owner_digests = dependency_proof_owner_implementation_digests(
            &graph,
            &component_by_node,
            &representatives,
            &reference_component_digests,
        )
        .expect("reference owner digests");
        assert_eq!(
            production_owner_digests, reference_owner_digests,
            "dense record leaves changed an owner implementation digest"
        );
        let (built_owner_digests, _, built_record_set) =
            build_dependency_graph_digests(&owner_list, records, direct)
                .expect("test dependency graph proof");
        assert_eq!(built_record_set, dense_record_set);
        assert_eq!(built_owner_digests, production_owner_digests);
        built_owner_digests
    }

    #[test]
    fn graph_proof_follows_exact_owner_and_entity_edges() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let caller = owner(0);
        let callee = owner(1);
        let owners = BTreeSet::from([root, caller, callee]);
        let callee_entity = indexed_entity(SemanticDependencyEntityDomainV1::SemanticExpression, 0);
        let mut collector = DependencyCollector::exhaustive_for_test();
        let callee_dependency = collect_dependency!(
            collector,
            callee,
            SemanticDependencyChannelV1::LocalFact,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"callee",
            Vec::new(),
        )
        .expect("callee dependency");
        let caller_dependency = collect_dependency!(
            collector,
            caller,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(1),
            SemanticDependencySemanticsV1::default(),
            &"caller",
            vec![dependency_owner(callee), dependency_entity(callee_entity)],
        )
        .expect("caller dependency");
        let ValidatedDependencyCollection {
            dependencies: records,
            coverage,
            direct,
        } = collector.finish(&owners).expect("collector finish");
        let original = test_dependency_graph_digests(&owners, &records, &direct);

        assert_eq!(coverage.len(), 2);
        assert_eq!(direct[&caller], vec![caller_dependency]);
        assert_eq!(direct[&callee], vec![callee_dependency]);
        let mut mutated = records;
        mutated[callee_dependency.as_usize()].payload_digest[0] ^= 1;
        let changed = test_dependency_graph_digests(&owners, &mutated, &direct);
        assert_ne!(
            original[&caller], changed[&caller],
            "caller proof must include both exact entity and callable-owner dependencies"
        );
        assert_ne!(original[&callee], changed[&callee]);
        assert_eq!(original[&root], changed[&root]);
    }

    #[test]
    fn collector_resolves_partitioned_references_without_changing_record_order() {
        let caller = owner(0);
        let callee = owner(1);
        let owners = BTreeSet::from([SemanticDependencyOwnerV1::ProgramRoot, caller, callee]);
        let target_subject = test_subject(0);
        let target_entity = target_subject.identity.clone();
        let source_subject = test_subject(1);
        let source_entity = source_subject.identity.clone();
        let mut collector = DependencyCollector::exhaustive_for_test();
        let target = collect_dependency!(
            collector,
            callee,
            SemanticDependencyChannelV1::LocalFact,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            target_subject,
            SemanticDependencySemanticsV1::default(),
            &"target",
            Vec::new(),
        )
        .expect("target dependency");
        let source = collect_dependency!(
            collector,
            caller,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            source_subject,
            SemanticDependencySemanticsV1::default(),
            &"source",
            vec![
                dependency_owner(caller),
                dependency_owner(callee),
                dependency_owner(callee),
                dependency_entity(source_entity),
                dependency_entity(target_entity.clone()),
                dependency_entity(target_entity),
            ],
        )
        .expect("source dependency");

        let ValidatedDependencyCollection {
            dependencies: records,
            coverage,
            direct,
        } = collector.finish(&owners).expect("collector finish");

        assert_eq!(target, SemanticDependencyRecordId(0));
        assert_eq!(source, SemanticDependencyRecordId(1));
        assert_eq!(
            records[source.as_usize()].referenced_dependencies,
            vec![target]
        );
        assert_eq!(records[source.as_usize()].referenced_owners, vec![callee]);
        assert_eq!(coverage.len(), 2);
        assert_eq!(direct[&caller], vec![source]);
        assert_eq!(direct[&callee], vec![target]);
    }

    #[test]
    fn duplicate_subject_classification_is_rejected() {
        let mut collector = DependencyCollector::exhaustive_for_test();
        let subject = test_subject(0);
        collector
            .structural(
                SemanticDependencyOwnerV1::ProgramRoot,
                subject.clone(),
                &"first",
            )
            .expect("first classification");
        let error = collector
            .diagnostic(SemanticDependencyOwnerV1::ProgramRoot, subject, &"second")
            .expect_err("duplicate subject must fail");
        assert!(error.to_string().contains("classified more than once"));
    }

    #[test]
    fn collector_rejects_unregistered_coverage_owner_before_commitment() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        collector
            .structural(owner(99), test_subject(0), &"unregistered coverage")
            .expect("coverage is inventoried before owners are finalized");

        let error = collector
            .finish(&owners)
            .expect_err("coverage owned outside the canonical owner set must fail");
        assert!(error.to_string().contains("references missing owner"));
    }

    #[test]
    fn collector_rejects_dependency_coverage_disagreement_before_commitment() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::LocalFact,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"dependency",
            Vec::new(),
        )
        .expect("dependency coverage");
        collector.coverage[0].subject = test_subject(1);

        let error = collector
            .finish(&owners)
            .expect_err("dependency coverage must still agree with its exact record");
        assert!(error.to_string().contains("disagrees with dependency"));
    }

    #[test]
    fn unresolved_entity_reference_is_rejected() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![SemanticDependencyRoleV1::AssuranceOrActivation],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"dependent",
            vec![dependency_entity(indexed_entity(
                SemanticDependencyEntityDomainV1::SemanticMemory,
                99,
            ))],
        )
        .expect("dependency is collected before references resolve");
        let error = collector
            .finish(&owners)
            .expect_err("unresolved exact entity must fail");
        assert!(error.to_string().contains("no dependency classification"));
    }

    #[test]
    fn partitioned_reference_resolution_preserves_error_order() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let missing_owner = owner(99);
        let missing_entity = indexed_entity(SemanticDependencyEntityDomainV1::SemanticMemory, 99);

        let mut owner_first = DependencyCollector::exhaustive_for_test();
        collect_dependency!(
            owner_first,
            root,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![SemanticDependencyRoleV1::AssuranceOrActivation],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"owner first",
            vec![
                dependency_owner(missing_owner),
                dependency_entity(missing_entity.clone()),
            ],
        )
        .expect("dependency is collected before references resolve");
        let owner_error = owner_first
            .finish(&owners)
            .expect_err("the first missing owner must surface first");
        assert!(owner_error.to_string().contains("references missing owner"));

        let mut entity_first = DependencyCollector::exhaustive_for_test();
        collect_dependency!(
            entity_first,
            root,
            SemanticDependencyChannelV1::AssuranceInput,
            vec![SemanticDependencyRoleV1::AssuranceOrActivation],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"entity first",
            vec![
                dependency_entity(missing_entity),
                dependency_owner(missing_owner),
            ],
        )
        .expect("dependency is collected before references resolve");
        let entity_error = entity_first
            .finish(&owners)
            .expect_err("the first missing entity must surface first");
        assert!(
            entity_error
                .to_string()
                .contains("no dependency classification")
        );
    }

    #[test]
    fn mixed_primary_owners_require_an_engine_identity() {
        let error = exact_owner(
            vec![SemanticDependencyOwnerV1::ProgramRoot, owner(0)],
            "mixed fixture",
        )
        .expect_err("mixed ownership must fail");
        assert!(error.to_string().contains("explicit engine identity"));
    }

    #[test]
    fn implementation_digest_detects_dependency_payload_mutation() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let owners = BTreeSet::from([root]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"stable",
            Vec::new(),
        )
        .expect("dependency");
        let ValidatedDependencyCollection {
            dependencies: records,
            direct,
            ..
        } = collector.finish(&owners).expect("collector finish");
        let original = test_dependency_graph_digests(&owners, &records, &direct);
        let mut mutated = records;
        mutated[0].payload_digest[0] ^= 1;
        let changed = test_dependency_graph_digests(&owners, &mutated, &direct);
        assert_ne!(original[&root], changed[&root]);
    }

    #[test]
    fn implementation_digest_covers_only_exact_reachable_components() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let isolated = owner(0);
        let owners = BTreeSet::from([root, isolated]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        let root_dependency = collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"root",
            Vec::new(),
        )
        .expect("root dependency");
        let isolated_dependency = collect_dependency!(
            collector,
            isolated,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(1),
            SemanticDependencySemanticsV1::default(),
            &"isolated",
            Vec::new(),
        )
        .expect("isolated dependency");
        let ValidatedDependencyCollection {
            dependencies: records,
            direct,
            ..
        } = collector.finish(&owners).expect("collector finish");
        let original = test_dependency_graph_digests(&owners, &records, &direct);

        let mut outside_mutation = records.clone();
        outside_mutation[isolated_dependency.as_usize()].payload_digest[0] ^= 1;
        let outside = test_dependency_graph_digests(&owners, &outside_mutation, &direct);
        assert_eq!(
            original[&root], outside[&root],
            "a dependency outside the exact reachable graph must not invalidate the owner"
        );
        assert_ne!(original[&isolated], outside[&isolated]);

        let mut inside_mutation = records;
        inside_mutation[root_dependency.as_usize()].payload_digest[0] ^= 1;
        let inside = test_dependency_graph_digests(&owners, &inside_mutation, &direct);
        assert_ne!(
            original[&root], inside[&root],
            "a reachable dependency must invalidate the owner"
        );
    }

    #[test]
    fn implementation_digest_handles_owner_cycles_once() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let first = owner(0);
        let second = owner(1);
        let owners = BTreeSet::from([root, first, second]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        collect_dependency!(
            collector,
            first,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"first",
            vec![dependency_owner(second)],
        )
        .expect("first dependency");
        let second_dependency = collect_dependency!(
            collector,
            second,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(1),
            SemanticDependencySemanticsV1::default(),
            &"second",
            vec![dependency_owner(first)],
        )
        .expect("second dependency");
        let ValidatedDependencyCollection {
            dependencies: records,
            direct,
            ..
        } = collector.finish(&owners).expect("collector finish");
        let original = test_dependency_graph_digests(&owners, &records, &direct);
        let mut mutated = records;
        mutated[second_dependency.as_usize()].payload_digest[0] ^= 1;
        let changed = test_dependency_graph_digests(&owners, &mutated, &direct);
        assert_ne!(original[&first], changed[&first]);
        assert_ne!(original[&second], changed[&second]);
        assert_eq!(original[&root], changed[&root]);
    }

    #[test]
    fn dependency_proof_components_follow_existing_forward_edges() {
        let root = SemanticDependencyOwnerV1::ProgramRoot;
        let first = owner(0);
        let second = owner(1);
        let isolated = owner(2);
        let owners = BTreeSet::from([root, first, second, isolated]);
        let mut collector = DependencyCollector::exhaustive_for_test();
        let root_dependency = collect_dependency!(
            collector,
            root,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(0),
            SemanticDependencySemanticsV1::default(),
            &"root",
            vec![dependency_owner(first)],
        )
        .expect("root dependency");
        let first_dependency = collect_dependency!(
            collector,
            first,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(1),
            SemanticDependencySemanticsV1::default(),
            &"first",
            vec![dependency_owner(second)],
        )
        .expect("first dependency");
        let second_dependency = collect_dependency!(
            collector,
            second,
            SemanticDependencyChannelV1::CalledCallable,
            vec![SemanticDependencyRoleV1::ResourceOrProviderBehavior],
            test_subject(2),
            SemanticDependencySemanticsV1::default(),
            &"second",
            vec![dependency_owner(first)],
        )
        .expect("second dependency");
        let isolated_dependency = collect_dependency!(
            collector,
            isolated,
            SemanticDependencyChannelV1::StructuralRepresentation,
            vec![SemanticDependencyRoleV1::FixedDefinition],
            test_subject(3),
            SemanticDependencySemanticsV1::default(),
            &"isolated",
            Vec::new(),
        )
        .expect("isolated dependency");
        let ValidatedDependencyCollection {
            dependencies: records,
            direct,
            ..
        } = collector.finish(&owners).expect("collector finish");
        let owner_list = owners.iter().copied().collect::<Vec<_>>();
        let graph =
            DependencyProofGraph::new(&owner_list, &records, &direct).expect("dependency graph");
        let (component_by_node, components) =
            dependency_proof_components(&graph).expect("dependency components");

        let root_node = graph.owner_ordinals[&root];
        let first_node = graph.owner_ordinals[&first];
        let second_node = graph.owner_ordinals[&second];
        let isolated_node = graph.owner_ordinals[&isolated];
        let root_record = graph.record_node(root_dependency).expect("root record");
        let first_record = graph.record_node(first_dependency).expect("first record");
        let second_record = graph.record_node(second_dependency).expect("second record");
        let isolated_record = graph
            .record_node(isolated_dependency)
            .expect("isolated record");

        assert_eq!(
            component_by_node[first_node],
            component_by_node[first_record]
        );
        assert_eq!(
            component_by_node[first_node],
            component_by_node[second_node]
        );
        assert_eq!(
            component_by_node[first_node],
            component_by_node[second_record]
        );
        assert_ne!(component_by_node[root_node], component_by_node[root_record]);
        assert_ne!(
            component_by_node[root_record],
            component_by_node[first_node]
        );
        assert_ne!(
            component_by_node[isolated_node],
            component_by_node[isolated_record]
        );
        assert_eq!(components.len(), 5);
        assert_eq!(components.member_offsets.len(), components.len() + 1);
        assert_eq!(components.member_arena.len(), 8);
        assert_eq!(components.iter().map(<[usize]>::len).sum::<usize>(), 8);
        assert!(
            components
                .iter()
                .all(|members| members.windows(2).all(|pair| pair[0] < pair[1])),
            "component members remain canonical for proof hashing"
        );
    }

    #[test]
    fn contextual_occurrence_is_owned_by_its_concrete_root_and_links_its_definition() {
        let checked = checked_fixture(
            "dependency-contextual-owner.bn",
            r#"
FUNCTION doubled(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new * 2
    )
}

rows: LIST { [value: 1] }
result:
    rows
    |> doubled(
        entry
        new: entry.value + 1
    )
"#,
            ProgramRole::Client,
        );
        let program = elaborate(checked, &[]).expect("contextual fixture elaborates");
        let proof_manifest = proof_manifest(&program);
        let materialization = program
            .execution_graph
            .materializations
            .first()
            .expect("fixture has one materialization");
        let expression = &program.execution_graph.expressions[materialization.body.as_usize()];
        let origin = &program.execution_graph.checked_expression_origins[expression.id.as_usize()];
        let occurrence = manifest_record(
            &proof_manifest,
            SemanticDependencySubjectKindV1::ExecutionExpression,
            expression_entity(expression.id),
        );
        assert_eq!(
            occurrence.owner,
            SemanticDependencyOwnerV1::ProgramRoot,
            "expanded wrapper work belongs to its ordinary concrete call root"
        );
        let definition = manifest_record(
            &proof_manifest,
            SemanticDependencySubjectKindV1::CheckedExpression,
            SemanticDependencyEntityV1::checked(
                SemanticDependencyEntityDomainV1::CheckedExpression,
                expression.checked_expr_id.0,
            ),
        );
        assert_eq!(definition.owner, owner(0));
        assert!(occurrence.referenced_dependencies.contains(&definition.id));

        let static_owner = expression.owner.expect("body has exact static owner");
        let static_record = manifest_record(
            &proof_manifest,
            SemanticDependencySubjectKindV1::ExecutionStaticOwner,
            static_owner_entity(static_owner),
        );
        assert!(
            occurrence
                .referenced_dependencies
                .contains(&static_record.id)
        );
        let frame = origin.call_instance.expect("body has exact call frame");
        let frame_record = manifest_record(
            &proof_manifest,
            SemanticDependencySubjectKindV1::OutCallInstance,
            out_call_entity(frame),
        );
        assert!(
            occurrence
                .referenced_dependencies
                .contains(&frame_record.id)
        );
    }

    #[test]
    fn producer_occurrence_owner_uses_the_synthetic_root_callable_and_fails_on_axis_drift() {
        let checked = checked_fixture(
            "dependency-producer-owner.bn",
            r#"
FUNCTION serve(value) {
    value + 0
}

ordinary: serve(value: 1)
"#,
            ProgramRole::Session,
        );
        let callable = SemanticCallableId(
            checked
                .callables
                .iter()
                .position(|callable| callable.name == "serve")
                .expect("serve callable"),
        );
        let program = elaborate(
            checked,
            &[ProducerMaterializationRequest {
                identity: [7; 32],
                callable,
                local_function: "serve".to_owned(),
                mode: ProducerMaterializationMode::Current,
            }],
        )
        .expect("producer fixture elaborates");
        let proof_manifest = proof_manifest(&program);
        let function = program
            .execution_graph
            .functions
            .first()
            .expect("fixture has one producer function");
        assert_eq!(function.callable, callable);
        let occurrence = manifest_record(
            &proof_manifest,
            SemanticDependencySubjectKindV1::ExecutionExpression,
            expression_entity(function.root),
        );
        assert_eq!(occurrence.owner, owner(callable.as_usize()));
        let producer_root = program
            .resolved_out_graph
            .producer_roots()
            .first()
            .expect("fixture has one producer root");
        let root_record = manifest_record(
            &proof_manifest,
            SemanticDependencySubjectKindV1::OutCallInstance,
            out_call_entity(producer_root.call),
        );
        assert_eq!(root_record.owner, owner(callable.as_usize()));

        let mut frame_only = program.execution_graph.clone();
        frame_only.expressions[function.root.as_usize()].owner = None;
        let frame_only_index = DependencyOwnerIndex::derive(
            &program.checked_program,
            &program.resolved_out_graph,
            &frame_only,
            &program.resource_graph,
            &program.reactive_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        )
        .expect("call frame alone retains the producer occurrence owner");
        assert_eq!(
            frame_only_index
                .expression(function.root)
                .expect("producer expression owner"),
            owner(callable.as_usize())
        );

        let ordinary_root = program
            .resolved_out_graph
            .call_instances
            .iter()
            .find(|call| call.parent.is_none() && call.id != producer_root.call)
            .expect("fixture has an ordinary program-root call")
            .id;
        let mut conflicting = program.execution_graph.clone();
        conflicting.checked_expression_origins[function.root.as_usize()].call_instance =
            Some(ordinary_root);
        let error = DependencyOwnerIndex::derive(
            &program.checked_program,
            &program.resolved_out_graph,
            &conflicting,
            &program.resource_graph,
            &program.reactive_graph,
            &program.scope_storage_graph,
            &program.memory_graph,
        )
        .expect_err("static and call-root occurrence axes must agree");
        assert!(error.to_string().contains("conflicting static"), "{error}");
    }

    #[test]
    fn validate_against_rejects_every_component_digest_mutation() {
        let program = semantic_program_fixture();
        macro_rules! reject_component {
            ($field:ident) => {
                assert_manifest_mutation_rejected(&program, stringify!($field), |manifest| {
                    manifest.component_digests.$field[0] ^= 1
                });
            };
        }
        reject_component!(producer_materializations);
        reject_component!(resolved_out_graph);
        reject_component!(execution_graph);
        reject_component!(resource_graph);
        reject_component!(reactive_graph);
        reject_component!(lowering_contract);
        reject_component!(view_binding_graph);
        reject_component!(scope_storage_graph);
        reject_component!(memory_graph);
    }

    #[test]
    fn validate_against_rejects_callable_implementation_digest_mutation() {
        let program = semantic_program_fixture();
        assert_manifest_mutation_rejected(&program, "callable implementation digest", |manifest| {
            let callable = manifest
                .callable_entries
                .first_mut()
                .expect("fixture has a callable dependency entry");
            callable.implementation_dependency_digest[0] ^= 1;
        });
    }

    #[test]
    fn v4_projection_seal_matches_the_independent_v3_inventory_counts() {
        let program = semantic_program_fixture();
        let proof = proof_manifest(&program);
        let sealed = program.dependency_manifest();
        let materialized = materialize_v4_projection_proof_from_v3(&program, &proof);

        assert_eq!(sealed.schema, CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V4);
        assert_eq!(proof.schema, CALLABLE_DEPENDENCY_MANIFEST_SCHEMA_V3);
        assert_eq!(sealed.checked_program_digest, proof.checked_program_digest);
        assert_eq!(sealed.component_digests, proof.component_digests);
        assert_eq!(
            sealed.proof_digests.dependency_record_count,
            proof.dependencies.len()
        );
        assert_eq!(
            sealed.proof_digests.coverage_record_count,
            proof.coverage.len()
        );
        assert!(
            sealed.proof_digests.projection_count < sealed.proof_digests.coverage_record_count,
            "V4 must fold exhaustive rows into fewer owner/projection requests"
        );
        assert_eq!(sealed.callable_entries.len(), proof.callable_entries.len());
        assert_eq!(
            sealed.proof_digests.dependency_rows_digest,
            materialized.dependency_rows_digest
        );
        assert_eq!(
            sealed.proof_digests.coverage_receipts_digest,
            materialized.coverage_receipts_digest
        );
        assert_eq!(
            sealed.proof_digests.projection_receipts_digest,
            materialized.projection_receipts_digest
        );
        assert_eq!(
            sealed.proof_digests.projection_count,
            materialized.projection_count
        );
        assert_eq!(
            sealed.proof_digests.projection_edge_count,
            materialized.projection_edge_count
        );
        assert_eq!(
            sealed.program_root.implementation_dependency_digest,
            materialized.implementation_digests[&SemanticDependencyOwnerV1::ProgramRoot]
        );
        for entry in &sealed.callable_entries {
            assert_eq!(
                entry.implementation_dependency_digest,
                materialized.implementation_digests[&SemanticDependencyOwnerV1::Callable {
                    callable: entry.callable,
                }]
            );
        }
        sealed
            .validate_against(
                DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
                &program.checked_program,
                &program.producer_materializations,
                &program.resolved_out_graph,
                &program.execution_graph,
                &program.resource_graph,
                &program.reactive_graph,
                &program.lowering_contract,
                &program.view_binding_graph,
                &program.scope_storage_graph,
                &program.memory_graph,
            )
            .expect("V4 projection receipts rederive deterministically");
    }

    #[test]
    fn exhaustive_coverage_inventory_is_checked_before_sealing() {
        let program = semantic_program_fixture();
        let mut proof = proof_manifest(&program);
        proof
            .coverage
            .pop()
            .expect("fixture has exhaustive coverage records");
        proof.manifest_digest =
            callable_dependency_manifest_digest(&proof).expect("rebound proof manifest digest");

        let error = proof
            .validate_against(
                DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
                &program.checked_program,
                &program.producer_materializations,
                &program.resolved_out_graph,
                &program.execution_graph,
                &program.resource_graph,
                &program.reactive_graph,
                &program.lowering_contract,
                &program.view_binding_graph,
                &program.scope_storage_graph,
                &program.memory_graph,
            )
            .expect_err("incomplete proof coverage must be rejected before sealing");
        assert!(
            error
                .to_string()
                .contains("deterministic checked+semantic rederivation"),
            "{error}"
        );
    }

    #[test]
    fn exhaustive_proof_is_validated_before_sealing() {
        let program = semantic_program_fixture();
        let mut proof = proof_manifest(&program);
        let callable = proof
            .callable_entries
            .first_mut()
            .expect("fixture has a callable dependency entry");
        callable.implementation_dependency_digest[0] ^= 1;
        proof.manifest_digest =
            callable_dependency_manifest_digest(&proof).expect("mutated proof manifest digest");

        let error = proof
            .validate_integrity(
                DEPENDENCY_CLASSIFIER_SCHEMA_DIGEST_V1,
                &program.checked_program,
                &program.execution_graph,
            )
            .expect_err("rebound canonical payload cannot hide a stale dependency graph proof");
        assert!(
            error.to_string().contains("exact dependency graph proof"),
            "{error}"
        );
    }
}
