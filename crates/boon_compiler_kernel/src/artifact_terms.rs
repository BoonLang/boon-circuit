use crate::{BytesTerm, KernelSolveError, TypeTerm, TypeTermArena, TypeTermId, TypeVariableId};
use boon_checked::{
    ArtifactBytesTermV1, ArtifactFlowTermV1, ArtifactObjectFieldTermV1,
    ArtifactTypeModuleBuilderV1, ArtifactTypeModuleV1, ArtifactTypeTermIdV1,
    ArtifactTypeTermKindV1, ArtifactVariantTermV1, FlowMode,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

const KERNEL_DEFINITION_FLOW_TERMS_DOMAIN_V1: &[u8] =
    b"boon.compiler-kernel.definition-flow-terms.v1\0";

/// Definition-local canonical type authority for the public result, callable
/// formals, and every solved expression. Dense term IDs are meaningful only
/// inside `module`; `stable_digest` is the receipt identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct KernelDefinitionFlowTermsV1 {
    pub module: ArtifactTypeModuleV1,
    pub result: ArtifactFlowTermV1,
    pub formals: Box<[ArtifactFlowTermV1]>,
    pub expressions: Box<[ArtifactFlowTermV1]>,
    pub stable_digest: [u8; 32],
}

impl Hash for KernelDefinitionFlowTermsV1 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Bind the structural module and ordered roots once. Hashing its rich
        // nodes again would undo the purpose of the canonical term authority.
        self.stable_digest.hash(state);
    }
}

pub(crate) fn materialize_definition_flow_terms_v1(
    source: &TypeTermArena,
    formal_roots: &[(TypeTermId, FlowMode)],
    result_root: (TypeTermId, FlowMode),
    expression_roots: &[(TypeTermId, FlowMode)],
) -> Result<KernelDefinitionFlowTermsV1, KernelSolveError> {
    let mut importer = KernelArtifactTermImporter::new(source);
    let formals = formal_roots
        .iter()
        .map(|(term, mode)| importer.import_flow(*term, *mode))
        .collect::<Result<Vec<_>, _>>()?;
    let result = importer.import_flow(result_root.0, result_root.1)?;
    let expressions = expression_roots
        .iter()
        .map(|(term, mode)| importer.import_flow(*term, *mode))
        .collect::<Result<Vec<_>, _>>()?;
    let module = importer.finish();
    let stable_digest = definition_flow_terms_digest(&module, result, &formals, &expressions);
    Ok(KernelDefinitionFlowTermsV1 {
        module,
        result,
        formals: formals.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        stable_digest,
    })
}

struct KernelArtifactTermImporter<'a> {
    source: &'a TypeTermArena,
    target: ArtifactTypeModuleBuilderV1,
    terms: HashMap<TypeTermId, ArtifactTypeTermIdV1>,
    variables: BTreeMap<TypeVariableId, u32>,
}

impl<'a> KernelArtifactTermImporter<'a> {
    fn new(source: &'a TypeTermArena) -> Self {
        Self {
            source,
            target: ArtifactTypeModuleBuilderV1::new(),
            // A component arena can contain tens of thousands of terms while
            // one definition reaches only a small subset. A dense scratch per
            // definition made NovyWave zero roughly 85 million empty slots.
            terms: HashMap::new(),
            variables: BTreeMap::new(),
        }
    }

    fn import_flow(
        &mut self,
        term: TypeTermId,
        mode: FlowMode,
    ) -> Result<ArtifactFlowTermV1, KernelSolveError> {
        let term = self.import(term)?;
        self.target.flow(mode, term).map_err(KernelSolveError::new)
    }

    fn import(&mut self, source_id: TypeTermId) -> Result<ArtifactTypeTermIdV1, KernelSolveError> {
        if let Some(term) = self.terms.get(&source_id).copied() {
            return Ok(term);
        }
        let kind = match self.source.term(source_id) {
            TypeTerm::Text => ArtifactTypeTermKindV1::Text,
            TypeTerm::Number => ArtifactTypeTermKindV1::Number,
            TypeTerm::Bytes(BytesTerm::Dynamic) => ArtifactTypeTermKindV1::Bytes {
                value: ArtifactBytesTermV1::Dynamic,
            },
            TypeTerm::Bytes(BytesTerm::Fixed(size)) => ArtifactTypeTermKindV1::Bytes {
                value: ArtifactBytesTermV1::Fixed(u64::try_from(*size).map_err(|_| {
                    KernelSolveError::new("kernel fixed byte-list size exceeds u64")
                })?),
            },
            TypeTerm::Absent => ArtifactTypeTermKindV1::Absent,
            TypeTerm::VariantSet(variants) => ArtifactTypeTermKindV1::VariantSet {
                variants: variants
                    .iter()
                    .map(|variant| match variant {
                        crate::VariantTerm::Tag(tag) => Ok(ArtifactVariantTermV1::Tag(
                            self.source.name(*tag).to_owned(),
                        )),
                        crate::VariantTerm::Tagged { tag, fields } => {
                            Ok(ArtifactVariantTermV1::Tagged {
                                tag: self.source.name(*tag).to_owned(),
                                fields: self.import(*fields)?,
                            })
                        }
                    })
                    .collect::<Result<Vec<_>, KernelSolveError>>()?
                    .into_boxed_slice(),
            },
            TypeTerm::Object { fields, open } => {
                let field_order = fields
                    .iter()
                    .map(|field| self.source.name(field.name).to_owned())
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let mut canonical_fields = fields
                    .iter()
                    .map(|field| (self.source.name(field.name).to_owned(), field.ty))
                    .collect::<Vec<_>>();
                // Rich checked objects store fields in a BTreeMap while
                // retaining authored order separately. Stable term identity
                // must use that same split or a direct kernel term for
                // `{ z, a }` differs from its rich checked projection.
                canonical_fields.sort_unstable_by(|left, right| left.0.cmp(&right.0));
                ArtifactTypeTermKindV1::Object {
                    fields: canonical_fields
                        .into_iter()
                        .map(|(name, ty)| {
                            Ok(ArtifactObjectFieldTermV1 {
                                name,
                                ty: self.import(ty)?,
                            })
                        })
                        .collect::<Result<Vec<_>, KernelSolveError>>()?
                        .into_boxed_slice(),
                    field_order,
                    open: *open,
                }
            }
            TypeTerm::OpenObjectPlaceholder => ArtifactTypeTermKindV1::Object {
                fields: Box::new([]),
                field_order: Box::new([]),
                open: true,
            },
            TypeTerm::RenderContract => ArtifactTypeTermKindV1::RenderContract,
            TypeTerm::List(item) => ArtifactTypeTermKindV1::List {
                item: self.import(*item)?,
            },
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => ArtifactTypeTermKindV1::Function {
                args: args
                    .iter()
                    .map(|argument| self.import(*argument))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
                result_mode: *result_mode,
                result: self.import(*result)?,
            },
            TypeTerm::UnresolvedShape(reason) => ArtifactTypeTermKindV1::UnresolvedShape {
                reason: self.source.name(*reason).to_owned(),
            },
            TypeTerm::Variable(variable) => {
                let next = u32::try_from(self.variables.len()).map_err(|_| {
                    KernelSolveError::new("kernel artifact type-variable count exceeds u32")
                })?;
                let ordinal = *self.variables.entry(*variable).or_insert(next);
                ArtifactTypeTermKindV1::Variable { ordinal }
            }
            TypeTerm::Unknown => ArtifactTypeTermKindV1::Unknown,
            TypeTerm::Union(members) => ArtifactTypeTermKindV1::Union {
                members: members
                    .iter()
                    .map(|member| self.import(*member))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
            },
            TypeTerm::Map { key, value } => ArtifactTypeTermKindV1::Map {
                key: self.import(*key)?,
                value: self.import(*value)?,
            },
            TypeTerm::Set(item) => ArtifactTypeTermKindV1::Set {
                item: self.import(*item)?,
            },
            TypeTerm::Bits(width) => ArtifactTypeTermKindV1::Bits { width: *width },
        };
        let target_id = self
            .target
            .intern_kind(kind)
            .map_err(KernelSolveError::new)?;
        if self.terms.insert(source_id, target_id).is_some() {
            return Err(KernelSolveError::new(
                "kernel artifact term was imported more than once",
            ));
        }
        Ok(target_id)
    }

    fn finish(self) -> ArtifactTypeModuleV1 {
        self.target.finish()
    }
}

fn definition_flow_terms_digest(
    module: &ArtifactTypeModuleV1,
    result: ArtifactFlowTermV1,
    formals: &[ArtifactFlowTermV1],
    expressions: &[ArtifactFlowTermV1],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(KERNEL_DEFINITION_FLOW_TERMS_DOMAIN_V1);
    hasher.update(module.stable_digest);
    hasher.update(result.stable_digest);
    update_flow_roots(&mut hasher, formals);
    update_flow_roots(&mut hasher, expressions);
    hasher.finalize().into()
}

fn update_flow_roots(hasher: &mut Sha256, roots: &[ArtifactFlowTermV1]) {
    hasher.update(
        u64::try_from(roots.len())
            .expect("kernel artifact flow root count exceeds u64")
            .to_be_bytes(),
    );
    for root in roots {
        hasher.update(root.stable_digest);
    }
}

#[cfg(test)]
pub(crate) fn materialize_checked_definition_flow_terms_for_test_v1(
    formals: &[boon_checked::FlowType],
    result: &boon_checked::FlowType,
    expressions: &[boon_checked::FlowType],
) -> KernelDefinitionFlowTermsV1 {
    let mut variables = BTreeMap::new();
    let mut next = 0;
    let formals = formals
        .iter()
        .map(|flow| crate::alpha_normalize_flow_type(flow, &mut variables, &mut next))
        .collect::<Vec<_>>();
    let result = crate::alpha_normalize_flow_type(result, &mut variables, &mut next);
    let expressions = expressions
        .iter()
        .map(|flow| crate::alpha_normalize_flow_type(flow, &mut variables, &mut next))
        .collect::<Vec<_>>();

    let mut builder = ArtifactTypeModuleBuilderV1::new();
    let formals = formals
        .iter()
        .map(|flow| builder.intern_flow(flow))
        .collect::<Result<Vec<_>, _>>()
        .expect("checked test formals produce an artifact type module");
    let result = builder
        .intern_flow(&result)
        .expect("checked test result produces an artifact type module");
    let expressions = expressions
        .iter()
        .map(|flow| builder.intern_flow(flow))
        .collect::<Result<Vec<_>, _>>()
        .expect("checked test expressions produce an artifact type module");
    let module = builder.finish();
    let stable_digest = definition_flow_terms_digest(&module, result, &formals, &expressions);
    KernelDefinitionFlowTermsV1 {
        module,
        result,
        formals: formals.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        stable_digest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TypeTermArena, TypeVariableId};
    use boon_checked::{FlowType, ObjectShape, Type, TypeVar};

    #[test]
    fn solver_roots_materialize_exactly_and_alpha_rebase_variables() {
        let mut source = TypeTermArena::new();
        let value_name = source.intern_name("value");
        let variable = source.variable(TypeVariableId(37));
        let record = source.object([(value_name, variable)], true);
        let list = source.list(record);
        let expected_record = Type::object(ObjectShape::from_ordered_fields(
            [("value".to_owned(), Type::Var(TypeVar(0)))],
            true,
        ));

        let artifact = materialize_definition_flow_terms_v1(
            &source,
            &[(variable, FlowMode::Continuous)],
            (list, FlowMode::TickPresent),
            &[
                (record, FlowMode::Continuous),
                (list, FlowMode::TickPresent),
            ],
        )
        .unwrap();

        assert_eq!(
            artifact
                .module
                .materialize_flow(artifact.formals[0])
                .unwrap(),
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Var(TypeVar(0)),
            }
        );
        assert_eq!(
            artifact
                .module
                .materialize_flow(artifact.expressions[0])
                .unwrap(),
            FlowType {
                mode: FlowMode::Continuous,
                ty: expected_record.clone(),
            }
        );
        assert_eq!(
            artifact.module.materialize_flow(artifact.result).unwrap(),
            FlowType {
                mode: FlowMode::TickPresent,
                ty: Type::List(Type::shared(expected_record)),
            }
        );
        assert_eq!(artifact.result, artifact.expressions[1]);
    }

    #[test]
    fn stable_term_identity_ignores_solver_insertion_order() {
        let build = |insert_unused_first: bool| {
            let mut source = TypeTermArena::new();
            if insert_unused_first {
                let unused = source.intern_name("unused");
                let number = source.number();
                let _ = source.object([(unused, number)], false);
            }
            let value = source.intern_name("value");
            let text = source.text();
            let root = source.object([(value, text)], false);
            let artifact = materialize_definition_flow_terms_v1(
                &source,
                &[],
                (root, FlowMode::Continuous),
                &[(root, FlowMode::Continuous)],
            )
            .unwrap();
            (root, artifact)
        };

        let first = build(false);
        let shifted = build(true);
        assert_ne!(first.0, shifted.0);
        assert_eq!(first.1.result.stable_digest, shifted.1.result.stable_digest);
        assert_eq!(first.1.stable_digest, shifted.1.stable_digest);
    }

    #[test]
    fn object_runtime_digest_matches_rich_projection_with_nonlexical_authored_order() {
        let mut source = TypeTermArena::new();
        let z = source.intern_name("z");
        let a = source.intern_name("a");
        let number = source.number();
        let text = source.text();
        let root = source.object([(z, number), (a, text)], false);
        let direct = materialize_definition_flow_terms_v1(
            &source,
            &[],
            (root, FlowMode::Continuous),
            &[(root, FlowMode::Continuous)],
        )
        .unwrap();

        let rich = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::object(ObjectShape::from_ordered_fields(
                [("z".to_owned(), Type::Number), ("a".to_owned(), Type::Text)],
                false,
            )),
        };
        assert_eq!(direct.module.materialize_flow(direct.result).unwrap(), rich);

        let mut rich_builder = ArtifactTypeModuleBuilderV1::new();
        let rich_term = rich_builder.intern_flow(&rich).unwrap();
        assert_eq!(
            direct.result.runtime_erased_digest, rich_term.runtime_erased_digest,
            "direct kernel and rich checked object projections must publish one runtime identity",
        );
    }
}
