use boon_parser::UnitOwnerSyntaxView;
use boon_syntax::{
    AstBlockBinding, AstCallArg, AstExprKind, AstParameter, AstPassContext, AstRecordField,
    AstStatementKind, AstTextSegment, StableCheckOwnerKey, StableExpressionKey, StableOwnerKey,
    UnitLocalStatementId,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};

const OWNER_KEY_FINGERPRINT_DOMAIN_V1: &[u8] = b"boon.check-owner-key.v1\0";
const OWNER_SYNTAX_FINGERPRINT_DOMAIN_V1: &[u8] = b"boon.owner-syntax-input.v1\0";
const OWNER_SOURCE_MAP_FINGERPRINT_DOMAIN_V1: &[u8] = b"boon.owner-source-map.v1\0";

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
    pub parent: Option<u32>,
    pub child_index: u32,
    pub kind: AstStatementKind,
    pub expression: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExpressionInput {
    pub stable_key: StableExpressionKey,
    pub linked_input: Option<u32>,
    pub kind: AstExprKind,
}

/// A reference from this owner's retained syntax into a descendant owner.
///
/// The syntax expression identity and its owning shard are both stable across
/// revisions. The child body is deliberately absent, so changing that body
/// cannot change the parent owner's syntax fingerprint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExternalExpressionInput {
    pub owner: StableCheckOwnerKey,
    pub expression: StableExpressionKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerChildInput {
    pub owner: StableCheckOwnerKey,
    pub parent: Option<u32>,
    pub child_index: u32,
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

/// Current source positions for one owner, fingerprinted independently from
/// its semantics so formatting edits cannot invalidate checked body results.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSourceMap {
    pub owner: StableCheckOwnerKey,
    pub path: String,
    pub statements: Box<[OwnerStatementSource]>,
    pub expressions: Box<[OwnerExpressionSource]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerSourceMap {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
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

    fn mapped(
        &mut self,
        expression: usize,
        context: &str,
    ) -> Result<u32, OwnerSyntaxProjectionError> {
        if let Some(local) = self.local_by_syntax.get(&expression) {
            return Ok(*local);
        }
        if let Some(external) = self.external_by_syntax.get(&expression) {
            return Ok(*external);
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
        if !is_descendant_owner(self.owner, &external_owner) {
            return Err(OwnerSyntaxProjectionError::new(format!(
                "owner {:?} {context} references expression {expression} owned outside its descendants by {external_owner:?}",
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
                owner: external_owner,
                expression: stable_key,
            });
        self.external_by_syntax.insert(expression, reference);
        Ok(reference)
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

fn is_descendant_owner(parent: &StableCheckOwnerKey, candidate: &StableCheckOwnerKey) -> bool {
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
    expressions: &mut ExpressionProjection<'_>,
) -> Result<(), OwnerSyntaxProjectionError> {
    binding.statement = usize::try_from(*statements.get(&binding.statement).ok_or_else(|| {
        OwnerSyntaxProjectionError::new(format!(
            "owner {:?} block binding references statement {} outside its shard",
            expressions.owner, binding.statement
        ))
    })?)
    .expect("u32 owner statement id fits usize");
    expressions.remap(&mut binding.value, "block binding value")?;
    binding.start = 0;
    binding.end = 0;
    Ok(())
}

fn normalize_expression_kind(
    kind: &mut AstExprKind,
    linked_input: Option<u32>,
    statements: &BTreeMap<usize, u32>,
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
                normalize_block_binding(binding, statements, expressions)?;
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

pub fn project_owner_syntax_input(
    view: UnitOwnerSyntaxView<'_>,
) -> Result<OwnerSyntaxInput, OwnerSyntaxProjectionError> {
    let owner = view.stable_key();
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
            parent,
            child_index,
            kind,
            expression,
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
            .map(|input| expression_projection.mapped(input, "linked input"))
            .transpose()?;
        let mut kind = expression.kind.clone();
        normalize_expression_kind(
            &mut kind,
            linked_input,
            &statement_by_syntax,
            &mut expression_projection,
        )?;
        expressions.push(OwnerExpressionInput {
            stable_key,
            linked_input,
            kind,
        });
    }
    let external_expressions = expression_projection.into_external_expressions();

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
        child_owners.push(OwnerChildInput {
            owner: child_owner_key(&owner, boundary.route.clone()),
            parent,
            child_index: checked_u32(boundary.child_index(), "child-owner position")?,
        });
    }

    let fingerprint_v1 = fingerprint_serialized(
        OWNER_SYNTAX_FINGERPRINT_DOMAIN_V1,
        &(
            &owner,
            &statements,
            &expressions,
            &external_expressions,
            &child_owners,
        ),
    )?;
    Ok(OwnerSyntaxInput {
        owner,
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
    let mut statements = Vec::with_capacity(view.statement_ids().len());
    for (statement, syntax) in view.statements().enumerate() {
        statements.push(OwnerStatementSource {
            statement: checked_u32(statement, "owner source statement count")?,
            line: checked_u64(syntax.line, "statement line")?,
            start: checked_u64(syntax.start, "statement start")?,
            end: checked_u64(syntax.end, "statement end")?,
        });
    }
    let mut expressions = Vec::with_capacity(view.expression_ids().len());
    for (syntax, expression) in view.expressions().zip(view.stable_expression_keys()) {
        expressions.push(OwnerExpressionSource {
            expression,
            line: checked_u64(syntax.line, "expression line")?,
            start: checked_u64(syntax.start, "expression start")?,
            end: checked_u64(syntax.end, "expression end")?,
        });
    }
    let path = view.path().to_owned();
    let fingerprint_v1 = fingerprint_serialized(
        OWNER_SOURCE_MAP_FINGERPRINT_DOMAIN_V1,
        &(&owner, &path, &statements, &expressions),
    )?;
    Ok(OwnerSourceMap {
        owner,
        path,
        statements: statements.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_parser::{
        ParsedSourceUnit, UnitSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys,
    };

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
        assert_ne!(before_map.fingerprint_v1(), after_map.fingerprint_v1());

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
                .fingerprint_v1(),
            project_owner_source_map(spaced.owner_view_for_key(&spaced_owner).unwrap())
                .unwrap()
                .fingerprint_v1()
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
