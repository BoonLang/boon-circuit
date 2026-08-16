use crate::{
    BytesType, CheckedExprId, CheckedExpression, FlowMode, FlowType, ObjectShape, ProgramRole,
    Type, TypeVar, Variant,
};
use boon_contract::SourceBundleDigestV1;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

pub const ARTIFACT_TYPE_MODULE_SCHEMA_V1: &str = "boon.artifact-type-module.v1";

const ARTIFACT_TYPE_TERM_DOMAIN_V1: &[u8] = b"boon.artifact-type-term.v1\0";
const ARTIFACT_FLOW_TERM_DOMAIN_V1: &[u8] = b"boon.artifact-flow-term.v1\0";
const ARTIFACT_TYPE_MODULE_DOMAIN_V1: &[u8] = b"boon.artifact-type-module.v1\0";
const CHECKED_RUNTIME_FLOW_TERM_PROJECTION_DOMAIN_V1: &[u8] =
    b"boon.checked-runtime-flow-term-projection.v1\0";
const CHECKED_RUNTIME_FLOW_TERM_HANDOFF_DOMAIN_V1: &[u8] =
    b"boon.checked-runtime-flow-term-handoff.v1\0";

pub const CHECKED_RUNTIME_FLOW_TERM_HANDOFF_SCHEMA_V1: &str =
    "boon.checked-runtime-flow-term-handoff.v1";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ArtifactTypeTermIdV1(pub u32);

impl ArtifactTypeTermIdV1 {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum ArtifactBytesTermV1 {
    Dynamic,
    Fixed(u64),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ArtifactObjectFieldTermV1 {
    pub name: String,
    pub ty: ArtifactTypeTermIdV1,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum ArtifactVariantTermV1 {
    Tag(String),
    Tagged {
        tag: String,
        fields: ArtifactTypeTermIdV1,
    },
}

/// Exact immutable public type node used after solver quiescence.
///
/// This is deliberately distinct from the inference kernel's `TypeTerm`:
/// unions preserve order and duplicates, variables are module-local alpha
/// ordinals, and inference-only placeholders cannot cross this boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactTypeTermKindV1 {
    Text,
    Number,
    Bytes {
        value: ArtifactBytesTermV1,
    },
    Absent,
    VariantSet {
        variants: Box<[ArtifactVariantTermV1]>,
    },
    Object {
        fields: Box<[ArtifactObjectFieldTermV1]>,
        field_order: Box<[String]>,
        open: bool,
    },
    RenderContract,
    List {
        item: ArtifactTypeTermIdV1,
    },
    Function {
        args: Box<[ArtifactTypeTermIdV1]>,
        result_mode: FlowMode,
        result: ArtifactTypeTermIdV1,
    },
    UnresolvedShape {
        reason: String,
    },
    Variable {
        ordinal: u32,
    },
    Unknown,
    /// Ordered public union. Do not flatten or deduplicate this variant.
    Union {
        members: Box<[ArtifactTypeTermIdV1]>,
    },
    Map {
        key: ArtifactTypeTermIdV1,
        value: ArtifactTypeTermIdV1,
    },
    Set {
        item: ArtifactTypeTermIdV1,
    },
    Bits {
        width: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactTypeTermV1 {
    pub id: ArtifactTypeTermIdV1,
    pub kind: ArtifactTypeTermKindV1,
    pub stable_digest: [u8; 32],
    pub contains_variable: bool,
    pub runtime_erased: ArtifactTypeTermIdV1,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ArtifactFlowTermV1 {
    pub mode: FlowMode,
    pub term: ArtifactTypeTermIdV1,
    pub stable_digest: [u8; 32],
    pub runtime_erased: ArtifactTypeTermIdV1,
    pub runtime_erased_digest: [u8; 32],
}

/// Frozen, arena-qualified type DAG. Dense IDs are local coordinates only;
/// receipts and cross-revision identities bind `stable_digest` values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactTypeModuleV1 {
    pub schema: String,
    pub terms: Box<[ArtifactTypeTermV1]>,
    pub stable_digest: [u8; 32],
}

impl Hash for ArtifactTypeModuleV1 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // The stable digest commits every structural term independently of
        // insertion-order-only dense IDs. Hashing the rich table again would
        // recreate the recursive proof replay this module is replacing.
        self.stable_digest.hash(state);
    }
}

impl ArtifactTypeModuleV1 {
    pub fn term(&self, id: ArtifactTypeTermIdV1) -> Option<&ArtifactTypeTermV1> {
        self.terms.get(id.as_usize()).filter(|term| term.id == id)
    }

    pub fn runtime_erased_term(&self, id: ArtifactTypeTermIdV1) -> Option<ArtifactTypeTermIdV1> {
        self.term(id).map(|term| term.runtime_erased)
    }

    pub fn materialize_type(&self, id: ArtifactTypeTermIdV1) -> Result<Type, String> {
        let mut cache = vec![None; self.terms.len()];
        self.materialize_type_with_cache(id, &mut cache)
    }

    pub fn materialize_flow(&self, flow: ArtifactFlowTermV1) -> Result<FlowType, String> {
        Ok(FlowType {
            mode: flow.mode,
            ty: self.materialize_type(flow.term)?,
        })
    }

    fn materialize_type_with_cache(
        &self,
        id: ArtifactTypeTermIdV1,
        cache: &mut [Option<Type>],
    ) -> Result<Type, String> {
        if let Some(ty) = cache.get(id.as_usize()).and_then(Clone::clone) {
            return Ok(ty);
        }
        let term = self
            .term(id)
            .ok_or_else(|| format!("artifact type module has no dense term {}", id.0))?;
        let ty = match &term.kind {
            ArtifactTypeTermKindV1::Text => Type::Text,
            ArtifactTypeTermKindV1::Number => Type::Number,
            ArtifactTypeTermKindV1::Bytes {
                value: ArtifactBytesTermV1::Dynamic,
            } => Type::Bytes(crate::BytesType::Dynamic),
            ArtifactTypeTermKindV1::Bytes {
                value: ArtifactBytesTermV1::Fixed(size),
            } => Type::Bytes(crate::BytesType::Fixed(usize::try_from(*size).map_err(
                |_| format!("artifact fixed byte-list size {size} exceeds usize"),
            )?)),
            ArtifactTypeTermKindV1::Absent => Type::Absent,
            ArtifactTypeTermKindV1::VariantSet { variants } => Type::VariantSet(
                variants
                    .iter()
                    .map(|variant| match variant {
                        ArtifactVariantTermV1::Tag(tag) => Ok(Variant::Tag(tag.clone())),
                        ArtifactVariantTermV1::Tagged { tag, fields } => {
                            let Type::Object(fields) =
                                self.materialize_type_with_cache(*fields, cache)?
                            else {
                                return Err(format!(
                                    "artifact tagged variant `{tag}` references non-object term {}",
                                    fields.0
                                ));
                            };
                            Ok(Variant::tagged(tag.clone(), fields.as_ref().clone()))
                        }
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into(),
            ),
            ArtifactTypeTermKindV1::Object {
                fields,
                field_order,
                open,
            } => Type::object(ObjectShape {
                fields: fields
                    .iter()
                    .map(|field| {
                        Ok((
                            field.name.clone(),
                            self.materialize_type_with_cache(field.ty, cache)?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>, String>>()?,
                field_order: field_order.to_vec(),
                open: *open,
            }),
            ArtifactTypeTermKindV1::RenderContract => Type::RenderContract,
            ArtifactTypeTermKindV1::List { item } => Type::List(Type::shared(
                self.materialize_type_with_cache(*item, cache)?,
            )),
            ArtifactTypeTermKindV1::Function {
                args,
                result_mode,
                result,
            } => Type::Function {
                args: args
                    .iter()
                    .map(|argument| self.materialize_type_with_cache(*argument, cache))
                    .collect::<Result<Vec<_>, _>>()?,
                result: Box::new(FlowType {
                    mode: *result_mode,
                    ty: self.materialize_type_with_cache(*result, cache)?,
                }),
            },
            ArtifactTypeTermKindV1::UnresolvedShape { reason } => Type::UnresolvedShape {
                reason: reason.clone(),
            },
            ArtifactTypeTermKindV1::Variable { ordinal } => Type::Var(TypeVar(*ordinal)),
            ArtifactTypeTermKindV1::Unknown => Type::Unknown,
            ArtifactTypeTermKindV1::Union { members } => Type::Union(
                members
                    .iter()
                    .map(|member| self.materialize_type_with_cache(*member, cache))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            ArtifactTypeTermKindV1::Map { key, value } => Type::Map {
                key: Box::new(self.materialize_type_with_cache(*key, cache)?),
                value: Box::new(self.materialize_type_with_cache(*value, cache)?),
            },
            ArtifactTypeTermKindV1::Set { item } => Type::Set(Type::shared(
                self.materialize_type_with_cache(*item, cache)?,
            )),
            ArtifactTypeTermKindV1::Bits { width } => Type::Bits { width: *width },
        };
        let slot = cache
            .get_mut(id.as_usize())
            .ok_or_else(|| format!("artifact type cache has no term {}", id.0))?;
        *slot = Some(ty.clone());
        Ok(ty)
    }
}

/// Opaque dense kernel publication before it is bound to one checked image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedRuntimeFlowTermProjectionV1 {
    expression_runtime_flow_digests: Box<[[u8; 32]]>,
    stable_digest: [u8; 32],
}

impl Default for CheckedRuntimeFlowTermProjectionV1 {
    fn default() -> Self {
        Self::from_runtime_flow_digests(Vec::new())
    }
}

impl CheckedRuntimeFlowTermProjectionV1 {
    pub fn from_runtime_flow_digests(expression_runtime_flow_digests: Vec<[u8; 32]>) -> Self {
        let stable_digest = runtime_flow_projection_digest(&expression_runtime_flow_digests);
        Self {
            expression_runtime_flow_digests: expression_runtime_flow_digests.into_boxed_slice(),
            stable_digest,
        }
    }

    /// Transitional oracle path for the historical checker. The greenfield
    /// kernel publishes the same digests directly from solved artifact terms.
    pub fn derive_from_checked_expressions(
        expressions: &[CheckedExpression],
    ) -> Result<Self, String> {
        let mut builder = ArtifactTypeModuleBuilderV1::new();
        let mut digests = Vec::with_capacity(expressions.len());
        for (index, expression) in expressions.iter().enumerate() {
            let dense = u32::try_from(index)
                .map_err(|_| "checked expression count exceeds u32".to_owned())?;
            if expression.id != CheckedExprId(dense) {
                return Err(format!(
                    "checked runtime flow-term projection expected dense expression {index}, found {}",
                    expression.id.0
                ));
            }
            digests.push(
                builder
                    .intern_flow(&expression.flow_type)?
                    .runtime_erased_digest,
            );
        }
        Ok(Self::from_runtime_flow_digests(digests))
    }

    /// Replace the runtime-erased flow proofs for a sparse set of expressions.
    ///
    /// The dense checker normally publishes these digests directly from its
    /// solved definition-local term modules. A few whole-project authorities
    /// become exact only while checked rows are linked (currently exact SOURCE
    /// payload projections). Rebuilding the complete rich type graph merely to
    /// publish those sparse corrections would defeat the compact handoff, so
    /// the linker re-interns only the corrected flow roots here.
    pub fn apply_expression_flow_overrides<'a>(
        &mut self,
        overrides: impl IntoIterator<Item = (CheckedExprId, &'a FlowType)>,
    ) -> Result<(), String> {
        let mut builder = ArtifactTypeModuleBuilderV1::new();
        let mut seen = BTreeMap::<CheckedExprId, [u8; 32]>::new();
        for (expression, flow_type) in overrides {
            let digest = builder.intern_flow(flow_type)?.runtime_erased_digest;
            if let Some(previous) = seen.insert(expression, digest)
                && previous != digest
            {
                return Err(format!(
                    "checked runtime flow-term expression {} has conflicting overrides",
                    expression.0
                ));
            }
        }
        for (expression, digest) in seen {
            let slot = self
                .expression_runtime_flow_digests
                .get_mut(expression.0 as usize)
                .ok_or_else(|| {
                    format!(
                        "checked runtime flow-term override references missing expression {}",
                        expression.0
                    )
                })?;
            *slot = digest;
        }
        self.stable_digest = runtime_flow_projection_digest(&self.expression_runtime_flow_digests);
        Ok(())
    }

    pub fn seal(
        self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        checked_image_digest: [u8; 32],
    ) -> CheckedRuntimeFlowTermHandoffV1 {
        let stable_digest = runtime_flow_handoff_digest(
            source_bundle_digest_v1,
            role,
            checked_image_digest,
            self.stable_digest,
        );
        CheckedRuntimeFlowTermHandoffV1 {
            schema: CHECKED_RUNTIME_FLOW_TERM_HANDOFF_SCHEMA_V1.to_owned(),
            source_bundle_digest_v1: *source_bundle_digest_v1.as_bytes(),
            role,
            checked_image_digest,
            expression_runtime_flow_digests: self.expression_runtime_flow_digests,
            projection_digest: self.stable_digest,
            stable_digest,
        }
    }

    pub fn expression_count(&self) -> usize {
        self.expression_runtime_flow_digests.len()
    }

    pub const fn stable_digest(&self) -> [u8; 32] {
        self.stable_digest
    }
}

/// Opaque dense checked-to-semantic type-proof handoff.
///
/// The rich definition-local term DAG remains owned by the checker kernel.
/// Semantic execution needs only the runtime-erased structural flow digest for
/// each dense checked expression, so this carrier does not leak arena-local
/// term IDs or duplicate recursively nested `Type` DTOs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedRuntimeFlowTermHandoffV1 {
    schema: String,
    source_bundle_digest_v1: [u8; 32],
    role: ProgramRole,
    checked_image_digest: [u8; 32],
    expression_runtime_flow_digests: Box<[[u8; 32]]>,
    projection_digest: [u8; 32],
    stable_digest: [u8; 32],
}

impl Default for CheckedRuntimeFlowTermHandoffV1 {
    fn default() -> Self {
        Self {
            schema: String::new(),
            source_bundle_digest_v1: [0; 32],
            role: ProgramRole::Client,
            checked_image_digest: [0; 32],
            expression_runtime_flow_digests: Box::new([]),
            projection_digest: [0; 32],
            stable_digest: [0; 32],
        }
    }
}

impl CheckedRuntimeFlowTermHandoffV1 {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn validate_authority(
        &self,
        source_bundle_digest_v1: SourceBundleDigestV1,
        role: ProgramRole,
        checked_image_digest: [u8; 32],
    ) -> Result<(), String> {
        if self.schema != CHECKED_RUNTIME_FLOW_TERM_HANDOFF_SCHEMA_V1
            || self.source_bundle_digest_v1 != *source_bundle_digest_v1.as_bytes()
            || self.role != role
            || self.checked_image_digest != checked_image_digest
        {
            return Err(
                "checked runtime flow-term handoff belongs to a different checked image".to_owned(),
            );
        }
        if runtime_flow_projection_digest(&self.expression_runtime_flow_digests)
            != self.projection_digest
        {
            return Err(
                "checked runtime flow-term handoff has a stale projection digest".to_owned(),
            );
        }
        if runtime_flow_handoff_digest(
            source_bundle_digest_v1,
            role,
            checked_image_digest,
            self.projection_digest,
        ) != self.stable_digest
        {
            return Err(
                "checked runtime flow-term handoff has a stale authority digest".to_owned(),
            );
        }
        Ok(())
    }

    pub fn expression_count(&self) -> usize {
        self.expression_runtime_flow_digests.len()
    }

    pub fn expression_runtime_flow_digest(&self, expression: CheckedExprId) -> Option<[u8; 32]> {
        self.expression_runtime_flow_digests
            .get(expression.0 as usize)
            .copied()
    }

    pub const fn stable_digest(&self) -> [u8; 32] {
        self.stable_digest
    }
}

/// Append-only construction owner for one artifact type module.
///
/// Callers select ordered versus canonical union explicitly. The builder
/// deduplicates exact structural nodes by stable digest and fails closed on a
/// digest collision.
pub struct ArtifactTypeModuleBuilderV1 {
    terms: Vec<ArtifactTypeTermV1>,
    ids_by_digest: BTreeMap<[u8; 32], Vec<ArtifactTypeTermIdV1>>,
    shared_types: HashMap<usize, ArtifactTypeTermIdV1>,
    shared_objects: HashMap<usize, ArtifactTypeTermIdV1>,
    shared_variants: HashMap<usize, ArtifactTypeTermIdV1>,
    unknown: ArtifactTypeTermIdV1,
}

impl ArtifactTypeModuleBuilderV1 {
    pub fn new() -> Self {
        let placeholder = ArtifactTypeTermIdV1(0);
        let mut builder = Self {
            terms: Vec::new(),
            ids_by_digest: BTreeMap::new(),
            shared_types: HashMap::new(),
            shared_objects: HashMap::new(),
            shared_variants: HashMap::new(),
            unknown: placeholder,
        };
        builder.unknown = builder
            .intern_kind(ArtifactTypeTermKindV1::Unknown)
            .expect("the scalar Unknown artifact term is infallible");
        builder
    }

    pub fn unknown(&self) -> ArtifactTypeTermIdV1 {
        self.unknown
    }

    /// Import an exact public checked type while preserving ordered unions,
    /// object field order metadata, and occurrence-local variable ordinals.
    pub fn intern_type(&mut self, ty: &Type) -> Result<ArtifactTypeTermIdV1, String> {
        match ty {
            Type::Text => self.intern_kind(ArtifactTypeTermKindV1::Text),
            Type::Number => self.intern_kind(ArtifactTypeTermKindV1::Number),
            Type::Bytes(BytesType::Dynamic) => self.intern_kind(ArtifactTypeTermKindV1::Bytes {
                value: ArtifactBytesTermV1::Dynamic,
            }),
            Type::Bytes(BytesType::Fixed(size)) => {
                self.intern_kind(ArtifactTypeTermKindV1::Bytes {
                    value: ArtifactBytesTermV1::Fixed(
                        u64::try_from(*size)
                            .map_err(|_| "fixed byte-list type size exceeds u64".to_owned())?,
                    ),
                })
            }
            Type::Absent => self.intern_kind(ArtifactTypeTermKindV1::Absent),
            Type::VariantSet(variants) => {
                let key = std::sync::Arc::as_ptr(&variants.0) as usize;
                if let Some(term) = self.shared_variants.get(&key).copied() {
                    return Ok(term);
                }
                let variants = variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => Ok(ArtifactVariantTermV1::Tag(tag.clone())),
                        Variant::Tagged { tag, fields } => Ok(ArtifactVariantTermV1::Tagged {
                            tag: tag.clone(),
                            fields: self.intern_object_shape(fields)?,
                        }),
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into_boxed_slice();
                let term = self.intern_kind(ArtifactTypeTermKindV1::VariantSet { variants })?;
                self.shared_variants.insert(key, term);
                Ok(term)
            }
            Type::Object(shape) => self.intern_object_shape(shape),
            Type::RenderContract => self.intern_kind(ArtifactTypeTermKindV1::RenderContract),
            Type::List(item) => {
                let key = std::sync::Arc::as_ptr(&item.0) as usize;
                let item = if let Some(term) = self.shared_types.get(&key).copied() {
                    term
                } else {
                    let term = self.intern_type(item)?;
                    self.shared_types.insert(key, term);
                    term
                };
                self.intern_kind(ArtifactTypeTermKindV1::List { item })
            }
            Type::Function { args, result } => {
                let args = args
                    .iter()
                    .map(|argument| self.intern_type(argument))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                let result_term = self.intern_type(&result.ty)?;
                self.intern_kind(ArtifactTypeTermKindV1::Function {
                    args,
                    result_mode: result.mode,
                    result: result_term,
                })
            }
            Type::UnresolvedShape { reason } => {
                self.intern_kind(ArtifactTypeTermKindV1::UnresolvedShape {
                    reason: reason.clone(),
                })
            }
            Type::Var(TypeVar(ordinal)) => {
                self.intern_kind(ArtifactTypeTermKindV1::Variable { ordinal: *ordinal })
            }
            Type::Unknown => Ok(self.unknown),
            Type::Union(members) => {
                let members = members
                    .iter()
                    .map(|member| self.intern_type(member))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                self.intern_kind(ArtifactTypeTermKindV1::Union { members })
            }
            Type::Map { key, value } => {
                let key = self.intern_type(key)?;
                let value = self.intern_type(value)?;
                self.intern_kind(ArtifactTypeTermKindV1::Map { key, value })
            }
            Type::Set(item) => {
                let key = std::sync::Arc::as_ptr(&item.0) as usize;
                let item = if let Some(term) = self.shared_types.get(&key).copied() {
                    term
                } else {
                    let term = self.intern_type(item)?;
                    self.shared_types.insert(key, term);
                    term
                };
                self.intern_kind(ArtifactTypeTermKindV1::Set { item })
            }
            Type::Bits { width } => {
                self.intern_kind(ArtifactTypeTermKindV1::Bits { width: *width })
            }
        }
    }

    pub fn intern_flow(&mut self, flow: &FlowType) -> Result<ArtifactFlowTermV1, String> {
        let term = self.intern_type(&flow.ty)?;
        self.flow(flow.mode, term)
    }

    pub fn intern_kind(
        &mut self,
        kind: ArtifactTypeTermKindV1,
    ) -> Result<ArtifactTypeTermIdV1, String> {
        self.validate_child_ids(&kind)?;
        let stable_digest = self.kind_digest(&kind)?;
        if let Some(candidates) = self.ids_by_digest.get(&stable_digest) {
            if let Some(existing) = candidates
                .iter()
                .copied()
                .find(|candidate| self.terms[candidate.as_usize()].kind == kind)
            {
                return Ok(existing);
            }
            return Err(format!(
                "artifact type digest collision for structural term {kind:?}"
            ));
        }
        let id = ArtifactTypeTermIdV1(
            u32::try_from(self.terms.len())
                .map_err(|_| "artifact type module exceeds u32".to_owned())?,
        );
        let contains_variable = self.kind_contains_variable(&kind)?;
        self.terms.push(ArtifactTypeTermV1 {
            id,
            kind: kind.clone(),
            stable_digest,
            contains_variable,
            runtime_erased: id,
        });
        self.ids_by_digest
            .entry(stable_digest)
            .or_default()
            .push(id);
        if contains_variable {
            let erased_kind = self.runtime_erased_kind(&kind)?;
            let erased = self.intern_kind(erased_kind)?;
            self.terms[id.as_usize()].runtime_erased = erased;
        }
        Ok(id)
    }

    pub fn flow(
        &self,
        mode: FlowMode,
        term: ArtifactTypeTermIdV1,
    ) -> Result<ArtifactFlowTermV1, String> {
        let definition = self
            .terms
            .get(term.as_usize())
            .filter(|candidate| candidate.id == term)
            .ok_or_else(|| format!("artifact flow references missing term {}", term.0))?;
        let erased = self
            .terms
            .get(definition.runtime_erased.as_usize())
            .filter(|candidate| candidate.id == definition.runtime_erased)
            .ok_or_else(|| {
                format!(
                    "artifact term {} has missing runtime-erased term {}",
                    term.0, definition.runtime_erased.0
                )
            })?;
        Ok(ArtifactFlowTermV1 {
            mode,
            term,
            stable_digest: flow_digest(mode, definition.stable_digest),
            runtime_erased: definition.runtime_erased,
            runtime_erased_digest: flow_digest(mode, erased.stable_digest),
        })
    }

    pub fn finish(self) -> ArtifactTypeModuleV1 {
        let mut digests = self
            .terms
            .iter()
            .map(|term| term.stable_digest)
            .collect::<Vec<_>>();
        digests.sort_unstable();
        let mut hasher = Sha256::new();
        hasher.update(ARTIFACT_TYPE_MODULE_DOMAIN_V1);
        update_len(&mut hasher, digests.len());
        for digest in digests {
            hasher.update(digest);
        }
        ArtifactTypeModuleV1 {
            schema: ARTIFACT_TYPE_MODULE_SCHEMA_V1.to_owned(),
            terms: self.terms.into_boxed_slice(),
            stable_digest: hasher.finalize().into(),
        }
    }

    fn validate_child_ids(&self, kind: &ArtifactTypeTermKindV1) -> Result<(), String> {
        for child in kind_children(kind) {
            if self
                .terms
                .get(child.as_usize())
                .is_none_or(|term| term.id != child)
            {
                return Err(format!(
                    "artifact type term references missing child {}",
                    child.0
                ));
            }
        }
        Ok(())
    }

    fn intern_object_shape(
        &mut self,
        shape: &crate::SharedObjectShape,
    ) -> Result<ArtifactTypeTermIdV1, String> {
        let key = std::sync::Arc::as_ptr(&shape.0) as usize;
        if let Some(term) = self.shared_objects.get(&key).copied() {
            return Ok(term);
        }
        let fields = shape
            .fields
            .iter()
            .map(|(name, ty)| {
                Ok(ArtifactObjectFieldTermV1 {
                    name: name.clone(),
                    ty: self.intern_type(ty)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
            .into_boxed_slice();
        let term = self.intern_kind(ArtifactTypeTermKindV1::Object {
            fields,
            field_order: shape.field_order.clone().into_boxed_slice(),
            open: shape.open,
        })?;
        self.shared_objects.insert(key, term);
        Ok(term)
    }

    fn kind_contains_variable(&self, kind: &ArtifactTypeTermKindV1) -> Result<bool, String> {
        if matches!(kind, ArtifactTypeTermKindV1::Variable { .. }) {
            return Ok(true);
        }
        Ok(kind_children(kind).into_iter().any(|child| {
            self.terms
                .get(child.as_usize())
                .is_some_and(|term| term.contains_variable)
        }))
    }

    fn runtime_erased_kind(
        &self,
        kind: &ArtifactTypeTermKindV1,
    ) -> Result<ArtifactTypeTermKindV1, String> {
        let erased = |id: ArtifactTypeTermIdV1| {
            self.terms
                .get(id.as_usize())
                .filter(|term| term.id == id)
                .map(|term| term.runtime_erased)
                .ok_or_else(|| format!("artifact runtime erasure has no term {}", id.0))
        };
        Ok(match kind {
            ArtifactTypeTermKindV1::Variable { .. } => ArtifactTypeTermKindV1::Unknown,
            ArtifactTypeTermKindV1::VariantSet { variants } => ArtifactTypeTermKindV1::VariantSet {
                variants: variants
                    .iter()
                    .map(|variant| match variant {
                        ArtifactVariantTermV1::Tag(tag) => {
                            Ok(ArtifactVariantTermV1::Tag(tag.clone()))
                        }
                        ArtifactVariantTermV1::Tagged { tag, fields } => {
                            Ok(ArtifactVariantTermV1::Tagged {
                                tag: tag.clone(),
                                fields: erased(*fields)?,
                            })
                        }
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into_boxed_slice(),
            },
            ArtifactTypeTermKindV1::Object {
                fields,
                field_order,
                open,
            } => ArtifactTypeTermKindV1::Object {
                fields: fields
                    .iter()
                    .map(|field| {
                        Ok(ArtifactObjectFieldTermV1 {
                            name: field.name.clone(),
                            ty: erased(field.ty)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into_boxed_slice(),
                field_order: field_order.clone(),
                open: *open,
            },
            ArtifactTypeTermKindV1::List { item } => ArtifactTypeTermKindV1::List {
                item: erased(*item)?,
            },
            ArtifactTypeTermKindV1::Function {
                args,
                result_mode,
                result,
            } => ArtifactTypeTermKindV1::Function {
                args: args
                    .iter()
                    .map(|argument| erased(*argument))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
                result_mode: *result_mode,
                result: erased(*result)?,
            },
            ArtifactTypeTermKindV1::Union { members } => ArtifactTypeTermKindV1::Union {
                members: members
                    .iter()
                    .map(|member| erased(*member))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            },
            ArtifactTypeTermKindV1::Map { key, value } => ArtifactTypeTermKindV1::Map {
                key: erased(*key)?,
                value: erased(*value)?,
            },
            ArtifactTypeTermKindV1::Set { item } => ArtifactTypeTermKindV1::Set {
                item: erased(*item)?,
            },
            other => other.clone(),
        })
    }

    fn kind_digest(&self, kind: &ArtifactTypeTermKindV1) -> Result<[u8; 32], String> {
        let mut hasher = Sha256::new();
        hasher.update(ARTIFACT_TYPE_TERM_DOMAIN_V1);
        let child_digest = |id: ArtifactTypeTermIdV1| {
            self.terms
                .get(id.as_usize())
                .filter(|term| term.id == id)
                .map(|term| term.stable_digest)
                .ok_or_else(|| format!("artifact digest has no child term {}", id.0))
        };
        match kind {
            ArtifactTypeTermKindV1::Text => hasher.update([0]),
            ArtifactTypeTermKindV1::Number => hasher.update([1]),
            ArtifactTypeTermKindV1::Bytes { value } => {
                hasher.update([2]);
                match value {
                    ArtifactBytesTermV1::Dynamic => hasher.update([0]),
                    ArtifactBytesTermV1::Fixed(size) => {
                        hasher.update([1]);
                        hasher.update(size.to_be_bytes());
                    }
                }
            }
            ArtifactTypeTermKindV1::Absent => hasher.update([3]),
            ArtifactTypeTermKindV1::VariantSet { variants } => {
                hasher.update([4]);
                update_len(&mut hasher, variants.len());
                for variant in variants.iter() {
                    match variant {
                        ArtifactVariantTermV1::Tag(tag) => {
                            hasher.update([0]);
                            update_string(&mut hasher, tag);
                        }
                        ArtifactVariantTermV1::Tagged { tag, fields } => {
                            hasher.update([1]);
                            update_string(&mut hasher, tag);
                            hasher.update(child_digest(*fields)?);
                        }
                    }
                }
            }
            ArtifactTypeTermKindV1::Object {
                fields,
                field_order,
                open,
            } => {
                hasher.update([5]);
                update_len(&mut hasher, fields.len());
                for field in fields.iter() {
                    update_string(&mut hasher, &field.name);
                    hasher.update(child_digest(field.ty)?);
                }
                update_len(&mut hasher, field_order.len());
                for field in field_order.iter() {
                    update_string(&mut hasher, field);
                }
                hasher.update([u8::from(*open)]);
            }
            ArtifactTypeTermKindV1::RenderContract => hasher.update([6]),
            ArtifactTypeTermKindV1::List { item } => {
                hasher.update([7]);
                hasher.update(child_digest(*item)?);
            }
            ArtifactTypeTermKindV1::Function {
                args,
                result_mode,
                result,
            } => {
                hasher.update([8]);
                update_len(&mut hasher, args.len());
                for argument in args.iter() {
                    hasher.update(child_digest(*argument)?);
                }
                hasher.update([flow_mode_tag(*result_mode)]);
                hasher.update(child_digest(*result)?);
            }
            ArtifactTypeTermKindV1::UnresolvedShape { reason } => {
                hasher.update([9]);
                update_string(&mut hasher, reason);
            }
            ArtifactTypeTermKindV1::Variable { ordinal } => {
                hasher.update([10]);
                hasher.update(ordinal.to_be_bytes());
            }
            ArtifactTypeTermKindV1::Unknown => hasher.update([11]),
            ArtifactTypeTermKindV1::Union { members } => {
                hasher.update([12]);
                update_len(&mut hasher, members.len());
                for member in members.iter() {
                    hasher.update(child_digest(*member)?);
                }
            }
            ArtifactTypeTermKindV1::Map { key, value } => {
                hasher.update([13]);
                hasher.update(child_digest(*key)?);
                hasher.update(child_digest(*value)?);
            }
            ArtifactTypeTermKindV1::Set { item } => {
                hasher.update([14]);
                hasher.update(child_digest(*item)?);
            }
            ArtifactTypeTermKindV1::Bits { width } => {
                hasher.update([15]);
                hasher.update(width.to_be_bytes());
            }
        }
        Ok(hasher.finalize().into())
    }
}

impl Default for ArtifactTypeModuleBuilderV1 {
    fn default() -> Self {
        Self::new()
    }
}

fn kind_children(kind: &ArtifactTypeTermKindV1) -> Vec<ArtifactTypeTermIdV1> {
    match kind {
        ArtifactTypeTermKindV1::VariantSet { variants } => variants
            .iter()
            .filter_map(|variant| match variant {
                ArtifactVariantTermV1::Tag(_) => None,
                ArtifactVariantTermV1::Tagged { fields, .. } => Some(*fields),
            })
            .collect(),
        ArtifactTypeTermKindV1::Object { fields, .. } => {
            fields.iter().map(|field| field.ty).collect()
        }
        ArtifactTypeTermKindV1::List { item } | ArtifactTypeTermKindV1::Set { item } => vec![*item],
        ArtifactTypeTermKindV1::Function { args, result, .. } => {
            args.iter().copied().chain([*result]).collect()
        }
        ArtifactTypeTermKindV1::Union { members } => members.to_vec(),
        ArtifactTypeTermKindV1::Map { key, value } => vec![*key, *value],
        ArtifactTypeTermKindV1::Text
        | ArtifactTypeTermKindV1::Number
        | ArtifactTypeTermKindV1::Bytes { .. }
        | ArtifactTypeTermKindV1::Absent
        | ArtifactTypeTermKindV1::RenderContract
        | ArtifactTypeTermKindV1::UnresolvedShape { .. }
        | ArtifactTypeTermKindV1::Variable { .. }
        | ArtifactTypeTermKindV1::Unknown
        | ArtifactTypeTermKindV1::Bits { .. } => Vec::new(),
    }
}

fn flow_digest(mode: FlowMode, term_digest: [u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ARTIFACT_FLOW_TERM_DOMAIN_V1);
    hasher.update([flow_mode_tag(mode)]);
    hasher.update(term_digest);
    hasher.finalize().into()
}

fn runtime_flow_projection_digest(digests: &[[u8; 32]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_RUNTIME_FLOW_TERM_PROJECTION_DOMAIN_V1);
    update_len(&mut hasher, digests.len());
    for digest in digests {
        hasher.update(digest);
    }
    hasher.finalize().into()
}

fn runtime_flow_handoff_digest(
    source_bundle_digest_v1: SourceBundleDigestV1,
    role: ProgramRole,
    checked_image_digest: [u8; 32],
    projection_digest: [u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(CHECKED_RUNTIME_FLOW_TERM_HANDOFF_DOMAIN_V1);
    hasher.update(source_bundle_digest_v1.as_bytes());
    update_string(&mut hasher, role.as_str());
    hasher.update(checked_image_digest);
    hasher.update(projection_digest);
    hasher.finalize().into()
}

fn flow_mode_tag(mode: FlowMode) -> u8 {
    match mode {
        FlowMode::Continuous => 0,
        FlowMode::TickPresent => 1,
        FlowMode::PresentOrAbsent => 2,
        FlowMode::Absent => 3,
    }
}

fn update_len(hasher: &mut Sha256, value: usize) {
    hasher.update(
        u64::try_from(value)
            .expect("artifact type collection length exceeds u64")
            .to_be_bytes(),
    );
}

fn update_string(hasher: &mut Sha256, value: &str) {
    update_len(hasher, value.len());
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_contract::SourceBundleUnit;

    fn source_bundle_digest() -> SourceBundleDigestV1 {
        SourceBundleDigestV1::new(
            "app/main.bn",
            [SourceBundleUnit::new("app/main.bn", "value: 1\n")],
        )
        .unwrap()
    }

    #[test]
    fn artifact_terms_preserve_ordered_union_duplicates_and_runtime_erasure() {
        let mut builder = ArtifactTypeModuleBuilderV1::new();
        let text = builder.intern_kind(ArtifactTypeTermKindV1::Text).unwrap();
        let variable = builder
            .intern_kind(ArtifactTypeTermKindV1::Variable { ordinal: 0 })
            .unwrap();
        let duplicate = builder
            .intern_kind(ArtifactTypeTermKindV1::Union {
                members: vec![text, text].into_boxed_slice(),
            })
            .unwrap();
        let single = builder
            .intern_kind(ArtifactTypeTermKindV1::Union {
                members: vec![text].into_boxed_slice(),
            })
            .unwrap();
        let open = builder
            .intern_kind(ArtifactTypeTermKindV1::Object {
                fields: vec![ArtifactObjectFieldTermV1 {
                    name: "value".to_owned(),
                    ty: variable,
                }]
                .into_boxed_slice(),
                field_order: vec!["value".to_owned()].into_boxed_slice(),
                open: true,
            })
            .unwrap();
        let flow = builder.flow(FlowMode::TickPresent, open).unwrap();
        let module = builder.finish();

        assert_ne!(
            module.term(duplicate).unwrap().stable_digest,
            module.term(single).unwrap().stable_digest
        );
        assert!(module.term(open).unwrap().contains_variable);
        assert_ne!(flow.term, flow.runtime_erased);
        assert_ne!(flow.stable_digest, flow.runtime_erased_digest);
        assert_eq!(
            module.materialize_type(duplicate).unwrap(),
            Type::Union(vec![Type::Text, Type::Text])
        );
        assert_eq!(
            module.materialize_flow(flow).unwrap().mode,
            FlowMode::TickPresent
        );
        assert_eq!(
            module.materialize_type(flow.runtime_erased).unwrap(),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Unknown)],
                true,
            ))
        );
    }

    #[test]
    fn runtime_flow_term_handoff_is_bound_to_one_checked_image() {
        let source = source_bundle_digest();
        let checked_image_digest = [7; 32];
        let projection =
            CheckedRuntimeFlowTermProjectionV1::from_runtime_flow_digests(vec![[3; 32]]);
        let handoff = projection.seal(source, ProgramRole::Client, checked_image_digest);

        handoff
            .validate_authority(source, ProgramRole::Client, checked_image_digest)
            .unwrap();
        assert!(
            handoff
                .validate_authority(source, ProgramRole::Server, checked_image_digest)
                .is_err()
        );
        assert!(
            handoff
                .validate_authority(source, ProgramRole::Client, [8; 32])
                .is_err()
        );

        let mut stale = handoff;
        stale.expression_runtime_flow_digests[0] = [4; 32];
        assert!(
            stale
                .validate_authority(source, ProgramRole::Client, checked_image_digest)
                .is_err()
        );
    }
}
