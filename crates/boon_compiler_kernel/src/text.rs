use crate::{
    KernelAbiInput, KernelDefinitionFactsInput, KernelDiagnosticKind, KernelExecutionShapeInput,
    KernelExpressionSemanticPayload, KernelMatchPatternPayload, KernelOwnerBuildError,
    KernelOwnerEdgeRole, KernelOwnerNodeKind, KernelOwnerProgramInput, KernelPattern,
    KernelRenderConstructorKind, KernelStatementKind,
};
use boon_checked::{FlowType, Type, Variant};
use boon_contract::{PackedTextCatalogBuilder, ProjectTextSnapshot};
use boon_data::ExactRoundingRule;
use boon_effect_schema::{ValueType, host_effect_spec};

/// Adds names introduced by normalized kernel equations rather than copied
/// from one source row. The one-shot project builder owns this responsibility
/// so direct packed producers cannot accidentally freeze an incomplete text
/// authority.
pub(crate) fn populate_reserved_project_text(
    catalog: &mut PackedTextCatalogBuilder,
) -> Result<(), KernelOwnerBuildError> {
    let mut collector = TextCollector::Intern(catalog);
    visit_reserved_project_text(&mut collector)
}

/// Temporary rich-input compatibility visitor.
///
/// Production project construction now owns a single mutable catalog through
/// `KernelProjectInputBuilder`. Existing rich producers still need this
/// adapter until their rows carry authority-qualified symbol/path IDs. Once
/// those producers migrate, this visitor and the rich constructors that call
/// it are deleted rather than retained as a second text-authority path.
pub(crate) fn populate_compatibility_project_text(
    catalog: &mut PackedTextCatalogBuilder,
    owners: &[KernelOwnerProgramInput],
    facts: &[KernelDefinitionFactsInput],
    abi: &KernelAbiInput,
) -> Result<(), KernelOwnerBuildError> {
    let mut collector = TextCollector::Intern(catalog);
    visit_compatibility_project_text(&mut collector, owners, facts, abi)
}

/// Fail-closed validation for the temporary rich adapter.
///
/// Current rich rows consume projection paths as symbol sequences, so this
/// checks every segment against the explicit frozen authority. Future packed
/// rows carry their `QualifiedPathId` directly and are checked by the builder
/// before they can enter the project.
pub(crate) fn validate_compatibility_project_text(
    text: &ProjectTextSnapshot,
    owners: &[KernelOwnerProgramInput],
    facts: &[KernelDefinitionFactsInput],
    abi: &KernelAbiInput,
) -> Result<(), KernelOwnerBuildError> {
    let mut collector = TextCollector::Validate(text);
    visit_reserved_project_text(&mut collector)?;
    visit_compatibility_project_text(&mut collector, owners, facts, abi)
}

/// Compatibility constructor for standalone owner/project helpers that have
/// not yet moved to `KernelProjectInputBuilder`.
pub(crate) fn compatibility_project_text_snapshot(
    owners: &[KernelOwnerProgramInput],
    facts: &[KernelDefinitionFactsInput],
    abi: &KernelAbiInput,
) -> Result<ProjectTextSnapshot, KernelOwnerBuildError> {
    let mut catalog = PackedTextCatalogBuilder::new();
    populate_reserved_project_text(&mut catalog)?;
    populate_compatibility_project_text(&mut catalog, owners, facts, abi)?;
    Ok(catalog.freeze())
}

fn visit_reserved_project_text(
    collector: &mut TextCollector<'_>,
) -> Result<(), KernelOwnerBuildError> {
    // These names are introduced by normalized kernel equations rather than
    // copied from one source row. Keep the list next to the visitor so a term
    // arena never falls back to a second mutable interner.
    for symbol in [
        "Row",
        "Column",
        "Stack",
        "kind",
        "key",
        "value",
        "label",
        "items",
        "Pulse",
        "False",
        "True",
        "Found",
        "NotFound",
        "Parsed",
        "InvalidNumber",
        "reason",
        "position",
    ] {
        collector.symbol(symbol)?;
    }
    for rule in ExactRoundingRule::ALL {
        collector.symbol(rule.as_tag())?;
    }
    Ok(())
}

fn visit_compatibility_project_text(
    collector: &mut TextCollector<'_>,
    owners: &[KernelOwnerProgramInput],
    facts: &[KernelDefinitionFactsInput],
    abi: &KernelAbiInput,
) -> Result<(), KernelOwnerBuildError> {
    for owner in owners {
        collector.owner(owner)?;
    }
    for facts in facts {
        collector.facts(facts)?;
    }
    for callable in abi.callables() {
        collector.symbol(&callable.name)?;
        for parameter in &callable.parameters {
            collector.symbol(&parameter.name)?;
            collector.flow_type(&parameter.flow_type)?;
        }
        for context in &callable.contexts {
            collector.symbol(&context.name)?;
            collector.flow_type(&context.flow_type)?;
        }
        collector.flow_type(&callable.result)?;
    }
    Ok(())
}

enum TextCollector<'a> {
    Intern(&'a mut PackedTextCatalogBuilder),
    Validate(&'a ProjectTextSnapshot),
}

impl TextCollector<'_> {
    fn symbol(&mut self, value: &str) -> Result<(), KernelOwnerBuildError> {
        match self {
            Self::Intern(catalog) => catalog
                .intern_symbol(value)
                .map(|_| ())
                .map_err(|error| KernelOwnerBuildError::new(error.to_string())),
            Self::Validate(text) => text.lookup_symbol(value).map(|_| ()).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "explicit kernel project text authority is missing symbol `{value}`"
                ))
            }),
        }
    }

    fn path(&mut self, segments: &[Box<str>]) -> Result<(), KernelOwnerBuildError> {
        match self {
            Self::Intern(catalog) => catalog
                .intern_path(segments.iter().map(AsRef::as_ref))
                .map(|_| ())
                .map_err(|error| KernelOwnerBuildError::new(error.to_string())),
            Self::Validate(text) => {
                for segment in segments {
                    if text.lookup_symbol(segment).is_none() {
                        return Err(KernelOwnerBuildError::new(format!(
                            "explicit kernel project text authority is missing path segment `{segment}`"
                        )));
                    }
                }
                Ok(())
            }
        }
    }

    fn owner(&mut self, owner: &KernelOwnerProgramInput) -> Result<(), KernelOwnerBuildError> {
        for node in &owner.nodes {
            match &node.kind {
                KernelOwnerNodeKind::Known(ty)
                | KernelOwnerNodeKind::Source(ty)
                | KernelOwnerNodeKind::FixedAbiCall { result: ty } => self.ty(ty)?,
                KernelOwnerNodeKind::KnownPacked(_)
                | KernelOwnerNodeKind::SourcePacked(_)
                | KernelOwnerNodeKind::FixedAbiCallPacked { .. } => {}
                KernelOwnerNodeKind::Tag(tag) => self.symbol(tag)?,
                KernelOwnerNodeKind::Record { tag: Some(tag) } => self.symbol(tag)?,
                KernelOwnerNodeKind::FormalRead { fields, .. }
                | KernelOwnerNodeKind::ContextRead { fields, .. }
                | KernelOwnerNodeKind::LexicalRead { fields }
                | KernelOwnerNodeKind::ValueRead { fields, .. }
                | KernelOwnerNodeKind::DerivedRead { fields } => self.path(fields)?,
                KernelOwnerNodeKind::PatternRead { pattern, fields } => {
                    self.pattern(pattern)?;
                    self.path(fields)?;
                }
                KernelOwnerNodeKind::MatchArm { pattern } => self.pattern(pattern)?,
                KernelOwnerNodeKind::RenderConstructor {
                    kind: KernelRenderConstructorKind::Fixed(tag),
                } => self.symbol(tag)?,
                KernelOwnerNodeKind::HostEffect { operation } => {
                    self.symbol(operation)?;
                    if let Some(spec) = host_effect_spec(operation) {
                        if let Some(schema) = spec.schema {
                            self.value_type(&schema.intent)?;
                            self.value_type(&schema.result)?;
                        }
                    }
                }
                KernelOwnerNodeKind::Infix { operation } => self.symbol(operation)?,
                KernelOwnerNodeKind::FieldProjection { field } => self.symbol(field)?,
                KernelOwnerNodeKind::Absent
                | KernelOwnerNodeKind::Text
                | KernelOwnerNodeKind::TextTemplate
                | KernelOwnerNodeKind::Number
                | KernelOwnerNodeKind::Byte
                | KernelOwnerNodeKind::Bits(_)
                | KernelOwnerNodeKind::Record { tag: None }
                | KernelOwnerNodeKind::Block
                | KernelOwnerNodeKind::Collection { .. }
                | KernelOwnerNodeKind::MapEntry
                | KernelOwnerNodeKind::CollectionItemRead
                | KernelOwnerNodeKind::FreshOut
                | KernelOwnerNodeKind::UserCall { .. }
                | KernelOwnerNodeKind::RenderConstructor {
                    kind: KernelRenderConstructorKind::StripeDirection,
                }
                | KernelOwnerNodeKind::PureBuiltin { .. }
                | KernelOwnerNodeKind::Latest
                | KernelOwnerNodeKind::When
                | KernelOwnerNodeKind::Then
                | KernelOwnerNodeKind::Draining
                | KernelOwnerNodeKind::Hold
                | KernelOwnerNodeKind::Arrow
                | KernelOwnerNodeKind::Delimiter
                | KernelOwnerNodeKind::Unknown
                | KernelOwnerNodeKind::Flush => {}
            }
            for edge in &node.inputs {
                match &edge.role {
                    KernelOwnerEdgeRole::RecordField { name, .. }
                    | KernelOwnerEdgeRole::AbiArgument { name } => self.symbol(name)?,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn facts(&mut self, facts: &KernelDefinitionFactsInput) -> Result<(), KernelOwnerBuildError> {
        for payload in &facts.expression_payloads {
            match payload {
                KernelExpressionSemanticPayload::HoldName(name) => self.symbol(name)?,
                KernelExpressionSemanticPayload::MatchPattern(pattern) => {
                    self.match_pattern_payload(pattern)?
                }
                KernelExpressionSemanticPayload::LexicalPath(path) => self.path(path)?,
                KernelExpressionSemanticPayload::None
                | KernelExpressionSemanticPayload::Text(_)
                | KernelExpressionSemanticPayload::TextTemplate(_)
                | KernelExpressionSemanticPayload::Number(_)
                | KernelExpressionSemanticPayload::Byte(_)
                | KernelExpressionSemanticPayload::Bits(_)
                | KernelExpressionSemanticPayload::Delimiter
                | KernelExpressionSemanticPayload::Invalid(_) => {}
            }
        }
        for call in &facts.call_syntax {
            self.symbol(&call.function)?;
            for argument in &call.arguments {
                self.symbol(&argument.name)?;
            }
        }
        for shape in &facts.execution_shapes {
            if let KernelExecutionShapeInput::Record { fields, .. } = shape {
                for field in fields {
                    self.symbol(&field.name)?;
                }
            }
        }
        for statement in &facts.statements {
            match &statement.kind {
                KernelStatementKind::Function { name, parameters } => {
                    self.symbol(name)?;
                    for parameter in parameters {
                        self.symbol(&parameter.name)?;
                    }
                }
                KernelStatementKind::Field { name } => self.symbol(name)?,
                KernelStatementKind::Source { field, event } => {
                    if let Some(field) = field {
                        self.symbol(field)?;
                    }
                    if let Some(event) = event {
                        self.symbol(event)?;
                    }
                }
                KernelStatementKind::Hold { field, name } => {
                    if let Some(field) = field {
                        self.symbol(field)?;
                    }
                    if let Some(name) = name {
                        self.symbol(name)?;
                    }
                }
                KernelStatementKind::List { field, .. } => {
                    if let Some(field) = field {
                        self.symbol(field)?;
                    }
                }
                KernelStatementKind::Block
                | KernelStatementKind::Spread
                | KernelStatementKind::Expression => {}
            }
        }
        for declaration in &facts.declarations {
            self.symbol(&declaration.name)?;
            if let Some(flow) = &declaration.declared_flow_type {
                self.flow_type(flow)?;
            }
        }
        for binding in &facts.lexical_bindings {
            self.path(&binding.projection)?;
        }
        for source in &facts.sources {
            self.path(&source.projection)?;
        }
        for state in &facts.states {
            self.path(&state.projection)?;
        }
        for list in &facts.lists {
            self.path(&list.projection)?;
        }
        for diagnostic in &facts.diagnostics {
            self.diagnostic(&diagnostic.kind)?;
        }
        Ok(())
    }

    fn pattern(&mut self, pattern: &KernelPattern) -> Result<(), KernelOwnerBuildError> {
        match pattern {
            KernelPattern::Tag { name, fields } => {
                self.symbol(name)?;
                self.path(fields)
            }
            KernelPattern::Binding { name } => self.symbol(name),
            KernelPattern::Wildcard
            | KernelPattern::Number
            | KernelPattern::Text
            | KernelPattern::Bits { .. }
            | KernelPattern::Invalid => Ok(()),
        }
    }

    fn match_pattern_payload(
        &mut self,
        pattern: &KernelMatchPatternPayload,
    ) -> Result<(), KernelOwnerBuildError> {
        match pattern {
            KernelMatchPatternPayload::Tag { name, fields } => {
                self.symbol(name)?;
                self.path(fields)
            }
            KernelMatchPatternPayload::Binding(name) => self.symbol(name),
            KernelMatchPatternPayload::Wildcard
            | KernelMatchPatternPayload::Number(_)
            | KernelMatchPatternPayload::Text(_)
            | KernelMatchPatternPayload::Bits(_)
            | KernelMatchPatternPayload::Invalid => Ok(()),
        }
    }

    fn diagnostic(
        &mut self,
        diagnostic: &KernelDiagnosticKind,
    ) -> Result<(), KernelOwnerBuildError> {
        match diagnostic {
            KernelDiagnosticKind::DuplicateRecordField { name }
            | KernelDiagnosticKind::UnresolvedValue { name }
            | KernelDiagnosticKind::AmbiguousValue { name, .. }
            | KernelDiagnosticKind::BareOrdinaryInput { name } => self.symbol(name),
            KernelDiagnosticKind::CallableUsedAsValue { function }
            | KernelDiagnosticKind::UnresolvedCallable { function }
            | KernelDiagnosticKind::AmbiguousCallable { function, .. }
            | KernelDiagnosticKind::PipeWithoutValueInput { function }
            | KernelDiagnosticKind::PassOnAuthoritativeCallable { function, .. }
            | KernelDiagnosticKind::MissingPassContext { function, .. } => self.symbol(function),
            KernelDiagnosticKind::UnexpectedCallEntry { function, name }
            | KernelDiagnosticKind::MissingCallEntry { function, name } => {
                self.symbol(function)?;
                self.symbol(name)
            }
            KernelDiagnosticKind::MisorderedCallEntry {
                function,
                expected_name,
                actual_name,
                ..
            } => {
                self.symbol(function)?;
                self.symbol(expected_name)?;
                self.symbol(actual_name)
            }
            KernelDiagnosticKind::CallInputType {
                actual, expected, ..
            } => {
                self.ty(actual)?;
                self.ty(expected)
            }
            KernelDiagnosticKind::InvalidExpression { .. }
            | KernelDiagnosticKind::InvalidPattern
            | KernelDiagnosticKind::InvalidNumberLiteral { .. }
            | KernelDiagnosticKind::InvalidBitsLiteral { .. }
            | KernelDiagnosticKind::ByteLiteralOutsideBytes
            | KernelDiagnosticKind::MissingPassedContext => Ok(()),
        }
    }

    fn flow_type(&mut self, flow: &FlowType) -> Result<(), KernelOwnerBuildError> {
        self.ty(&flow.ty)
    }

    fn ty(&mut self, ty: &Type) -> Result<(), KernelOwnerBuildError> {
        match ty {
            Type::VariantSet(variants) => {
                for variant in variants.iter() {
                    match variant {
                        Variant::Tag(tag) => self.symbol(tag)?,
                        Variant::Tagged { tag, fields } => {
                            self.symbol(tag)?;
                            for (name, ty) in fields.ordered_fields() {
                                self.symbol(name)?;
                                self.ty(ty)?;
                            }
                        }
                    }
                }
            }
            Type::Object(shape) => {
                for (name, ty) in shape.ordered_fields() {
                    self.symbol(name)?;
                    self.ty(ty)?;
                }
            }
            Type::List(item) | Type::Set(item) => self.ty(item)?,
            Type::Function { args, result } => {
                for argument in args.iter() {
                    self.ty(argument)?;
                }
                self.flow_type(result)?;
            }
            Type::Union(members) => {
                for member in members.iter() {
                    self.ty(member)?;
                }
            }
            Type::Map { key, value } => {
                self.ty(key)?;
                self.ty(value)?;
            }
            Type::Text
            | Type::Number
            | Type::Bytes(_)
            | Type::Absent
            | Type::RenderContract
            | Type::UnresolvedShape { .. }
            | Type::Var(_)
            | Type::Unknown
            | Type::Bits { .. } => {}
        }
        Ok(())
    }

    fn value_type(&mut self, ty: &ValueType) -> Result<(), KernelOwnerBuildError> {
        match ty {
            ValueType::List { item } => self.value_type(item)?,
            ValueType::Record { fields, .. } => {
                for field in fields {
                    self.symbol(field.name)?;
                    self.value_type(&field.value_type)?;
                }
            }
            ValueType::Variant { variants } => {
                for variant in variants {
                    self.symbol(variant.tag)?;
                    for field in &variant.fields {
                        self.symbol(field.name)?;
                        self.value_type(&field.value_type)?;
                    }
                }
            }
            ValueType::Number | ValueType::Text | ValueType::Bytes { .. } => {}
        }
        Ok(())
    }
}
