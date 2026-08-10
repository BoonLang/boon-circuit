use boon_checked::CheckedValueUse;
use boon_parser::UnitOwnerSyntaxView;
use boon_syntax::{
    AstBlockBinding, AstBlockBindingDeclaration, AstCallArg, AstExprKind, AstParameter,
    AstPassContext, AstRecordField, AstStatementKind, AstTextSegment, StableCheckOwnerKey,
    StableExpressionKey, StableExpressionRouteSegment, StableOwnerKey, StableStatementKey,
    UnitItemKind, UnitLocalStatementId,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};

const OWNER_KEY_FINGERPRINT_DOMAIN_V1: &[u8] = b"boon.check-owner-key.v1\0";
const OWNER_SYNTAX_FINGERPRINT_DOMAIN_V2: &[u8] = b"boon.owner-syntax-input.v2\0";
const OWNER_SOURCE_MAP_FINGERPRINT_DOMAIN_V2: &[u8] = b"boon.owner-source-map.v2\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSyntaxProjectionError {
    message: String,
}

impl OwnerSyntaxProjectionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerSyntaxProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerSyntaxProjectionError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerStatementInput {
    pub id: u32,
    pub stable_key: StableStatementKey,
    pub parent: Option<u32>,
    pub child_index: u32,
    pub kind: AstStatementKind,
    pub expression: Option<u32>,
    pub value_use: CheckedValueUse,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExpressionInput {
    pub stable_key: StableExpressionKey,
    pub linked_input: Option<u32>,
    /// Exact `WHEN`/`WHILE` selector for a match arm. This can address an
    /// enclosing owner when an authored child item owns the arm body.
    pub pattern_selector: Option<u32>,
    pub kind: AstExprKind,
}

/// A reference from this owner's retained syntax across an owner boundary.
///
/// Ordinary structural references point into descendant owners. A normalized
/// pipeline `linked_input` may instead point into an enclosing owner (notably a
/// `HOLD` initializer). The syntax expression identity and its owning shard are
/// both stable across revisions; the other body is deliberately absent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExternalExpressionInput {
    pub owner: StableCheckOwnerKey,
    pub expression: StableExpressionKey,
    pub exact_enclosing_capture: bool,
}

impl OwnerExternalExpressionInput {
    /// Whether this reference captures one exact expression from an enclosing
    /// owner rather than consuming a descendant owner's public result.
    pub fn is_exact_enclosing_capture_for(&self, consumer: &StableCheckOwnerKey) -> bool {
        self.exact_enclosing_capture && is_descendant_owner(&self.owner, consumer)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerChildInput {
    pub owner: StableCheckOwnerKey,
    pub parent: Option<u32>,
    pub child_index: u32,
    /// Stable identity of the authored expression at the direct owner
    /// boundary. This can differ from `result_expression` when the child's
    /// public value is supplied by a deeper descendant owner.
    pub boundary_expression: Option<StableExpressionKey>,
    /// Stable identity of the child's public result at this boundary. A
    /// function declaration has no value in its containing statement lane.
    pub result_expression: Option<StableExpressionKey>,
    /// Exact placement of the public result in the containing owner. Keeping
    /// statement-lane and expression-edge placement distinct prevents a lost
    /// structural edge from silently falling back to a broader lexical scope.
    pub result_placement: OwnerChildResultPlacementInput,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExpressionParentEdgeInput {
    pub child_owner: StableCheckOwnerKey,
    pub child_expression: StableExpressionKey,
    pub owner: StableCheckOwnerKey,
    pub expression: StableExpressionKey,
    pub segment: StableExpressionRouteSegment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerChildResultPlacementInput {
    Valueless,
    StatementLane,
    ExpressionEdge {
        edge: OwnerExpressionParentEdgeInput,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerContainingScopeInput {
    ProjectRoot,
    OwnerStatement {
        owner: StableCheckOwnerKey,
        statement: StableStatementKey,
    },
}

/// Span-free, owner-bounded syntax consumed by future interface/body requests.
///
/// Expression references use one compact namespace: local expressions come
/// first, followed by `external_expressions`. Stable expression keys remain
/// alongside both forms for cross-revision identity, while source positions
/// live exclusively in [`OwnerSourceMap`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSyntaxInput {
    pub owner: StableCheckOwnerKey,
    pub containing_scope: OwnerContainingScopeInput,
    pub statements: Box<[OwnerStatementInput]>,
    pub expressions: Box<[OwnerExpressionInput]>,
    pub external_expressions: Box<[OwnerExternalExpressionInput]>,
    pub child_owners: Box<[OwnerChildInput]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerSyntaxInput {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn external_expression(&self, reference: usize) -> Option<&OwnerExternalExpressionInput> {
        reference
            .checked_sub(self.expressions.len())
            .and_then(|external| self.external_expressions.get(external))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerStatementSource {
    pub statement: u32,
    pub stable_key: StableStatementKey,
    pub line: u64,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExpressionSource {
    pub expression: StableExpressionKey,
    pub line: u64,
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSourceAnchorSite {
    Statement { statement: u32 },
    Expression { expression: StableExpressionKey },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerSourceAnchorRole {
    FunctionParameter { ordinal: u32 },
    CallArgument { ordinal: u32 },
    CallPass,
    PipeArgument { ordinal: u32 },
    PipePass,
    RecordField { ordinal: u32 },
    BlockBinding { ordinal: u32 },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerSourceAnchor {
    pub site: OwnerSourceAnchorSite,
    pub role: OwnerSourceAnchorRole,
    pub line: u64,
    pub start: u64,
    pub end: u64,
}

/// Current source positions for one owner, fingerprinted independently from
/// its semantics so formatting edits cannot invalidate checked body results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSourceMap {
    owner: StableCheckOwnerKey,
    path: String,
    statements: Box<[OwnerStatementSource]>,
    expressions: Box<[OwnerExpressionSource]>,
    anchors: Box<[OwnerSourceAnchor]>,
    fingerprint_v2: [u8; 32],
}

impl OwnerSourceMap {
    pub const fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn statements(&self) -> &[OwnerStatementSource] {
        &self.statements
    }

    pub fn expressions(&self) -> &[OwnerExpressionSource] {
        &self.expressions
    }

    pub const fn fingerprint_v2(&self) -> [u8; 32] {
        self.fingerprint_v2
    }

    pub fn anchor(
        &self,
        site: &OwnerSourceAnchorSite,
        role: OwnerSourceAnchorRole,
    ) -> Option<&OwnerSourceAnchor> {
        self.anchors
            .binary_search_by(|anchor| (&anchor.site, anchor.role).cmp(&(site, role)))
            .ok()
            .and_then(|index| self.anchors.get(index))
    }
}

struct Sha256Writer(Sha256);

impl Write for Sha256Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn fingerprint_serialized<T: Serialize>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], OwnerSyntaxProjectionError> {
    let mut writer = Sha256Writer(Sha256::new());
    writer.0.update(domain);
    serde_json::to_writer(&mut writer, value).map_err(|error| {
        OwnerSyntaxProjectionError::new(format!("cannot fingerprint owner syntax: {error}"))
    })?;
    Ok(writer.0.finalize().into())
}

pub fn stable_check_owner_key_fingerprint_v1(owner: &StableCheckOwnerKey) -> [u8; 32] {
    fingerprint_serialized(OWNER_KEY_FINGERPRINT_DOMAIN_V1, owner)
        .expect("stable check-owner keys always serialize to the digest writer")
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerSyntaxProjectionError> {
    u32::try_from(value).map_err(|_| {
        OwnerSyntaxProjectionError::new(format!("{context} exceeds the owner-local u32 bound"))
    })
}

fn checked_u64(value: usize, context: &str) -> Result<u64, OwnerSyntaxProjectionError> {
    u64::try_from(value).map_err(|_| {
        OwnerSyntaxProjectionError::new(format!("{context} exceeds the source-map u64 bound"))
    })
}

struct ExpressionProjection<'a> {
    owner: &'a StableCheckOwnerKey,
    view: UnitOwnerSyntaxView<'a>,
    local_by_syntax: &'a BTreeMap<usize, u32>,
    external_by_syntax: BTreeMap<usize, u32>,
    external_expressions: Vec<OwnerExternalExpressionInput>,
}

impl<'a> ExpressionProjection<'a> {
    fn new(
        owner: &'a StableCheckOwnerKey,
        view: UnitOwnerSyntaxView<'a>,
        local_by_syntax: &'a BTreeMap<usize, u32>,
    ) -> Self {
        Self {
            owner,
            view,
            local_by_syntax,
            external_by_syntax: BTreeMap::new(),
            external_expressions: Vec::new(),
        }
    }

    fn mapped_with_enclosing(
        &mut self,
        expression: usize,
        context: &str,
        allow_enclosing: bool,
        exact_enclosing_capture: bool,
    ) -> Result<u32, OwnerSyntaxProjectionError> {
        if let Some(local) = self.local_by_syntax.get(&expression) {
            return Ok(*local);
        }
        if let Some(external) = self.external_by_syntax.get(&expression) {
            let external_index = (*external as usize)
                .checked_sub(self.local_by_syntax.len())
                .ok_or_else(|| {
                    OwnerSyntaxProjectionError::new(
                        "owner external-expression interner is inconsistent",
                    )
                })?;
            let external_expression = self
                .external_expressions
                .get_mut(external_index)
                .ok_or_else(|| {
                    OwnerSyntaxProjectionError::new(
                        "owner external-expression interner is inconsistent",
                    )
                })?;
            let permitted = is_descendant_owner(self.owner, &external_expression.owner)
                || (allow_enclosing && is_descendant_owner(&external_expression.owner, self.owner));
            if permitted {
                external_expression.exact_enclosing_capture |= exact_enclosing_capture
                    && is_descendant_owner(&external_expression.owner, self.owner);
                return Ok(*external);
            }
            return Err(OwnerSyntaxProjectionError::new(format!(
                "owner {:?} {context} references expression {expression} owned outside its permitted boundary by {:?}",
                self.owner, external_expression.owner
            )));
        }
        let external_owner = self
            .view
            .stable_check_owner_for_syntax_expression(expression)
            .ok_or_else(|| {
                OwnerSyntaxProjectionError::new(format!(
                    "owner {:?} {context} references unrouted expression {expression}",
                    self.owner
                ))
            })?;
        let permitted = is_descendant_owner(self.owner, &external_owner)
            || (allow_enclosing && is_descendant_owner(&external_owner, self.owner));
        if !permitted {
            return Err(OwnerSyntaxProjectionError::new(format!(
                "owner {:?} {context} references expression {expression} owned outside its permitted boundary by {external_owner:?}",
                self.owner
            )));
        }
        let stable_key = self
            .view
            .stable_expression_key_for_syntax(expression)
            .ok_or_else(|| {
                OwnerSyntaxProjectionError::new(format!(
                    "owner {:?} {context} references expression {expression} without a stable identity",
                    self.owner
                ))
            })?;
        let external_index = checked_u32(
            self.external_expressions.len(),
            "owner external expression count",
        )?;
        let local_count = checked_u32(self.local_by_syntax.len(), "owner expression count")?;
        let reference = local_count.checked_add(external_index).ok_or_else(|| {
            OwnerSyntaxProjectionError::new("owner expression namespace exceeds the u32 bound")
        })?;
        self.external_expressions
            .push(OwnerExternalExpressionInput {
                exact_enclosing_capture: exact_enclosing_capture
                    && is_descendant_owner(&external_owner, self.owner),
                owner: external_owner,
                expression: stable_key,
            });
        self.external_by_syntax.insert(expression, reference);
        Ok(reference)
    }

    fn mapped(
        &mut self,
        expression: usize,
        context: &str,
    ) -> Result<u32, OwnerSyntaxProjectionError> {
        self.mapped_with_enclosing(expression, context, false, false)
    }

    fn remap(
        &mut self,
        expression: &mut usize,
        context: &str,
    ) -> Result<(), OwnerSyntaxProjectionError> {
        *expression = self.mapped(*expression, context)? as usize;
        Ok(())
    }

    fn remap_optional(
        &mut self,
        expression: &mut Option<usize>,
        context: &str,
    ) -> Result<(), OwnerSyntaxProjectionError> {
        if let Some(expression) = expression {
            self.remap(expression, context)?;
        }
        Ok(())
    }

    fn remap_linkable(
        &mut self,
        expression: &mut usize,
        linked_input: Option<u32>,
        context: &str,
    ) -> Result<(), OwnerSyntaxProjectionError> {
        if let Some(linked_input) = linked_input {
            *expression = linked_input as usize;
            Ok(())
        } else {
            self.remap(expression, context)
        }
    }

    fn into_external_expressions(self) -> Vec<OwnerExternalExpressionInput> {
        self.external_expressions
    }
}

pub(crate) fn is_descendant_owner(
    parent: &StableCheckOwnerKey,
    candidate: &StableCheckOwnerKey,
) -> bool {
    if parent.source_unit_id() != candidate.source_unit_id() {
        return false;
    }
    match (parent, candidate) {
        (StableCheckOwnerKey::UnitRoot(_), StableCheckOwnerKey::Item(_)) => true,
        (StableCheckOwnerKey::Item(parent), StableCheckOwnerKey::Item(candidate)) => {
            let parent = parent.item_route.segments();
            let candidate = candidate.item_route.segments();
            candidate.len() > parent.len() && candidate.starts_with(parent)
        }
        _ => false,
    }
}

fn normalize_parameter(parameter: &mut AstParameter) {
    parameter.start = 0;
    parameter.end = 0;
}

fn normalize_argument(
    argument: &mut AstCallArg,
    expressions: &mut ExpressionProjection<'_>,
) -> Result<(), OwnerSyntaxProjectionError> {
    expressions.remap(&mut argument.value, "call argument")?;
    argument.start = 0;
    argument.end = 0;
    Ok(())
}

fn normalize_pass(
    pass: &mut AstPassContext,
    expressions: &mut ExpressionProjection<'_>,
) -> Result<(), OwnerSyntaxProjectionError> {
    expressions.remap(&mut pass.value, "PASS value")?;
    pass.start = 0;
    pass.end = 0;
    Ok(())
}

fn normalize_record_field(
    field: &mut AstRecordField,
    expressions: &mut ExpressionProjection<'_>,
) -> Result<(), OwnerSyntaxProjectionError> {
    expressions.remap(&mut field.value, "record field")?;
    field.start = 0;
    field.end = 0;
    Ok(())
}

fn normalize_block_binding(
    binding: &mut AstBlockBinding,
    statements: &BTreeMap<usize, u32>,
    child_statements: &BTreeMap<usize, u32>,
    expressions: &mut ExpressionProjection<'_>,
) -> Result<(), OwnerSyntaxProjectionError> {
    let AstBlockBindingDeclaration::Local { statement } = binding.declaration else {
        return Err(OwnerSyntaxProjectionError::new(format!(
            "owner {:?} received a pre-normalized child BLOCK declaration",
            expressions.owner
        )));
    };
    binding.declaration = if let Some(statement) = statements.get(&statement) {
        AstBlockBindingDeclaration::Local {
            statement: usize::try_from(*statement).expect("u32 owner statement id fits usize"),
        }
    } else if let Some(child) = child_statements.get(&statement) {
        AstBlockBindingDeclaration::Child {
            child: usize::try_from(*child).expect("u32 owner child id fits usize"),
        }
    } else {
        return Err(OwnerSyntaxProjectionError::new(format!(
            "owner {:?} block binding references statement {statement} outside its local and direct-child shards",
            expressions.owner
        )));
    };
    expressions.remap(&mut binding.value, "block binding value")?;
    binding.start = 0;
    binding.end = 0;
    Ok(())
}

fn normalize_expression_kind(
    kind: &mut AstExprKind,
    linked_input: Option<u32>,
    statements: &BTreeMap<usize, u32>,
    child_statements: &BTreeMap<usize, u32>,
    expressions: &mut ExpressionProjection<'_>,
) -> Result<(), OwnerSyntaxProjectionError> {
    match kind {
        AstExprKind::TextTemplate { segments } => {
            for segment in segments {
                if let AstTextSegment::Dynamic { value } = segment {
                    expressions.remap(value, "text interpolation")?;
                }
            }
        }
        AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
            for field in fields {
                normalize_record_field(field, expressions)?;
            }
        }
        AstExprKind::Flush { payload } => {
            expressions.remap_optional(payload, "FLUSH payload")?;
        }
        AstExprKind::Call { args, pass, .. } => {
            for argument in args {
                normalize_argument(argument, expressions)?;
            }
            if let Some(pass) = pass {
                normalize_pass(pass, expressions)?;
            }
        }
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => {
            expressions.remap_linkable(input, linked_input, "pipe input")?;
            for argument in args {
                normalize_argument(argument, expressions)?;
            }
            if let Some(pass) = pass {
                normalize_pass(pass, expressions)?;
            }
            for arm in arms {
                expressions.remap(arm, "pipe arm")?;
            }
        }
        AstExprKind::Draining { input } => {
            expressions.remap_linkable(input, linked_input, "DRAINING input")?;
        }
        AstExprKind::Hold { initial, .. } => {
            expressions.remap_linkable(initial, linked_input, "HOLD initial value")?;
        }
        AstExprKind::Latest { branches } => {
            for branch in branches {
                expressions.remap(branch, "LATEST branch")?;
            }
        }
        AstExprKind::When { input, arms } => {
            expressions.remap_linkable(input, linked_input, "WHEN input")?;
            for arm in arms {
                expressions.remap(arm, "WHEN arm")?;
            }
        }
        AstExprKind::Then { input, output } => {
            expressions.remap_linkable(input, linked_input, "THEN input")?;
            expressions.remap_optional(output, "THEN output")?;
        }
        AstExprKind::Infix { left, right, .. } => {
            expressions.remap_linkable(left, linked_input, "infix left operand")?;
            expressions.remap(right, "infix right operand")?;
        }
        AstExprKind::MatchArm { output, .. } => {
            expressions.remap_optional(output, "match-arm output")?;
        }
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                normalize_block_binding(binding, statements, child_statements, expressions)?;
            }
            expressions.remap_optional(result, "block result")?;
        }
        AstExprKind::ListLiteral { items, .. }
        | AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => {
            for item in items {
                expressions.remap(item, "collection item")?;
            }
        }
        AstExprKind::Arrow { left, output, .. } => {
            expressions.remap(left, "arrow left operand")?;
            expressions.remap_optional(output, "arrow output")?;
        }
        AstExprKind::MapEntry { key, value } => {
            expressions.remap(key, "map key")?;
            expressions.remap(value, "map value")?;
        }
        AstExprKind::MapLiteral { entries } => {
            for entry in entries {
                expressions.remap(entry, "map entry")?;
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_)
        | AstExprKind::BitsLiteral { .. } => {}
    }
    Ok(())
}

fn child_owner_key(
    parent: &StableCheckOwnerKey,
    route: boon_syntax::StableItemRoute,
) -> StableCheckOwnerKey {
    StableCheckOwnerKey::Item(StableOwnerKey {
        source_unit_id: parent.source_unit_id().clone(),
        item_route: route,
    })
}

fn owner_expression_contains_render_constructor(
    expression: usize,
    expressions: &[OwnerExpressionInput],
    seen: &mut std::collections::BTreeSet<usize>,
) -> bool {
    if !seen.insert(expression) {
        return false;
    }
    let Some(expression) = expressions.get(expression) else {
        return false;
    };
    let contains = |value, seen: &mut std::collections::BTreeSet<_>| {
        owner_expression_contains_render_constructor(value, expressions, seen)
    };
    match &expression.kind {
        AstExprKind::Call { function, args, .. } => {
            crate::is_registered_render_constructor(function)
                || args.iter().any(|argument| contains(argument.value, seen))
        }
        AstExprKind::Pipe {
            input, op, args, ..
        } => {
            crate::is_registered_render_constructor(op)
                || contains(*input, seen)
                || args.iter().any(|argument| contains(argument.value, seen))
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => contains(*initial, seen),
        AstExprKind::Then { input, output } => {
            contains(*input, seen) || output.is_some_and(|output| contains(output, seen))
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => contains(*output, seen),
        AstExprKind::Block { bindings, result } => {
            bindings.iter().any(|binding| contains(binding.value, seen))
                || result.is_some_and(|result| contains(result, seen))
        }
        AstExprKind::Infix { left, right, .. } => contains(*left, seen) || contains(*right, seen),
        AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => {
            fields.iter().any(|field| contains(field.value, seen))
        }
        AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::ListLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => items.iter().any(|item| contains(*item, seen)),
        AstExprKind::MapLiteral { entries } => entries.iter().any(|entry| contains(*entry, seen)),
        AstExprKind::MapEntry { key, value } => contains(*key, seen) || contains(*value, seen),
        AstExprKind::Arrow { left, output, .. } => {
            contains(*left, seen) || output.is_some_and(|output| contains(output, seen))
        }
        AstExprKind::TextTemplate { segments } => segments.iter().any(|segment| match segment {
            AstTextSegment::Static { .. } => false,
            AstTextSegment::Dynamic { value } => contains(*value, seen),
        }),
        AstExprKind::Flush {
            payload: Some(payload),
        } => contains(*payload, seen),
        AstExprKind::Tag(tag) if tag == "NoElement" => true,
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::BitsLiteral { .. }
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest { .. }
        | AstExprKind::MatchArm { output: None, .. }
        | AstExprKind::Delimiter
        | AstExprKind::Flush { payload: None }
        | AstExprKind::Unknown(_) => false,
    }
}

fn owner_statement_field(statement: &OwnerStatementInput) -> Option<&str> {
    match &statement.kind {
        AstStatementKind::Field { name } => Some(name),
        AstStatementKind::List {
            field: Some(name), ..
        } => Some(name),
        _ => None,
    }
}

fn classify_owner_statement_value_uses(
    statements: &mut [OwnerStatementInput],
    expressions: &[OwnerExpressionInput],
    inherited_render_context: bool,
) {
    fn contains_render_context(
        statement: usize,
        statements: &[OwnerStatementInput],
        expressions: &[OwnerExpressionInput],
        children: &[Vec<usize>],
        functions: &mut std::collections::BTreeSet<usize>,
    ) -> bool {
        let child_contains = children[statement].iter().copied().any(|child| {
            contains_render_context(child, statements, expressions, children, functions)
        });
        let field_contains = owner_statement_field(&statements[statement]).is_some_and(|field| {
            matches!(
                field,
                "document" | "scene" | "root" | "child" | "items" | "children"
            )
        });
        let expression_contains = statements[statement].expression.is_some_and(|expression| {
            owner_expression_contains_render_constructor(
                expression as usize,
                expressions,
                &mut std::collections::BTreeSet::new(),
            )
        });
        let contains = child_contains || field_contains || expression_contains;
        if contains
            && matches!(
                statements[statement].kind,
                AstStatementKind::Function { .. }
            )
        {
            functions.insert(statement);
        }
        contains
    }

    fn collect_slots(
        statement: usize,
        in_render_context: bool,
        statements: &mut [OwnerStatementInput],
        children: &[Vec<usize>],
        functions: &std::collections::BTreeSet<usize>,
    ) {
        let field = owner_statement_field(&statements[statement]).map(str::to_owned);
        let next_in_render_context = in_render_context
            || matches!(field.as_deref(), Some("document" | "scene"))
            || functions.contains(&statement);
        if next_in_render_context
            && matches!(
                field.as_deref(),
                Some("root" | "child" | "items" | "children")
            )
        {
            statements[statement].value_use = CheckedValueUse::RenderSlot;
        }
        for child in children[statement].iter().copied() {
            collect_slots(
                child,
                next_in_render_context,
                statements,
                children,
                functions,
            );
        }
    }

    let mut children = vec![Vec::new(); statements.len()];
    let mut roots = Vec::new();
    for (index, statement) in statements.iter().enumerate() {
        match statement.parent {
            Some(parent) if (parent as usize) < children.len() => {
                children[parent as usize].push(index)
            }
            _ => roots.push(index),
        }
    }
    let mut functions = std::collections::BTreeSet::new();
    for root in roots.iter().copied() {
        let _ = contains_render_context(root, statements, expressions, &children, &mut functions);
    }
    for root in roots {
        collect_slots(
            root,
            inherited_render_context,
            statements,
            &children,
            &functions,
        );
    }
}

fn containing_scope_is_render_context(containing_scope: &OwnerContainingScopeInput) -> bool {
    let OwnerContainingScopeInput::OwnerStatement { owner, statement } = containing_scope else {
        return false;
    };
    let owner_context = match owner {
        StableCheckOwnerKey::UnitRoot(_) => false,
        StableCheckOwnerKey::Item(owner) => owner.item_route.segments().iter().any(|segment| {
            segment.kind == UnitItemKind::Function
                || segment.names.iter().any(|name| {
                    matches!(
                        name.as_str(),
                        "document" | "scene" | "root" | "child" | "items" | "children"
                    )
                })
        }),
    };
    owner_context
        || statement
            .route
            .statement_route
            .iter()
            .flat_map(|segment| &segment.names)
            .any(|name| {
                matches!(
                    name.as_str(),
                    "document" | "scene" | "root" | "child" | "items" | "children"
                )
            })
}

pub fn project_owner_syntax_input(
    view: UnitOwnerSyntaxView<'_>,
) -> Result<OwnerSyntaxInput, OwnerSyntaxProjectionError> {
    let owner = view.stable_key();
    let containing_scope = match &owner {
        StableCheckOwnerKey::UnitRoot(_) => OwnerContainingScopeInput::ProjectRoot,
        StableCheckOwnerKey::Item(_) => {
            let root = *view.statement_ids().first().ok_or_else(|| {
                OwnerSyntaxProjectionError::new(format!(
                    "item owner {owner:?} has no root statement"
                ))
            })?;
            let parent = view
                .statement_locator(root)
                .ok_or_else(|| {
                    OwnerSyntaxProjectionError::new(format!(
                        "item owner {owner:?} has no root statement locator"
                    ))
                })?
                .parent();
            match parent {
                None => OwnerContainingScopeInput::ProjectRoot,
                Some(parent) => {
                    let statement = view.stable_statement_key_local(parent).ok_or_else(|| {
                        OwnerSyntaxProjectionError::new(format!(
                            "item owner {owner:?} has no stable containing statement"
                        ))
                    })?;
                    let parent_owner = statement.route.owner.clone().map_or_else(
                        || StableCheckOwnerKey::UnitRoot(owner.source_unit_id().clone()),
                        |item_route| {
                            StableCheckOwnerKey::Item(StableOwnerKey {
                                source_unit_id: owner.source_unit_id().clone(),
                                item_route,
                            })
                        },
                    );
                    OwnerContainingScopeInput::OwnerStatement {
                        owner: parent_owner,
                        statement,
                    }
                }
            }
        }
    };
    let mut statement_by_local = BTreeMap::<UnitLocalStatementId, u32>::new();
    let mut statement_by_syntax = BTreeMap::<usize, u32>::new();
    for (dense, (local, statement)) in view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
    {
        let dense = checked_u32(dense, "owner statement count")?;
        if statement_by_local.insert(local, dense).is_some()
            || statement_by_syntax.insert(statement.id, dense).is_some()
        {
            return Err(OwnerSyntaxProjectionError::new(format!(
                "owner {owner:?} has a duplicate statement identity"
            )));
        }
    }
    let mut child_statements = BTreeMap::<usize, u32>::new();
    for (child, boundary) in view.child_owners().iter().enumerate() {
        let child = checked_u32(child, "owner child count")?;
        let statement = view.child_owner_statement_id(boundary).ok_or_else(|| {
            OwnerSyntaxProjectionError::new(format!(
                "owner {owner:?} child {:?} has no authored statement",
                boundary.route
            ))
        })?;
        if child_statements.insert(statement, child).is_some() {
            return Err(OwnerSyntaxProjectionError::new(format!(
                "owner {owner:?} has duplicate child statement identity {statement}"
            )));
        }
    }

    let mut expression_by_syntax = BTreeMap::<usize, u32>::new();
    for (dense, expression) in view.expressions().enumerate() {
        let dense = checked_u32(dense, "owner expression count")?;
        if expression_by_syntax.insert(expression.id, dense).is_some() {
            return Err(OwnerSyntaxProjectionError::new(format!(
                "owner {owner:?} has a duplicate expression identity"
            )));
        }
    }
    let mut expression_projection = ExpressionProjection::new(&owner, view, &expression_by_syntax);

    let mut statements = Vec::with_capacity(view.statement_ids().len());
    for (local, statement) in view.statement_ids().iter().copied().zip(view.statements()) {
        let id = statement_by_local[&local];
        let locator = view.statement_locator(local).ok_or_else(|| {
            OwnerSyntaxProjectionError::new(format!(
                "owner {owner:?} statement {} has no parser locator",
                local.as_usize()
            ))
        })?;
        let parent = match locator.parent() {
            Some(parent) => statement_by_local.get(&parent).copied(),
            None => None,
        };
        let owner_root = matches!(view.route(), boon_syntax::UnitOwnerRoute::Item(_)) && id == 0;
        let child_index = if owner_root || (locator.parent().is_some() && parent.is_none()) {
            0
        } else {
            checked_u32(locator.child_index(), "owner statement child index")?
        };
        let mut kind = statement.kind.clone();
        if let AstStatementKind::Function { parameters, .. } = &mut kind {
            parameters.iter_mut().for_each(normalize_parameter);
        }
        let expression = statement
            .expr
            .map(|expression| expression_projection.mapped(expression, "statement value"))
            .transpose()?;
        statements.push(OwnerStatementInput {
            id,
            stable_key: view.stable_statement_key_local(local).ok_or_else(|| {
                OwnerSyntaxProjectionError::new(format!(
                    "owner {owner:?} statement {} has no stable key",
                    local.as_usize()
                ))
            })?,
            parent,
            child_index,
            kind,
            expression,
            value_use: CheckedValueUse::RuntimeValue,
        });
    }

    let mut expressions = Vec::with_capacity(view.expression_ids().len());
    for ((local, expression), stable_key) in view
        .expression_ids()
        .iter()
        .copied()
        .zip(view.expressions())
        .zip(view.stable_expression_keys())
    {
        debug_assert_eq!(
            view.stable_expression_key_local(local).as_ref(),
            Some(&stable_key)
        );
        let linked_input = expression
            .linked_input
            .map(|input| {
                expression_projection.mapped_with_enclosing(
                    input,
                    "linked input",
                    true,
                    matches!(expression.kind, AstExprKind::Hold { .. }),
                )
            })
            .transpose()?;
        let pattern_selector = matches!(expression.kind, AstExprKind::MatchArm { .. })
            .then(|| view.pattern_selector_for_syntax_expression(expression.id))
            .flatten()
            .map(|selector| {
                expression_projection.mapped_with_enclosing(
                    selector,
                    "pattern selector",
                    true,
                    true,
                )
            })
            .transpose()?;
        let mut kind = expression.kind.clone();
        normalize_expression_kind(
            &mut kind,
            linked_input,
            &statement_by_syntax,
            &child_statements,
            &mut expression_projection,
        )?;
        expressions.push(OwnerExpressionInput {
            stable_key,
            linked_input,
            pattern_selector,
            kind,
        });
    }
    classify_owner_statement_value_uses(
        &mut statements,
        &expressions,
        containing_scope_is_render_context(&containing_scope),
    );
    let mut child_owners = Vec::with_capacity(view.child_owners().len());
    for boundary in view.child_owners() {
        let parent = boundary
            .parent()
            .map(|parent| {
                statement_by_local.get(&parent).copied().ok_or_else(|| {
                    OwnerSyntaxProjectionError::new(format!(
                        "owner {owner:?} child boundary has a parent outside its shard"
                    ))
                })
            })
            .transpose()?;
        let child_owner = child_owner_key(&owner, boundary.route.clone());
        let child_result = view.child_owner_result_expression(boundary);
        let result_expression = child_result
            .map(|expression| {
                // The containing owner consumes the child's frozen public
                // result just like any other descendant expression. Intern it
                // in the compact external-expression namespace even when no
                // retained local expression points at it directly.
                expression_projection.mapped(expression, "child-owner public result")?;
                view.stable_expression_key_for_syntax(expression)
                    .ok_or_else(|| {
                        OwnerSyntaxProjectionError::new(format!(
                            "owner {owner:?} child {child_owner:?} has no stable public result expression"
                        ))
                    })
            })
            .transpose()?;
        let boundary_expression = view.child_owner_boundary_expression(boundary);
        let result_placement = match child_result {
            None => OwnerChildResultPlacementInput::Valueless,
            Some(_) if parent.is_none() => OwnerChildResultPlacementInput::StatementLane,
            Some(expression) => view
                .stable_expression_boundary_parent_edge_for_syntax(expression)
                .map(
                    |(child_owner, child_expression, owner, expression, segment)| {
                        OwnerChildResultPlacementInput::ExpressionEdge {
                            edge: OwnerExpressionParentEdgeInput {
                                child_owner,
                                child_expression,
                                owner,
                                expression,
                                segment,
                            },
                        }
                    },
                )
                .unwrap_or(OwnerChildResultPlacementInput::StatementLane),
        };
        let boundary_expression = boundary_expression
            .map(|expression| {
                view.stable_expression_key_for_syntax(expression)
                    .ok_or_else(|| {
                        OwnerSyntaxProjectionError::new(format!(
                            "owner {owner:?} child {child_owner:?} has no stable boundary expression"
                        ))
                    })
            })
            .transpose()?;
        child_owners.push(OwnerChildInput {
            owner: child_owner,
            parent,
            child_index: checked_u32(boundary.child_index(), "child-owner position")?,
            boundary_expression,
            result_expression,
            result_placement,
        });
    }
    let external_expressions = expression_projection.into_external_expressions();

    let fingerprint_v1 = fingerprint_serialized(
        OWNER_SYNTAX_FINGERPRINT_DOMAIN_V2,
        &(
            &owner,
            &containing_scope,
            &statements,
            &expressions,
            &external_expressions,
            &child_owners,
        ),
    )?;
    Ok(OwnerSyntaxInput {
        owner,
        containing_scope,
        statements: statements.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        external_expressions: external_expressions.into_boxed_slice(),
        child_owners: child_owners.into_boxed_slice(),
        fingerprint_v1,
    })
}

pub fn project_owner_source_map(
    view: UnitOwnerSyntaxView<'_>,
) -> Result<OwnerSourceMap, OwnerSyntaxProjectionError> {
    let owner = view.stable_key();
    let physical_line = |start: usize, label: &str| {
        view.physical_line_for_byte(start).ok_or_else(|| {
            OwnerSyntaxProjectionError::new(format!(
                "owner {owner:?} {label} start {start} is outside the parsed line table"
            ))
        })
    };
    let mut anchors = Vec::new();
    let mut statements = Vec::with_capacity(view.statement_ids().len());
    for (statement, (local, syntax)) in view
        .statement_ids()
        .iter()
        .copied()
        .zip(view.statements())
        .enumerate()
    {
        let statement = checked_u32(statement, "owner source statement count")?;
        statements.push(OwnerStatementSource {
            statement,
            stable_key: view.stable_statement_key_local(local).ok_or_else(|| {
                OwnerSyntaxProjectionError::new(format!(
                    "owner {owner:?} statement {} has no stable source key",
                    local.as_usize()
                ))
            })?,
            line: checked_u64(physical_line(syntax.start, "statement")?, "statement line")?,
            start: checked_u64(syntax.start, "statement start")?,
            end: checked_u64(syntax.end, "statement end")?,
        });
        if let AstStatementKind::Function { parameters, .. } = &syntax.kind {
            for parameter in parameters {
                anchors.push(OwnerSourceAnchor {
                    site: OwnerSourceAnchorSite::Statement { statement },
                    role: OwnerSourceAnchorRole::FunctionParameter {
                        ordinal: checked_u32(parameter.ordinal, "function parameter ordinal")?,
                    },
                    line: checked_u64(
                        physical_line(parameter.start, "function parameter")?,
                        "function parameter line",
                    )?,
                    start: checked_u64(parameter.start, "function parameter start")?,
                    end: checked_u64(parameter.end, "function parameter end")?,
                });
            }
        }
    }
    let mut expressions = Vec::with_capacity(view.expression_ids().len());
    for (syntax, expression) in view.expressions().zip(view.stable_expression_keys()) {
        let expression_site = OwnerSourceAnchorSite::Expression {
            expression: expression.clone(),
        };
        expressions.push(OwnerExpressionSource {
            expression,
            line: checked_u64(
                physical_line(syntax.start, "expression")?,
                "expression line",
            )?,
            start: checked_u64(syntax.start, "expression start")?,
            end: checked_u64(syntax.end, "expression end")?,
        });
        let mut push_anchor = |role: OwnerSourceAnchorRole,
                               line: usize,
                               start: usize,
                               end: usize|
         -> Result<(), OwnerSyntaxProjectionError> {
            anchors.push(OwnerSourceAnchor {
                site: expression_site.clone(),
                role,
                line: checked_u64(line, "expression subspan line")?,
                start: checked_u64(start, "expression subspan start")?,
                end: checked_u64(end, "expression subspan end")?,
            });
            Ok(())
        };
        match &syntax.kind {
            AstExprKind::Call { args, pass, .. } => {
                for (ordinal, argument) in args.iter().enumerate() {
                    push_anchor(
                        OwnerSourceAnchorRole::CallArgument {
                            ordinal: checked_u32(ordinal, "call argument ordinal")?,
                        },
                        physical_line(argument.start, "call argument")?,
                        argument.start,
                        argument.end,
                    )?;
                }
                if let Some(pass) = pass {
                    push_anchor(
                        OwnerSourceAnchorRole::CallPass,
                        physical_line(pass.start, "call PASS")?,
                        pass.start,
                        pass.end,
                    )?;
                }
            }
            AstExprKind::Pipe { args, pass, .. } => {
                for (ordinal, argument) in args.iter().enumerate() {
                    push_anchor(
                        OwnerSourceAnchorRole::PipeArgument {
                            ordinal: checked_u32(ordinal, "pipe argument ordinal")?,
                        },
                        physical_line(argument.start, "pipe argument")?,
                        argument.start,
                        argument.end,
                    )?;
                }
                if let Some(pass) = pass {
                    push_anchor(
                        OwnerSourceAnchorRole::PipePass,
                        physical_line(pass.start, "pipe PASS")?,
                        pass.start,
                        pass.end,
                    )?;
                }
            }
            AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
                for (ordinal, field) in fields.iter().enumerate() {
                    push_anchor(
                        OwnerSourceAnchorRole::RecordField {
                            ordinal: checked_u32(ordinal, "record field ordinal")?,
                        },
                        physical_line(field.start, "record field")?,
                        field.start,
                        field.end,
                    )?;
                }
            }
            AstExprKind::Block { bindings, .. } => {
                for (ordinal, binding) in bindings.iter().enumerate() {
                    push_anchor(
                        OwnerSourceAnchorRole::BlockBinding {
                            ordinal: checked_u32(ordinal, "block binding ordinal")?,
                        },
                        physical_line(binding.start, "BLOCK binding")?,
                        binding.start,
                        binding.end,
                    )?;
                }
            }
            AstExprKind::Identifier(_)
            | AstExprKind::Path(_)
            | AstExprKind::Drain { .. }
            | AstExprKind::StringLiteral(_)
            | AstExprKind::TextLiteral(_)
            | AstExprKind::TextTemplate { .. }
            | AstExprKind::Number(_)
            | AstExprKind::ByteLiteral { .. }
            | AstExprKind::Tag(_)
            | AstExprKind::Flush { .. }
            | AstExprKind::Source
            | AstExprKind::Draining { .. }
            | AstExprKind::Hold { .. }
            | AstExprKind::Latest { .. }
            | AstExprKind::When { .. }
            | AstExprKind::Then { .. }
            | AstExprKind::Infix { .. }
            | AstExprKind::MatchArm { .. }
            | AstExprKind::ListLiteral { .. }
            | AstExprKind::BytesLiteral { .. }
            | AstExprKind::Delimiter
            | AstExprKind::Unknown(_)
            | AstExprKind::Arrow { .. }
            | AstExprKind::MapEntry { .. }
            | AstExprKind::MapLiteral { .. }
            | AstExprKind::SetLiteral { .. }
            | AstExprKind::BitsLiteral { .. } => {}
        }
    }
    anchors.sort_by(|left, right| (&left.site, left.role).cmp(&(&right.site, right.role)));
    if anchors
        .windows(2)
        .any(|pair| pair[0].site == pair[1].site && pair[0].role == pair[1].role)
    {
        return Err(OwnerSyntaxProjectionError::new(format!(
            "owner {owner:?} has duplicate source anchors"
        )));
    }
    let path = view.path().to_owned();
    let fingerprint_v2 = fingerprint_serialized(
        OWNER_SOURCE_MAP_FINGERPRINT_DOMAIN_V2,
        &(&owner, &path, &statements, &expressions, &anchors),
    )?;
    Ok(OwnerSourceMap {
        owner,
        path,
        statements: statements.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        anchors: anchors.into_boxed_slice(),
        fingerprint_v2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_parser::{
        ParsedSourceUnit, UnitSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys,
    };
    use std::collections::BTreeSet;

    fn link(parsed: ParsedSourceUnit) -> UnitSyntaxSnapshot {
        let key = project_unit_link_keys(
            "app/RUN.bn",
            [(
                parsed.source_unit_id.clone(),
                parsed.declared_functions.clone(),
            )],
        )
        .unwrap()
        .remove(&parsed.source_unit_id)
        .unwrap();
        parsed.into_unit_syntax_snapshot(key).unwrap()
    }

    fn owner_named(unit: &UnitSyntaxSnapshot, name: &str) -> StableCheckOwnerKey {
        unit.stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == [name])
                )
            })
            .unwrap()
    }

    #[test]
    fn owner_syntax_fingerprint_ignores_spans_and_unrelated_owner_bodies() {
        let before = link(
            parse_project_source_unit(
                "app/RUN.bn",
                "left: 1\nright: helper(input: 2)\nFUNCTION helper(input) {\n    input + 1\n}\n",
            )
            .unwrap(),
        );
        let after = link(
            parse_project_source_unit(
                "app/RUN.bn",
                "earlier: 999\nleft : 1\nright: helper(input: 3)\nFUNCTION helper(input) {\n    input + 1\n}\n",
            )
            .unwrap(),
        );

        let before_helper = owner_named(&before, "helper");
        let after_helper = owner_named(&after, "helper");
        assert_eq!(before_helper, after_helper);
        let before_input =
            project_owner_syntax_input(before.owner_view_for_key(&before_helper).unwrap()).unwrap();
        let after_input =
            project_owner_syntax_input(after.owner_view_for_key(&after_helper).unwrap()).unwrap();
        assert_eq!(before_input.fingerprint_v1(), after_input.fingerprint_v1());
        assert_eq!(before_input, after_input);

        let before_map =
            project_owner_source_map(before.owner_view_for_key(&before_helper).unwrap()).unwrap();
        let after_map =
            project_owner_source_map(after.owner_view_for_key(&after_helper).unwrap()).unwrap();
        assert_ne!(before_map.fingerprint_v2(), after_map.fingerprint_v2());

        let before_right = owner_named(&before, "right");
        let after_right = owner_named(&after, "right");
        assert_ne!(
            project_owner_syntax_input(before.owner_view_for_key(&before_right).unwrap())
                .unwrap()
                .fingerprint_v1(),
            project_owner_syntax_input(after.owner_view_for_key(&after_right).unwrap())
                .unwrap()
                .fingerprint_v1()
        );
    }

    #[test]
    fn owner_source_map_changes_without_changing_semantic_input() {
        let compact = link(parse_project_source_unit("app/RUN.bn", "value: 1\n").unwrap());
        let spaced = link(parse_project_source_unit("app/RUN.bn", "value    :    1\n").unwrap());
        let compact_owner = owner_named(&compact, "value");
        let spaced_owner = owner_named(&spaced, "value");
        assert_eq!(compact_owner, spaced_owner);
        assert_eq!(
            project_owner_syntax_input(compact.owner_view_for_key(&compact_owner).unwrap())
                .unwrap()
                .fingerprint_v1(),
            project_owner_syntax_input(spaced.owner_view_for_key(&spaced_owner).unwrap())
                .unwrap()
                .fingerprint_v1()
        );
        assert_ne!(
            project_owner_source_map(compact.owner_view_for_key(&compact_owner).unwrap())
                .unwrap()
                .fingerprint_v2(),
            project_owner_source_map(spaced.owner_view_for_key(&spaced_owner).unwrap())
                .unwrap()
                .fingerprint_v2()
        );
    }

    #[test]
    fn owner_source_map_retains_exact_diagnostic_subspans() {
        let source = concat!(
            "FUNCTION helper(input, output: OUT) {\n",
            "    input\n",
            "}\n",
            "FUNCTION subject(\n",
            "    input\n",
            "    output: OUT\n",
            ") {\n",
            "    BLOCK {\n",
            "        record: [\n",
            "            value:\n",
            "                input\n",
            "        ]\n",
            "        called:\n",
            "            helper(\n",
            "                input:\n",
            "                    record\n",
            "                PASS:\n",
            "                    [theme: input]\n",
            "            )\n",
            "        piped:\n",
            "            input\n",
            "            |> helper(\n",
            "                output\n",
            "                PASS:\n",
            "                    [theme: input]\n",
            "            )\n",
            "        piped\n",
            "    }\n",
            "}\n",
        );
        let unit = link(parse_project_source_unit("app/RUN.bn", source).unwrap());
        let owner = owner_named(&unit, "subject");
        let source_map =
            project_owner_source_map(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let roles = source_map
            .anchors
            .iter()
            .map(|anchor| anchor.role)
            .collect::<BTreeSet<_>>();
        assert!(roles.contains(&OwnerSourceAnchorRole::FunctionParameter { ordinal: 0 }));
        assert!(roles.contains(&OwnerSourceAnchorRole::FunctionParameter { ordinal: 1 }));
        assert!(roles.contains(&OwnerSourceAnchorRole::CallArgument { ordinal: 0 }));
        assert!(roles.contains(&OwnerSourceAnchorRole::CallPass));
        assert!(roles.contains(&OwnerSourceAnchorRole::PipeArgument { ordinal: 0 }));
        assert!(roles.contains(&OwnerSourceAnchorRole::PipePass));
        assert!(roles.contains(&OwnerSourceAnchorRole::RecordField { ordinal: 0 }));
        assert!(roles.contains(&OwnerSourceAnchorRole::BlockBinding { ordinal: 0 }));
        assert_eq!(source_map.path(), "app/RUN.bn");
        let physical_line = |start: u64| {
            1 + source.as_bytes()[..usize::try_from(start).unwrap()]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count() as u64
        };
        for statement in source_map.statements() {
            assert!(statement.start < statement.end);
            assert!(usize::try_from(statement.end).unwrap() <= source.len());
            assert_eq!(statement.line, physical_line(statement.start));
        }
        for expression in source_map.expressions() {
            assert!(expression.start < expression.end);
            assert!(usize::try_from(expression.end).unwrap() <= source.len());
            assert_eq!(expression.line, physical_line(expression.start));
        }
        for anchor in &source_map.anchors {
            assert!(anchor.start < anchor.end);
            assert!(usize::try_from(anchor.end).unwrap() <= source.len());
            assert_eq!(anchor.line, physical_line(anchor.start));
        }
        assert!(
            source_map
                .anchors
                .windows(2)
                .all(|pair| { (&pair[0].site, pair[0].role) < (&pair[1].site, pair[1].role) })
        );
    }

    #[test]
    fn block_binding_retains_exact_child_declaration_and_parent_edge() {
        let unit = link(
            parse_project_source_unit(
                "app/RUN.bn",
                "container: BLOCK {\n    child: [value: 1]\n    child\n}\n",
            )
            .unwrap(),
        );
        let container = owner_named(&unit, "container");
        let child = owner_named(&unit, "child");
        let input =
            project_owner_syntax_input(unit.owner_view_for_key(&container).unwrap()).unwrap();

        assert_eq!(input.child_owners.len(), 1);
        assert_eq!(input.child_owners[0].owner, child);
        let block = input
            .expressions
            .iter()
            .find(|expression| matches!(&expression.kind, AstExprKind::Block { .. }))
            .expect("container BLOCK expression");
        let AstExprKind::Block { bindings, .. } = &block.kind else {
            unreachable!();
        };
        let binding = bindings
            .iter()
            .find(|binding| binding.name == "child")
            .expect("child BLOCK binding");
        assert_eq!(
            binding.declaration,
            AstBlockBindingDeclaration::Child { child: 0 }
        );
        let external = input
            .external_expression(binding.value)
            .expect("child public result reference");
        assert_eq!(external.owner, child);
        assert_eq!(
            input.child_owners[0].result_expression.as_ref(),
            Some(&external.expression)
        );

        let OwnerChildResultPlacementInput::ExpressionEdge { edge } =
            &input.child_owners[0].result_placement
        else {
            panic!("child result must have an expression edge");
        };
        assert_eq!(edge.owner, container);
        assert_eq!(edge.expression, block.stable_key);
        assert_eq!(
            edge.segment.role,
            boon_syntax::StableExpressionChildRole::BlockBinding
        );
        assert_eq!(edge.segment.label.as_deref(), Some("child"));
        assert_eq!(edge.segment.matching_sibling_reverse_ordinal, 0);
    }

    #[test]
    fn nested_functions_are_valueless_in_enclosing_block_and_list_structure() {
        let unit = link(
            parse_project_source_unit(
                "app/RUN.bn",
                concat!(
                    "container: BLOCK {\n",
                    "    FUNCTION helper_block() { 1 }\n",
                    "}\n",
                    "items: LIST {\n",
                    "    FUNCTION helper_list() { 2 }\n",
                    "}\n",
                ),
            )
            .unwrap(),
        );

        let container = owner_named(&unit, "container");
        let container =
            project_owner_syntax_input(unit.owner_view_for_key(&container).unwrap()).unwrap();
        let helper = container
            .child_owners
            .iter()
            .find(|child| {
                matches!(
                    &child.owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| {
                            segment.names.as_ref() == ["helper_block"]
                        })
                )
            })
            .expect("nested block function boundary");
        assert_eq!(helper.result_expression, None);
        assert_eq!(
            helper.result_placement,
            OwnerChildResultPlacementInput::Valueless
        );
        let block = container
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::Block { bindings, result } => Some((bindings, result)),
                _ => None,
            })
            .expect("container BLOCK");
        assert!(block.0.is_empty());
        assert_eq!(*block.1, None);

        let items = owner_named(&unit, "items");
        let items = project_owner_syntax_input(unit.owner_view_for_key(&items).unwrap()).unwrap();
        let helper = items
            .child_owners
            .iter()
            .find(|child| {
                matches!(
                    &child.owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| {
                            segment.names.as_ref() == ["helper_list"]
                        })
                )
            })
            .expect("nested list function boundary");
        assert_eq!(helper.result_expression, None);
        assert_eq!(
            helper.result_placement,
            OwnerChildResultPlacementInput::Valueless
        );
        let list = items
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::ListLiteral { items, .. } => Some(items),
                _ => None,
            })
            .expect("items LIST");
        assert!(list.is_empty());
    }

    #[test]
    fn child_parent_edge_is_derived_from_the_public_pipeline_result() {
        let unit = link(
            parse_project_source_unit(
                "app/RUN.bn",
                concat!(
                    "container: BLOCK {\n",
                    "    child:\n",
                    "        TEXT { hello }\n",
                    "        |> Text/trim()\n",
                    "    child\n",
                    "}\n",
                ),
            )
            .unwrap(),
        );
        let container = owner_named(&unit, "container");
        let input =
            project_owner_syntax_input(unit.owner_view_for_key(&container).unwrap()).unwrap();
        let child = input.child_owners.first().expect("pipeline child boundary");
        assert_ne!(child.boundary_expression, child.result_expression);
        let OwnerChildResultPlacementInput::ExpressionEdge { edge } = &child.result_placement
        else {
            panic!("pipeline child result must retain an expression edge");
        };
        assert!(
            child.boundary_expression.as_ref() == Some(&edge.child_expression)
                || child.result_expression.as_ref() == Some(&edge.child_expression)
        );
    }

    #[test]
    fn parent_owner_references_child_by_stable_identity_not_child_body() {
        let before = link(
            parse_project_source_unit("app/RUN.bn", "container: [\n    child: 1\n]\n").unwrap(),
        );
        let after = link(
            parse_project_source_unit("app/RUN.bn", "container: [\n    child: 999\n]\n").unwrap(),
        );
        let before_container = owner_named(&before, "container");
        let after_container = owner_named(&after, "container");
        let before_child = owner_named(&before, "child");
        let after_child = owner_named(&after, "child");
        assert_eq!(before_container, after_container);
        assert_eq!(before_child, after_child);

        let before_parent =
            project_owner_syntax_input(before.owner_view_for_key(&before_container).unwrap())
                .unwrap();
        let after_parent =
            project_owner_syntax_input(after.owner_view_for_key(&after_container).unwrap())
                .unwrap();
        assert_eq!(before_parent, after_parent);
        assert_eq!(before_parent.external_expressions.len(), 1);
        assert_eq!(before_parent.external_expressions[0].owner, before_child);
        let external_reference = before_parent
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::Object(fields) => fields.first().map(|field| field.value),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            before_parent
                .external_expression(external_reference)
                .unwrap()
                .owner,
            before_child
        );

        let before_child =
            project_owner_syntax_input(before.owner_view_for_key(&before_child).unwrap()).unwrap();
        let after_child =
            project_owner_syntax_input(after.owner_view_for_key(&after_child).unwrap()).unwrap();
        assert_ne!(before_child.fingerprint_v1(), after_child.fingerprint_v1());
    }
}
