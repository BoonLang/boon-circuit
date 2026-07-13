use crate::{FieldId, ListId, MachinePlan, ScopeId, SourceId, StateId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

macro_rules! document_usize_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub fn as_usize(self) -> usize {
                    self.0
                }
            }
        )+
    };
}

document_usize_ids!(
    DocumentExprId,
    DocumentFunctionId,
    DocumentParameterId,
    DocumentLocalId,
    DocumentNameId,
    DocumentConstantId,
    DocumentBindingId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentTemplateId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentMaterializationId(pub u64);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentNodeId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentPlan {
    pub root: DocumentRoot,
    pub initial_patch_batch: DocumentInitialPatchBatch,
    pub names: Vec<String>,
    pub constants: Vec<DocumentConstant>,
    pub expressions: Vec<DocumentExpr>,
    pub functions: Vec<DocumentFunction>,
    pub templates: Vec<DocumentTemplate>,
    pub materializations: Vec<DocumentMaterialization>,
    pub view_bindings: Vec<DocumentViewBinding>,
    pub unresolved_op_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRoot {
    pub kind: DocumentRootKind,
    pub node: DocumentNodeId,
    pub template: DocumentTemplateId,
    pub expression: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentInitialPatchBatch {
    pub root: DocumentNodeId,
    pub patches: Vec<DocumentInitialPatch>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentInitialPatch {
    MountRoot {
        root: DocumentNodeId,
        template: DocumentTemplateId,
        root_kind: DocumentRootKind,
        expression: DocumentExprId,
    },
    RegisterTemplate {
        template: DocumentTemplateId,
    },
    RegisterBinding {
        binding: DocumentBindingId,
    },
    RegisterMaterialization {
        materialization: DocumentMaterializationId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentRootKind {
    Document,
    Scene,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentExpr {
    pub id: DocumentExprId,
    pub compiler_id: usize,
    pub value_class: DocumentValueClass,
    pub op: DocumentExprOp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentValueClass {
    Static,
    DynamicScalar,
    DynamicStructure,
    Render,
    ChildList,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentExprOp {
    Constant {
        constant: DocumentConstantId,
    },
    Read {
        read: DocumentRead,
    },
    Project {
        input: DocumentExprId,
        field: DocumentNameId,
    },
    Record {
        fields: Vec<DocumentRecordField>,
    },
    TaggedRecord {
        tag: DocumentNameId,
        fields: Vec<DocumentRecordField>,
    },
    List {
        items: Vec<DocumentListItem>,
    },
    TextTemplate {
        segments: Vec<DocumentTextSegment>,
    },
    LocalBlock {
        bindings: Vec<DocumentLocalBinding>,
        result: DocumentExprId,
    },
    FunctionCall {
        function: DocumentFunctionId,
        arguments: Vec<DocumentCallArgument>,
        passed: Option<DocumentExprId>,
    },
    Builtin {
        builtin: DocumentBuiltin,
        input: Option<DocumentExprId>,
        arguments: Vec<DocumentBuiltinArgument>,
    },
    Scalar {
        operation: DocumentScalarOp,
        left: DocumentExprId,
        right: Option<DocumentExprId>,
    },
    Select {
        input: DocumentExprId,
        arms: Vec<DocumentSelectArm>,
    },
    Latest {
        branches: Vec<DocumentExprId>,
    },
    Then {
        input: DocumentExprId,
        output: Option<DocumentExprId>,
    },
    BindSource {
        input: DocumentExprId,
        source: DocumentExprId,
    },
    Constructor {
        template: DocumentTemplateId,
        constructor: DocumentConstructor,
        arguments: Vec<DocumentConstructorArgument>,
    },
    Materialize {
        materialization: DocumentMaterializationId,
    },
    NoElement,
    SourceContext,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentRead {
    State {
        state: StateId,
    },
    Field {
        field: FieldId,
    },
    List {
        list: ListId,
    },
    Source {
        source: SourceId,
    },
    Parameter {
        parameter: DocumentParameterId,
        projection: Vec<DocumentNameId>,
    },
    Local {
        local: DocumentLocalId,
        projection: Vec<DocumentNameId>,
    },
    Passed {
        projection: Vec<DocumentNameId>,
    },
    Matched {
        selector: usize,
        projection: Vec<DocumentNameId>,
    },
    Row {
        scope: ScopeId,
        field: Option<FieldId>,
        projection: Vec<DocumentNameId>,
    },
    ElementState {
        projection: Vec<DocumentNameId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentConstant {
    pub id: DocumentConstantId,
    pub value: DocumentConstantValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentConstantValue {
    Text { value: String },
    Number { coefficient: i64, scale: u32 },
    Byte { value: u8 },
    Bool { value: bool },
    Bytes { value: Vec<u8> },
    Enum { name: DocumentNameId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentScalarOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    And,
    Or,
    Negate,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentBuiltin {
    BoolAnd,
    BoolNot,
    BoolToggle,
    BytesFind,
    BytesSlice,
    BytesStartsWith,
    BytesToText,
    DirectoryEntries,
    ErrorNew,
    ErrorText,
    FileReadBytes,
    FileWriteText,
    LightAmbient,
    LightDirectional,
    LightSpot,
    ListAny,
    ListAppend,
    ListChunk,
    ListCount,
    ListFilterFieldEqual,
    ListFilterFieldNotEqual,
    ListFilterTextContains,
    ListFind,
    ListFindValue,
    ListGet,
    ListIsNotEmpty,
    ListJoinField,
    ListLatest,
    ListLength,
    ListMap,
    ListRange,
    ListRemove,
    ListRetain,
    ListSortBy,
    ListSum,
    LogError,
    LogInfo,
    NumberBitWidth,
    NumberInterpolate,
    NumberMax,
    NumberMin,
    NumberProjectOffset,
    NumberProjectTime,
    NumberProjectWidth,
    NumberToAsciiText,
    NumberToText,
    RouterGoTo,
    RouterRoute,
    Svg,
    TextAllCharsIn,
    TextConcat,
    TextContains,
    TextEmpty,
    TextIsEmpty,
    TextJoinLines,
    TextLength,
    TextSpace,
    TextStartsWith,
    TextSubstring,
    TextTimeRangeLabel,
    TextToBytes,
    TextToNumber,
    TextToUppercase,
    TextTrim,
    UlidGenerate,
    UrlEncode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentRecordField {
    pub name: Option<DocumentNameId>,
    pub value: DocumentExprId,
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentListItem {
    pub value: DocumentExprId,
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentTextSegment {
    Static { constant: DocumentConstantId },
    Dynamic { value: DocumentExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentLocalBinding {
    pub local: DocumentLocalId,
    pub value: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentCallArgument {
    pub parameter: DocumentParameterId,
    pub value: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentBuiltinArgument {
    pub name: Option<DocumentNameId>,
    pub value: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentSelectArm {
    pub pattern: DocumentPattern,
    pub output: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentPattern {
    Constant { constant: DocumentConstantId },
    Tag { tag: DocumentNameId },
    Wildcard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentConstructor {
    DocumentNew,
    ElementContainer,
    ElementStripe,
    ElementText,
    ElementLabel,
    ElementParagraph,
    ElementLink,
    ElementButton,
    ElementCheckbox,
    ElementTextInput,
    SceneNew,
    SceneElementStripe,
    SceneElementBlock,
    SceneElementText,
    SceneElementTextInput,
    SceneElementCheckbox,
    SceneElementLabel,
    SceneElementButton,
    SceneElementParagraph,
    SceneElementLink,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentConstructorArgument {
    pub name: DocumentNameId,
    pub role: DocumentArgumentRole,
    pub value: DocumentExprId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentArgumentRole {
    Value,
    StaticStyle,
    DynamicStyle,
    StaticText,
    DynamicText,
    Child,
    Children,
    EventBindings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentFunction {
    pub id: DocumentFunctionId,
    pub parameters: Vec<DocumentParameterId>,
    pub body: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentTemplate {
    pub id: DocumentTemplateId,
    pub node: DocumentNodeId,
    pub compiler_expr_id: usize,
    pub owner_function: Option<DocumentFunctionId>,
    pub constructor: DocumentConstructor,
    pub expression: DocumentExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentMaterialization {
    pub id: DocumentMaterializationId,
    pub compiler_expr_id: usize,
    pub source: DocumentMaterializationSource,
    pub item_scope: ScopeId,
    pub item_parameter: DocumentParameterId,
    pub template_function: DocumentFunctionId,
    pub template_arguments: Vec<DocumentCallArgument>,
    pub row_identity: DocumentRowIdentity,
    pub policy: DocumentMaterializationPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentMaterializationSource {
    List {
        list: ListId,
    },
    Field {
        field: FieldId,
    },
    ScopedField {
        scope: ScopeId,
        field: FieldId,
    },
    ParameterField {
        parameter: DocumentParameterId,
        field: FieldId,
    },
    Parameter {
        parameter: DocumentParameterId,
        projection: Vec<DocumentNameId>,
    },
    Expression {
        expression: DocumentExprId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentRowIdentity {
    ListHiddenKeyAndGeneration { list: ListId },
    ScopedHiddenKeyAndGeneration { scope: ScopeId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentMaterializationPolicy {
    VisibleRange,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentViewBinding {
    pub id: DocumentBindingId,
    pub template: Option<DocumentTemplateId>,
    pub attribute: DocumentNameId,
    pub kind: DocumentBindingKind,
    pub target: DocumentBindingTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentBindingKind {
    Data,
    Source,
    Target,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DocumentBindingTarget {
    Source { source: SourceId },
    State { state: StateId },
    Field { field: FieldId },
    List { list: ListId },
    ScopedField { scope: ScopeId, field: FieldId },
    Expression { expression: DocumentExprId },
}

impl DocumentPlan {
    pub fn build_initial_patch_batch(
        root: DocumentRoot,
        templates: &[DocumentTemplate],
        view_bindings: &[DocumentViewBinding],
        materializations: &[DocumentMaterialization],
    ) -> DocumentInitialPatchBatch {
        let mut template_ids = templates
            .iter()
            .map(|template| template.id)
            .collect::<Vec<_>>();
        template_ids.sort_unstable();
        template_ids.dedup();
        let mut binding_ids = view_bindings
            .iter()
            .map(|binding| binding.id)
            .collect::<Vec<_>>();
        binding_ids.sort_unstable();
        binding_ids.dedup();
        let mut materialization_ids = materializations
            .iter()
            .map(|materialization| materialization.id)
            .collect::<Vec<_>>();
        materialization_ids.sort_unstable();
        materialization_ids.dedup();

        let patches = std::iter::once(DocumentInitialPatch::MountRoot {
            root: root.node,
            template: root.template,
            root_kind: root.kind,
            expression: root.expression,
        })
        .chain(
            template_ids
                .into_iter()
                .map(|template| DocumentInitialPatch::RegisterTemplate { template }),
        )
        .chain(
            binding_ids
                .into_iter()
                .map(|binding| DocumentInitialPatch::RegisterBinding { binding }),
        )
        .chain(materialization_ids.into_iter().map(|materialization| {
            DocumentInitialPatch::RegisterMaterialization { materialization }
        }))
        .collect();
        DocumentInitialPatchBatch {
            root: root.node,
            patches,
        }
    }

    pub(crate) fn verify(&self, _machine: &MachinePlan) -> Result<(), String> {
        if self.unresolved_op_count != 0 {
            return Err(format!(
                "{} unresolved document operation(s)",
                self.unresolved_op_count
            ));
        }
        if self.root.expression.0 >= self.expressions.len() {
            return Err("document root expression is out of bounds".to_owned());
        }
        if self.initial_patch_batch.root != self.root.node
            || self.initial_patch_batch
                != Self::build_initial_patch_batch(
                    self.root,
                    &self.templates,
                    &self.view_bindings,
                    &self.materializations,
                )
        {
            return Err("document initial patch batch is not canonical".to_owned());
        }
        if !self
            .expressions
            .iter()
            .enumerate()
            .all(|(index, expression)| expression.id.0 == index)
        {
            return Err("document expression ids are not dense and ordered".to_owned());
        }
        let function_ids = self
            .functions
            .iter()
            .map(|function| function.id)
            .collect::<BTreeSet<_>>();
        if function_ids.len() != self.functions.len() {
            return Err("document function ids are not unique".to_owned());
        }
        let template_ids = self
            .templates
            .iter()
            .map(|template| template.id)
            .collect::<BTreeSet<_>>();
        if template_ids.len() != self.templates.len() {
            return Err("document template ids are not unique".to_owned());
        }
        let materialization_ids = self
            .materializations
            .iter()
            .map(|materialization| materialization.id)
            .collect::<BTreeSet<_>>();
        if materialization_ids.len() != self.materializations.len() {
            return Err("document materialization ids are not unique".to_owned());
        }
        let expression_count = self.expressions.len();
        let constant_count = self.constants.len();
        for expression in &self.expressions {
            for referenced in expression.op.expression_refs() {
                if referenced.0 >= expression_count {
                    return Err(format!(
                        "document expression {} references missing expression {}",
                        expression.id.0, referenced.0
                    ));
                }
            }
            for constant in expression.op.constant_refs() {
                if constant.0 >= constant_count {
                    return Err(format!(
                        "document expression {} references missing constant {}",
                        expression.id.0, constant.0
                    ));
                }
            }
            if let DocumentExprOp::FunctionCall { function, .. } = &expression.op
                && !function_ids.contains(function)
            {
                return Err(format!(
                    "document expression {} references missing function {}",
                    expression.id.0, function.0
                ));
            }
            if let DocumentExprOp::Constructor { template, .. } = &expression.op
                && !template_ids.contains(template)
            {
                return Err(format!(
                    "document expression {} references missing template {}",
                    expression.id.0, template.0
                ));
            }
            if let DocumentExprOp::Materialize { materialization } = &expression.op
                && !materialization_ids.contains(materialization)
            {
                return Err(format!(
                    "document expression {} references missing materialization {}",
                    expression.id.0, materialization.0
                ));
            }
        }
        if self
            .functions
            .iter()
            .any(|function| function.body.0 >= expression_count)
        {
            return Err("document function body expression is out of bounds".to_owned());
        }
        if self.materializations.iter().any(|materialization| {
            !function_ids.contains(&materialization.template_function)
                || matches!(
                    materialization.source,
                    DocumentMaterializationSource::Expression { expression }
                        if expression.0 >= expression_count
                )
        }) {
            return Err("document materialization has an unresolved typed reference".to_owned());
        }
        if self.names.iter().any(String::is_empty) {
            return Err("document name table contains an empty name".to_owned());
        }
        Ok(())
    }
}

impl DocumentExprOp {
    fn expression_refs(&self) -> Vec<DocumentExprId> {
        match self {
            Self::Constant { .. } | Self::Read { .. } | Self::NoElement | Self::SourceContext => {
                Vec::new()
            }
            Self::Project { input, .. } => vec![*input],
            Self::Record { fields } | Self::TaggedRecord { fields, .. } => {
                fields.iter().map(|field| field.value).collect()
            }
            Self::List { items } => items.iter().map(|item| item.value).collect(),
            Self::TextTemplate { segments } => segments
                .iter()
                .filter_map(|segment| match segment {
                    DocumentTextSegment::Static { .. } => None,
                    DocumentTextSegment::Dynamic { value } => Some(*value),
                })
                .collect(),
            Self::LocalBlock { bindings, result } => bindings
                .iter()
                .map(|binding| binding.value)
                .chain(std::iter::once(*result))
                .collect(),
            Self::FunctionCall {
                arguments, passed, ..
            } => arguments
                .iter()
                .map(|argument| argument.value)
                .chain(passed.iter().copied())
                .collect(),
            Self::Builtin {
                input, arguments, ..
            } => input
                .iter()
                .copied()
                .chain(arguments.iter().map(|argument| argument.value))
                .collect(),
            Self::Scalar { left, right, .. } => std::iter::once(*left)
                .chain(right.iter().copied())
                .collect(),
            Self::Select { input, arms } => std::iter::once(*input)
                .chain(arms.iter().map(|arm| arm.output))
                .collect(),
            Self::Latest { branches } => branches.clone(),
            Self::Then { input, output } => std::iter::once(*input)
                .chain(output.iter().copied())
                .collect(),
            Self::BindSource { input, source } => vec![*input, *source],
            Self::Constructor { arguments, .. } => {
                arguments.iter().map(|argument| argument.value).collect()
            }
            Self::Materialize { .. } => Vec::new(),
        }
    }

    fn constant_refs(&self) -> Vec<DocumentConstantId> {
        match self {
            Self::Constant { constant } => vec![*constant],
            Self::Select { arms, .. } => arms
                .iter()
                .filter_map(|arm| match arm.pattern {
                    DocumentPattern::Constant { constant } => Some(constant),
                    DocumentPattern::Tag { .. } | DocumentPattern::Wildcard => None,
                })
                .collect(),
            Self::TextTemplate { segments } => segments
                .iter()
                .filter_map(|segment| match segment {
                    DocumentTextSegment::Static { constant } => Some(*constant),
                    DocumentTextSegment::Dynamic { .. } => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}
