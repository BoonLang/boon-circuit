use boon_contract::{
    CanonicalSourceBundleV1, SourceBundleDigestV1, SourceBundleError, SourceBundleUnit,
};
#[cfg(test)]
use boon_syntax::ProgramRoleRoot;
use boon_syntax::{
    __parser_pack_syntax_node_id, __parser_unpack_syntax_node_id, AstBlockBinding,
    AstBlockBindingDeclaration, AstCallArg, AstCallArgKind, AstDrainPath, AstExpr, AstExprKind,
    AstMatchPattern, AstParameter, AstParameterKind, AstPassContext, AstProgram, AstRecordField,
    AstStatement, AstStatementKind, AstTextSegment, AstToken, AstTokenKind, BytesSizeSyntax,
    DocumentAst, LANGUAGE_FEATURE_REGISTRY, LanguageFeatureParseExpectation, LanguageFeatureStage,
    ParsedProgramFields, ParsedSourceFile, ParsedSourceUnitFields, ParserItem, ParserLine,
    ProgramKind, SourceUnitId, StableCheckOwnerKey, StableDefinitionKey, StableExpressionChildRole,
    StableExpressionKey, StableExpressionRouteSegment, StableItemRoute, StableItemRouteSegment,
    StableOccurrenceKey, StableOccurrenceRoute, StableOwnerKey, StableStatementKey,
    StableStatementKind, StableStatementRoute, StableStatementRouteSegment, SyntaxUnitNamespace,
    UnitCheckOwnerSlot, UnitChildOwnerBoundary, UnitExpressionParentEdge, UnitItemIndex,
    UnitItemIndexEntry, UnitItemKind, UnitItemParameter, UnitLocalExpressionId,
    UnitLocalStatementId, UnitOwnerIndex, UnitOwnerIndexEntry, UnitOwnerRoute,
    UnitStatementLocator, is_program_role_root, is_reserved_standard_root,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Clone, Copy)]
struct ParserTrace {
    enabled: bool,
}

impl ParserTrace {
    fn from_environment() -> Self {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        Self {
            enabled: *ENABLED.get_or_init(|| std::env::var_os("BOON_PARSER_TRACE").is_some()),
        }
    }

    #[inline]
    fn start(self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    #[inline]
    fn phase(
        self,
        scope: &str,
        phase: &str,
        started: Option<Instant>,
        counters: impl FnOnce() -> String,
    ) {
        let Some(started) = started else {
            return;
        };
        eprintln!(
            "boon_parser {scope} {phase}: {:.3}ms {}",
            started.elapsed().as_secs_f64() * 1_000.0,
            counters(),
        );
    }
}

/// Opaque parser-produced syntax artifact.
///
/// Its fields remain readable through [`boon_syntax::ParsedProgramFields`], but callers
/// cannot mutate them through this wrapper or construct an accepted
/// `ParsedProgram` from deserialized fields.
///
/// ```compile_fail
/// use boon_parser::ParsedProgram;
/// use boon_syntax::ParsedProgramFields;
///
/// let fields: ParsedProgramFields = serde_json::from_str("{}").unwrap();
/// let _ = ParsedProgram { fields };
/// ```
///
/// ```compile_fail
/// use boon_parser::ParsedProgram;
///
/// let _: ParsedProgram = serde_json::from_str("{}").unwrap();
/// ```
///
/// ```compile_fail
/// use boon_parser::ParsedProgram;
///
/// let _: ParsedProgram = Default::default();
/// ```
///
/// ```compile_fail
/// let mut parsed = boon_parser::parse_source("main.bn", "value: 1").unwrap();
/// parsed.path.clear();
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedProgram {
    fields: std::sync::Arc<ParsedProgramFields>,
}

impl std::ops::Deref for ParsedProgram {
    type Target = ParsedProgramFields;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl ParsedProgram {
    fn from_parser_fields(fields: ParsedProgramFields) -> Self {
        Self {
            fields: std::sync::Arc::new(fields),
        }
    }

    pub fn stable_occurrence_key(&self, expression_id: usize) -> Option<StableOccurrenceKey> {
        let route = self.occurrence_routes.get(expression_id)?.clone()?;
        let expression = self.ast.expressions.get(expression_id)?;
        let source_unit = self
            .files
            .iter()
            .filter(|file| file.start_line <= expression.line)
            .max_by_key(|file| file.start_line)?
            .source_unit_id()
            .ok()?;
        Some(StableOccurrenceKey {
            source_unit_id: source_unit,
            route,
        })
    }

    pub fn stable_expression_key(&self, expression_id: usize) -> Option<StableExpressionKey> {
        let route_digest_v1 = *self
            .expression_route_digests_v1
            .get(expression_id)?
            .as_ref()?;
        let expression = self.ast.expressions.get(expression_id)?;
        let source_unit_id = self
            .files
            .iter()
            .filter(|file| file.start_line <= expression.line)
            .max_by_key(|file| file.start_line)?
            .source_unit_id()
            .ok()?;
        Some(StableExpressionKey {
            source_unit_id,
            route_digest_v1,
        })
    }
}

impl Serialize for ParsedProgram {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.fields.serialize(serializer)
    }
}

/// Context-independent syntax artifact for one canonical source unit.
///
/// The path and [`boon_syntax::SourceUnitId`] are the exact normalized project-relative
/// identity of the unit. All token, statement, expression, line, and byte
/// positions in `ast` are local to `source`; no module role or project-global
/// rebasing has been applied. Function names are retained exactly as declared
/// so project assembly can apply its context once, after cache lookup.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ParsedSourceUnit {
    #[serde(flatten)]
    fields: ParsedSourceUnitFields,
}

impl std::ops::Deref for ParsedSourceUnit {
    type Target = ParsedSourceUnitFields;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl ParsedSourceUnit {
    fn from_parser_fields(fields: ParsedSourceUnitFields) -> Self {
        Self { fields }
    }

    pub fn stable_owner_keys(&self) -> impl Iterator<Item = StableOwnerKey> + '_ {
        self.item_index.owners().map(|entry| StableOwnerKey {
            source_unit_id: self.source_unit_id.clone(),
            item_route: entry.route.clone(),
        })
    }

    pub fn stable_check_owner_keys(&self) -> impl Iterator<Item = StableCheckOwnerKey> + '_ {
        self.owner_index
            .entries()
            .iter()
            .map(|entry| stable_check_owner_key(&self.source_unit_id, &entry.route))
    }

    pub fn owner_index(&self) -> &UnitOwnerIndex {
        &self.owner_index
    }

    pub fn owner_view(&self, route: &UnitOwnerRoute) -> Option<UnitOwnerSyntaxView<'_>> {
        Some(UnitOwnerSyntaxView {
            fields: &self.fields,
            entry: self.owner_index.entry(route)?,
            namespace: None,
        })
    }

    pub fn owner_view_for_key(
        &self,
        owner: &StableCheckOwnerKey,
    ) -> Option<UnitOwnerSyntaxView<'_>> {
        if owner.source_unit_id() != &self.source_unit_id {
            return None;
        }
        match owner {
            StableCheckOwnerKey::UnitRoot(_) => self.owner_view(&UnitOwnerRoute::UnitRoot),
            StableCheckOwnerKey::Item(owner) => {
                self.owner_view(&UnitOwnerRoute::Item(owner.item_route.clone()))
            }
        }
    }

    pub fn stable_check_owner_for_local_statement(
        &self,
        statement_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let statement = UnitLocalStatementId::__parser_new(statement_id)?;
        stable_check_owner_key_for_slot(
            &self.source_unit_id,
            &self.item_index,
            self.owner_index.statement_owner(statement)?,
        )
    }

    pub fn stable_check_owner_for_local_expression(
        &self,
        expression_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let expression = UnitLocalExpressionId::__parser_new(expression_id)?;
        stable_check_owner_key_for_slot(
            &self.source_unit_id,
            &self.item_index,
            self.owner_index.expression_owner(expression)?,
        )
    }

    pub fn stable_statement_key_local(&self, statement_id: usize) -> Option<StableStatementKey> {
        let statement = UnitLocalStatementId::__parser_new(statement_id)?;
        Some(StableStatementKey {
            source_unit_id: self.source_unit_id.clone(),
            route: self.owner_index.statement_route(statement)?.clone(),
        })
    }

    pub fn stable_expression_key_local(&self, expression_id: usize) -> Option<StableExpressionKey> {
        Some(StableExpressionKey {
            source_unit_id: self.source_unit_id.clone(),
            route_digest_v1: *self
                .expression_route_digests_v1
                .get(expression_id)?
                .as_ref()?,
        })
    }

    pub fn stable_definition_keys(&self) -> impl Iterator<Item = StableDefinitionKey> + '_ {
        self.item_index
            .definitions()
            .map(|entry| StableDefinitionKey {
                source_unit_id: self.source_unit_id.clone(),
                item_route: entry.route.clone(),
            })
    }

    pub fn stable_occurrence_key(&self, expression_id: usize) -> Option<StableOccurrenceKey> {
        Some(StableOccurrenceKey {
            source_unit_id: self.source_unit_id.clone(),
            route: self.occurrence_routes.get(expression_id)?.clone()?,
        })
    }

    pub fn stable_expression_key(&self, expression_id: usize) -> Option<StableExpressionKey> {
        self.stable_expression_key_local(expression_id)
    }

    /// Consume a context-independent parser unit into the unit-native syntax
    /// representation used by the persistent compiler.
    ///
    /// The mutation happens while this unit is uniquely owned. It does not
    /// clone another unit and never constructs a project-global AST arena.
    pub fn into_unit_syntax_snapshot(
        self,
        link_key: ProjectUnitLinkKey,
    ) -> Result<UnitSyntaxSnapshot, ParseError> {
        into_unit_syntax_snapshot_with_work(self, link_key, &ParseWorkRecorder::disabled())
    }
}

fn stable_check_owner_key(
    source_unit_id: &SourceUnitId,
    route: &UnitOwnerRoute,
) -> StableCheckOwnerKey {
    match route {
        UnitOwnerRoute::UnitRoot => StableCheckOwnerKey::UnitRoot(source_unit_id.clone()),
        UnitOwnerRoute::Item(item_route) => StableCheckOwnerKey::Item(StableOwnerKey {
            source_unit_id: source_unit_id.clone(),
            item_route: item_route.clone(),
        }),
    }
}

fn stable_check_owner_key_for_slot(
    source_unit_id: &SourceUnitId,
    item_index: &UnitItemIndex,
    slot: UnitCheckOwnerSlot,
) -> Option<StableCheckOwnerKey> {
    if slot.is_unit_root() {
        return Some(StableCheckOwnerKey::UnitRoot(source_unit_id.clone()));
    }
    let item = item_index.entries.get(slot.item_index()?)?;
    Some(StableCheckOwnerKey::Item(StableOwnerKey {
        source_unit_id: source_unit_id.clone(),
        item_route: item.route.clone(),
    }))
}

fn unit_statement_by_local_id<'a>(
    fields: &'a ParsedSourceUnitFields,
    statement: UnitLocalStatementId,
    depth: usize,
) -> Option<&'a AstStatement> {
    if depth > fields.owner_index.statement_count() {
        return None;
    }
    let locator = fields.owner_index.statement_locator(statement)?;
    match locator.parent() {
        Some(parent) => unit_statement_by_local_id(fields, parent, depth + 1)?
            .children
            .get(locator.child_index()),
        None => fields.ast.statements.get(locator.child_index()),
    }
}

/// Borrowed syntax body for one parser-owned check owner.
///
/// The view yields only this owner's statements and expressions. Descendant
/// item bodies are replaced by the ordered `child_owners` boundary list, so a
/// checker cannot accidentally perform a whole-unit recursive traversal.
#[derive(Clone, Copy)]
pub struct UnitOwnerSyntaxView<'a> {
    fields: &'a ParsedSourceUnitFields,
    entry: &'a UnitOwnerIndexEntry,
    namespace: Option<SyntaxUnitNamespace>,
}

impl<'a> UnitOwnerSyntaxView<'a> {
    pub fn source_unit_id(&self) -> &'a SourceUnitId {
        &self.fields.source_unit_id
    }

    pub fn path(&self) -> &'a str {
        &self.fields.path
    }

    /// Physical one-based source line containing `byte`.
    ///
    /// Parser expressions can span multiple lines while retaining the line of
    /// their first token. Source-map subspans must instead use the line that
    /// owns their exact byte start. The parsed line table is shared by every
    /// owner view, so this lookup stays logarithmic without rescanning source
    /// text per owner or anchor.
    pub fn physical_line_for_byte(&self, byte: usize) -> Option<usize> {
        let index = self
            .fields
            .ast
            .lines
            .partition_point(|line| line.start <= byte)
            .checked_sub(1)?;
        let line = self.fields.ast.lines.get(index)?;
        (byte <= line.end).then_some(line.line)
    }

    pub fn route(&self) -> &'a UnitOwnerRoute {
        &self.entry.route
    }

    pub fn stable_key(&self) -> StableCheckOwnerKey {
        stable_check_owner_key(&self.fields.source_unit_id, &self.entry.route)
    }

    pub fn child_owners(&self) -> &'a [UnitChildOwnerBoundary] {
        &self.entry.child_owners
    }

    pub fn child_owner_routes(&self) -> impl Iterator<Item = &'a StableItemRoute> + 'a {
        self.entry
            .child_owners
            .iter()
            .map(|boundary| &boundary.route)
    }

    /// Return the syntax id of the authored statement that starts a direct
    /// child owner. The id follows this view's raw or packed namespace and is
    /// intended only for normalizing references retained inside the AST.
    pub fn child_owner_statement_id(&self, boundary: &UnitChildOwnerBoundary) -> Option<usize> {
        self.syntax_id(boundary.statement().as_usize())
    }

    /// Return the expression whose structural parent edge crosses from this
    /// owner into the direct child owner. This is intentionally the child
    /// statement's authored expression, not its potentially deeper public
    /// result expression.
    pub fn child_owner_boundary_expression(
        &self,
        boundary: &UnitChildOwnerBoundary,
    ) -> Option<usize> {
        boundary
            .boundary_expression()
            .and_then(|expression| self.syntax_id(expression.as_usize()))
    }

    /// Return the exact public value expression exported by a child-owner
    /// boundary, when that child is a value-bearing item.
    ///
    /// The parent keeps only this stable result identity; it never traverses
    /// the child body. Function declarations intentionally export no value at
    /// their placement in the containing statement sequence.
    pub fn child_owner_result_expression(
        &self,
        boundary: &UnitChildOwnerBoundary,
    ) -> Option<usize> {
        boundary
            .result_expression()
            .and_then(|expression| self.syntax_id(expression.as_usize()))
    }

    pub fn statement_ids(&self) -> &'a [UnitLocalStatementId] {
        &self.entry.statements
    }

    pub fn expression_ids(&self) -> &'a [UnitLocalExpressionId] {
        &self.entry.expressions
    }

    pub fn statement_locator(
        &self,
        statement: UnitLocalStatementId,
    ) -> Option<UnitStatementLocator> {
        self.fields.owner_index.statement_locator(statement)
    }

    /// Return any unit-local statement reachable through this owner's
    /// containment chain, including an enclosing statement owned by another
    /// item. Owner projections use this to preserve declaration authority at
    /// a child boundary without retaining the whole parent AST.
    pub fn statement_for_local(&self, statement: UnitLocalStatementId) -> Option<&'a AstStatement> {
        unit_statement_by_local_id(self.fields, statement, 0)
    }

    /// Return the stable check owner of any unit-local statement. This is the
    /// statement counterpart of `stable_check_owner_for_syntax_expression` and
    /// is intended for exact cross-owner containment authority.
    pub fn stable_check_owner_for_local_statement(
        &self,
        statement: UnitLocalStatementId,
    ) -> Option<StableCheckOwnerKey> {
        stable_check_owner_key_for_slot(
            &self.fields.source_unit_id,
            &self.fields.item_index,
            self.fields.owner_index.statement_owner(statement)?,
        )
    }

    pub fn stable_statement_key_local(
        &self,
        statement: UnitLocalStatementId,
    ) -> Option<StableStatementKey> {
        Some(StableStatementKey {
            source_unit_id: self.fields.source_unit_id.clone(),
            route: self.fields.owner_index.statement_route(statement)?.clone(),
        })
    }

    pub fn stable_expression_key_local(
        &self,
        expression: UnitLocalExpressionId,
    ) -> Option<StableExpressionKey> {
        Some(StableExpressionKey {
            source_unit_id: self.fields.source_unit_id.clone(),
            route_digest_v1: *self
                .fields
                .expression_route_digests_v1
                .get(expression.as_usize())?
                .as_ref()?,
        })
    }

    /// Returns the stable identity for any expression in this source unit.
    ///
    /// Owner projections use this only for explicit references that cross a
    /// child-owner boundary. The returned identity never exposes the unit-
    /// local arena id as semantic input.
    pub fn stable_expression_key_for_syntax(
        &self,
        expression_id: usize,
    ) -> Option<StableExpressionKey> {
        let expression = self.local_expression_id(expression_id)?;
        self.stable_expression_key_local(expression)
    }

    /// Return the exact stable parent edge of any expression in this source
    /// unit. The parent can belong to this view even when `expression_id`
    /// belongs to a direct child owner; this is the structural fact required
    /// to place a child public result in the parent's lexical expression scope.
    pub fn stable_expression_parent_edge_for_syntax(
        &self,
        expression_id: usize,
    ) -> Option<(
        StableCheckOwnerKey,
        StableExpressionKey,
        StableExpressionRouteSegment,
    )> {
        let expression = self.local_expression_id(expression_id)?;
        let edge = self.fields.owner_index.expression_parent_edge(expression)?;
        let parent = edge.parent();
        Some((
            stable_check_owner_key_for_slot(
                &self.fields.source_unit_id,
                &self.fields.item_index,
                self.fields.owner_index.expression_owner(parent)?,
            )?,
            self.stable_expression_key_local(parent)?,
            edge.segment().clone(),
        ))
    }

    /// Return the structural expression edge that crosses from a descendant
    /// owner into this view. A direct child owner's public value may itself be
    /// owned by a deeper descendant, so the immediate parent edge is not
    /// necessarily the containment boundary.
    pub fn stable_expression_boundary_parent_edge_for_syntax(
        &self,
        expression_id: usize,
    ) -> Option<(
        StableCheckOwnerKey,
        StableExpressionKey,
        StableCheckOwnerKey,
        StableExpressionKey,
        StableExpressionRouteSegment,
    )> {
        let expected_owner = self.stable_key();
        let mut expression = self.local_expression_id(expression_id)?;
        for _ in 0..self.fields.ast.expressions.len() {
            let edge = self.fields.owner_index.expression_parent_edge(expression)?;
            let parent = edge.parent();
            let child_owner = stable_check_owner_key_for_slot(
                &self.fields.source_unit_id,
                &self.fields.item_index,
                self.fields.owner_index.expression_owner(expression)?,
            )?;
            let child_expression = self.stable_expression_key_local(expression)?;
            let owner = stable_check_owner_key_for_slot(
                &self.fields.source_unit_id,
                &self.fields.item_index,
                self.fields.owner_index.expression_owner(parent)?,
            )?;
            if owner == expected_owner {
                return Some((
                    child_owner,
                    child_expression,
                    owner,
                    self.stable_expression_key_local(parent)?,
                    edge.segment().clone(),
                ));
            }
            expression = parent;
        }
        None
    }

    /// Returns the stable check owner for any expression in this source unit.
    pub fn stable_check_owner_for_syntax_expression(
        &self,
        expression_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let expression = self.local_expression_id(expression_id)?;
        stable_check_owner_key_for_slot(
            &self.fields.source_unit_id,
            &self.fields.item_index,
            self.fields.owner_index.expression_owner(expression)?,
        )
    }

    /// Return the selector structurally owning one `WHEN`/`WHILE` match arm.
    ///
    /// The selector can live in an enclosing owner when the arm wraps an
    /// authored child item such as `LIST`. The owner projection retains this
    /// exact cross-boundary expression instead of guessing from the enclosing
    /// owner's public result.
    pub fn pattern_selector_for_syntax_expression(&self, expression_id: usize) -> Option<usize> {
        let expression = self.local_expression_id(expression_id)?;
        let parent = self.fields.owner_index.expression_parent(expression)?;
        let parent = self.fields.ast.expressions.get(parent.as_usize())?;
        let selector = match &parent.kind {
            AstExprKind::When { input, arms } if arms.contains(&expression_id) => {
                parent.linked_input.unwrap_or(*input)
            }
            AstExprKind::Pipe {
                input, op, arms, ..
            } if op == "WHILE" && arms.contains(&expression_id) => {
                parent.linked_input.unwrap_or(*input)
            }
            _ => return None,
        };
        Some(selector)
    }

    fn local_expression_id(&self, expression_id: usize) -> Option<UnitLocalExpressionId> {
        let local = match self.namespace {
            Some(expected) => {
                let (namespace, local) = __parser_unpack_syntax_node_id(expression_id)?;
                (namespace == expected).then_some(local)?
            }
            None => expression_id,
        };
        UnitLocalExpressionId::__parser_new(local)
    }

    fn syntax_id(&self, local: usize) -> Option<usize> {
        self.namespace.map_or(Some(local), |namespace| {
            __parser_pack_syntax_node_id(namespace, local)
        })
    }

    pub fn statements(&self) -> impl Iterator<Item = &'a AstStatement> + 'a {
        let fields = self.fields;
        self.entry.statements.iter().map(move |statement| {
            unit_statement_by_local_id(fields, *statement, 0)
                .expect("parser owner index statement locator resolves")
        })
    }

    pub fn expressions(&self) -> impl Iterator<Item = &'a AstExpr> + 'a {
        let expressions = &self.fields.ast.expressions;
        self.entry.expressions.iter().map(move |expression| {
            expressions
                .get(expression.as_usize())
                .expect("parser owner index expression locator resolves")
        })
    }

    pub fn stable_expression_keys(&self) -> impl Iterator<Item = StableExpressionKey> + 'a {
        let fields = self.fields;
        self.entry
            .expressions
            .iter()
            .map(move |expression| StableExpressionKey {
                source_unit_id: fields.source_unit_id.clone(),
                route_digest_v1: fields.expression_route_digests_v1[expression.as_usize()]
                    .expect("owned parser expression has a structural identity"),
            })
    }

    /// Count authored fieldless HOLD owners for one declaration authority that
    /// precede this owner in source order. This is unit-local lookup metadata,
    /// not part of a stable owner key: checked state lowering uses it only to
    /// reproduce the declaration's dense `state_N` path ordinal across owner
    /// boundaries, including sibling HOLDs whose authored names differ.
    pub fn preceding_fieldless_hold_count_for_authority(
        &self,
        authority: &StableCheckOwnerKey,
    ) -> Option<usize> {
        let StableCheckOwnerKey::Item(authority) = authority else {
            return None;
        };
        if &authority.source_unit_id != self.source_unit_id() {
            return None;
        }
        let UnitOwnerRoute::Item(_) = self.route() else {
            return None;
        };
        let current_statement = self.entry.statements.first()?.as_usize();
        let authority_route = authority.item_route.segments();
        Some(
            self.fields
                .item_index
                .entries
                .iter()
                .filter(|candidate| candidate.local_statement_id < current_statement)
                .filter(|candidate| {
                    let candidate_route = candidate.route.segments();
                    let in_authority = candidate_route == authority_route
                        && authority_route
                            .last()
                            .is_some_and(|segment| segment.kind == UnitItemKind::Hold);
                    let in_fieldless_chain = candidate_route
                        .strip_prefix(authority_route)
                        .is_some_and(|descendants| {
                            !descendants.is_empty()
                                && descendants
                                    .iter()
                                    .all(|segment| segment.kind == UnitItemKind::Hold)
                        });
                    if !in_authority && !in_fieldless_chain {
                        return false;
                    }
                    let statement = UnitLocalStatementId::__parser_new(
                        candidate.local_statement_id,
                    )
                    .and_then(|statement| unit_statement_by_local_id(self.fields, statement, 0));
                    matches!(
                        statement.map(|statement| &statement.kind),
                        Some(AstStatementKind::Hold {
                            field: None,
                            name: Some(_),
                        })
                    )
                })
                .count(),
        )
    }

    /// Whether this owner's public result is the declaration value published
    /// by `authority` after walking only the transparent state-path wrappers
    /// used by checked lowering.
    pub fn public_result_is_transparent_from_authority(
        &self,
        authority: &StableCheckOwnerKey,
    ) -> Option<bool> {
        let StableCheckOwnerKey::Item(authority) = authority else {
            return None;
        };
        if &authority.source_unit_id != self.source_unit_id() {
            return None;
        }
        let UnitOwnerRoute::Item(current_route) = self.route() else {
            return None;
        };
        let result_for_route = |route: &StableItemRoute| {
            self.fields
                .owner_index
                .entries()
                .iter()
                .flat_map(|entry| &entry.child_owners)
                .find(|boundary| &boundary.route == route)
                .and_then(UnitChildOwnerBoundary::result_expression)
        };
        let mut expression = result_for_route(&authority.item_route)?;
        let target = result_for_route(current_route)?;
        let mut visited = BTreeSet::new();
        while visited.insert(expression) {
            if expression == target {
                return Some(true);
            }
            let syntax = self.fields.ast.expressions.get(expression.as_usize())?;
            let next = match &syntax.kind {
                AstExprKind::Draining { input } => Some(*input),
                AstExprKind::Flush { payload } => *payload,
                AstExprKind::Block { result, .. }
                | AstExprKind::MatchArm { output: result, .. } => *result,
                _ => None,
            }?;
            expression = self.local_expression_id(next)?;
        }
        Some(false)
    }
}

fn into_unit_syntax_snapshot_with_work(
    mut parsed: ParsedSourceUnit,
    link_key: ProjectUnitLinkKey,
    work: &ParseWorkRecorder,
) -> Result<UnitSyntaxSnapshot, ParseError> {
    // `ParsedSourceUnit` is an opaque, parser-produced validation seal. Project
    // linking changes only module spellings and the syntax lookup encoding;
    // neither operation can change token policy, delimiter balance, list
    // capacities, reducer syntax, hidden identities, or AST topology. Rebuilding
    // `ValidationIndex` and replaying every source policy here used to turn a
    // reusable unit into another whole-unit validation pass.
    if let Some(module) = link_key.module.as_deref() {
        let functions_by_module = BTreeMap::from([(
            module.to_owned(),
            link_key.module_functions.iter().cloned().collect(),
        )]);
        namespace_source_unit_module(&mut parsed.fields.ast, module, &functions_by_module, work);
    }
    pack_source_unit_syntax_ids(
        &parsed.fields.path,
        &mut parsed.fields.ast,
        link_key.namespace,
    )?;
    Ok(UnitSyntaxSnapshot {
        fields: parsed.fields,
        namespace: link_key.namespace,
        module: link_key.module.clone(),
        link_key,
    })
}

/// Project-link one freshly parsed unit and return deterministic namespace work.
/// Source validation belongs to the opaque parser artifact and is not replayed;
/// session cache hits never call this function.
pub fn link_project_source_unit_profiled(
    parsed: ParsedSourceUnit,
    link_key: ProjectUnitLinkKey,
) -> (Result<UnitSyntaxSnapshot, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let linked = into_unit_syntax_snapshot_with_work(parsed, link_key, &work);
    (linked, work.finish())
}

/// Assign deterministic, collision-checked namespaces from stable source-unit
/// paths. Open addressing is deterministic in canonical path order. A rare
/// collision can relink the colliding units, but it cannot silently alias two
/// syntax arenas.
pub fn project_syntax_namespaces(
    paths: impl IntoIterator<Item = SourceUnitId>,
) -> Result<BTreeMap<SourceUnitId, SyntaxUnitNamespace>, ParseError> {
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    if paths.len() > usize::from(SyntaxUnitNamespace::MAX) {
        return Err(ParseError {
            path: String::new(),
            line: None,
            column: None,
            message: format!(
                "project has {} source units but the unit-native syntax namespace supports at most {}",
                paths.len(),
                SyntaxUnitNamespace::MAX,
            ),
        });
    }
    let mut occupied = BTreeSet::new();
    let mut assigned = BTreeMap::new();
    for path in paths {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in path.as_str().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        let mut candidate = u16::try_from(hash % u64::from(SyntaxUnitNamespace::MAX))
            .unwrap_or_default()
            .saturating_add(1);
        let start = candidate;
        loop {
            if occupied.insert(candidate) {
                let namespace = SyntaxUnitNamespace::__parser_new(candidate)
                    .expect("candidate is inside syntax namespace range");
                assigned.insert(path, namespace);
                break;
            }
            candidate = if candidate == SyntaxUnitNamespace::MAX {
                1
            } else {
                candidate + 1
            };
            if candidate == start {
                return Err(ParseError {
                    path: path.into_string(),
                    line: None,
                    column: None,
                    message: "unit-native syntax namespace is exhausted".to_owned(),
                });
            }
        }
    }
    Ok(assigned)
}

/// Compute the body-insensitive link context for already parsed units.
pub fn project_unit_link_keys(
    entrypoint: &str,
    units: impl IntoIterator<Item = (SourceUnitId, Vec<String>)>,
) -> Result<BTreeMap<SourceUnitId, ProjectUnitLinkKey>, ParseError> {
    let units = units.into_iter().collect::<Vec<_>>();
    let namespaces = project_syntax_namespaces(units.iter().map(|(id, _)| id.clone()))?;
    let mut functions_by_module = BTreeMap::<String, BTreeSet<String>>::new();
    for (source_unit_id, functions) in &units {
        if let Some(module) =
            project_module_name_for_source_unit(entrypoint, source_unit_id.as_str())
        {
            functions_by_module
                .entry(module)
                .or_default()
                .extend(functions.iter().cloned());
        }
    }
    let mut keys = BTreeMap::new();
    for (source_unit_id, _) in units {
        let module = project_module_name_for_source_unit(entrypoint, source_unit_id.as_str());
        let module_functions = module
            .as_ref()
            .and_then(|module| functions_by_module.get(module))
            .map(|functions| functions.iter().cloned().collect())
            .unwrap_or_default();
        keys.insert(
            source_unit_id.clone(),
            ProjectUnitLinkKey {
                namespace: *namespaces.get(&source_unit_id).ok_or_else(|| ParseError {
                    path: source_unit_id.as_str().to_owned(),
                    line: None,
                    column: None,
                    message: "source unit has no syntax namespace".to_owned(),
                })?,
                module,
                module_functions,
            },
        );
    }
    Ok(keys)
}

fn pack_source_unit_syntax_ids(
    path: &str,
    ast: &mut AstProgram,
    namespace: SyntaxUnitNamespace,
) -> Result<(), ParseError> {
    fn pack(path: &str, namespace: SyntaxUnitNamespace, local: usize) -> Result<usize, ParseError> {
        __parser_pack_syntax_node_id(namespace, local).ok_or_else(|| {
            parsed_source_unit_invariant_error(
                path,
                format!("unit-local syntax id {local} exceeds the packed syntax bound"),
            )
        })
    }

    fn pack_statement(
        path: &str,
        namespace: SyntaxUnitNamespace,
        statement: &mut AstStatement,
    ) -> Result<(), ParseError> {
        statement.id = pack(path, namespace, statement.id)?;
        if let Some(expression) = &mut statement.expr {
            *expression = pack(path, namespace, *expression)?;
        }
        for child in &mut statement.children {
            pack_statement(path, namespace, child)?;
        }
        Ok(())
    }

    fn pack_expression_reference(
        path: &str,
        namespace: SyntaxUnitNamespace,
        reference: &mut usize,
    ) -> Result<(), ParseError> {
        *reference = pack(path, namespace, *reference)?;
        Ok(())
    }

    for statement in &mut ast.statements {
        pack_statement(path, namespace, statement)?;
    }
    for expression in ast.expressions.__parser_make_mut() {
        expression.id = pack(path, namespace, expression.id)?;
        if let Some(input) = &mut expression.linked_input {
            pack_expression_reference(path, namespace, input)?;
        }
        match &mut expression.kind {
            AstExprKind::TextTemplate { segments } => {
                for segment in segments {
                    if let AstTextSegment::Dynamic { value } = segment {
                        pack_expression_reference(path, namespace, value)?;
                    }
                }
            }
            AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
                for field in fields {
                    pack_expression_reference(path, namespace, &mut field.value)?;
                }
            }
            AstExprKind::Flush { payload } => {
                if let Some(payload) = payload {
                    pack_expression_reference(path, namespace, payload)?;
                }
            }
            AstExprKind::Call { args, pass, .. } => {
                for argument in args {
                    pack_expression_reference(path, namespace, &mut argument.value)?;
                }
                if let Some(pass) = pass {
                    pack_expression_reference(path, namespace, &mut pass.value)?;
                }
            }
            AstExprKind::Pipe {
                input,
                args,
                pass,
                arms,
                ..
            } => {
                pack_expression_reference(path, namespace, input)?;
                for argument in args {
                    pack_expression_reference(path, namespace, &mut argument.value)?;
                }
                if let Some(pass) = pass {
                    pack_expression_reference(path, namespace, &mut pass.value)?;
                }
                for arm in arms {
                    pack_expression_reference(path, namespace, arm)?;
                }
            }
            AstExprKind::Draining { input } => {
                pack_expression_reference(path, namespace, input)?;
            }
            AstExprKind::Hold { initial, .. } => {
                pack_expression_reference(path, namespace, initial)?;
            }
            AstExprKind::Latest { branches } => {
                for branch in branches {
                    pack_expression_reference(path, namespace, branch)?;
                }
            }
            AstExprKind::When { input, arms } => {
                pack_expression_reference(path, namespace, input)?;
                for arm in arms {
                    pack_expression_reference(path, namespace, arm)?;
                }
            }
            AstExprKind::Then { input, output } => {
                pack_expression_reference(path, namespace, input)?;
                if let Some(output) = output {
                    pack_expression_reference(path, namespace, output)?;
                }
            }
            AstExprKind::Infix { left, right, .. } => {
                pack_expression_reference(path, namespace, left)?;
                pack_expression_reference(path, namespace, right)?;
            }
            AstExprKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    pack_expression_reference(path, namespace, output)?;
                }
            }
            AstExprKind::Block { bindings, result } => {
                for binding in bindings {
                    let AstBlockBindingDeclaration::Local { statement } = &mut binding.declaration
                    else {
                        return Err(parsed_source_unit_invariant_error(
                            path,
                            "raw source-unit BLOCK binding contains an owner-normalized child declaration",
                        ));
                    };
                    *statement = pack(path, namespace, *statement)?;
                    pack_expression_reference(path, namespace, &mut binding.value)?;
                }
                if let Some(result) = result {
                    pack_expression_reference(path, namespace, result)?;
                }
            }
            AstExprKind::ListLiteral { items, .. }
            | AstExprKind::BytesLiteral { items, .. }
            | AstExprKind::SetLiteral { items } => {
                for item in items {
                    pack_expression_reference(path, namespace, item)?;
                }
            }
            AstExprKind::Arrow { left, output, .. } => {
                pack_expression_reference(path, namespace, left)?;
                if let Some(output) = output {
                    pack_expression_reference(path, namespace, output)?;
                }
            }
            AstExprKind::MapEntry { key, value } => {
                pack_expression_reference(path, namespace, key)?;
                pack_expression_reference(path, namespace, value)?;
            }
            AstExprKind::MapLiteral { entries } => {
                for entry in entries {
                    pack_expression_reference(path, namespace, entry)?;
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
    }
    Ok(())
}

/// Project context that affects only syntax lookup and authored module names.
///
/// The key is intentionally body-insensitive. A body edit can rebuild one unit
/// snapshot while every other unit remains shared; adding or removing a module
/// function relinks only units whose module lookup actually changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectUnitLinkKey {
    pub namespace: SyntaxUnitNamespace,
    pub module: Option<String>,
    pub module_functions: Vec<String>,
}

/// Immutable unit-native syntax used by the persistent compiler.
///
/// Expression and statement arenas remain physically owned by this unit. Their
/// packed ids contain the collision-checked unit namespace, so a project view
/// can borrow the arenas directly without copying or rebasing their nodes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UnitSyntaxSnapshot {
    #[serde(flatten)]
    fields: ParsedSourceUnitFields,
    namespace: SyntaxUnitNamespace,
    module: Option<String>,
    #[serde(skip)]
    link_key: ProjectUnitLinkKey,
}

impl std::ops::Deref for UnitSyntaxSnapshot {
    type Target = ParsedSourceUnitFields;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl UnitSyntaxSnapshot {
    pub fn namespace(&self) -> SyntaxUnitNamespace {
        self.namespace
    }

    pub fn module(&self) -> Option<&str> {
        self.module.as_deref()
    }

    pub fn link_key(&self) -> &ProjectUnitLinkKey {
        &self.link_key
    }

    pub fn stable_owner_keys(&self) -> impl Iterator<Item = StableOwnerKey> + '_ {
        self.item_index.owners().map(|entry| StableOwnerKey {
            source_unit_id: self.source_unit_id.clone(),
            item_route: entry.route.clone(),
        })
    }

    pub fn stable_check_owner_keys(&self) -> impl Iterator<Item = StableCheckOwnerKey> + '_ {
        self.owner_index
            .entries()
            .iter()
            .map(|entry| stable_check_owner_key(&self.source_unit_id, &entry.route))
    }

    pub fn owner_index(&self) -> &UnitOwnerIndex {
        &self.owner_index
    }

    pub fn owner_view(&self, route: &UnitOwnerRoute) -> Option<UnitOwnerSyntaxView<'_>> {
        Some(UnitOwnerSyntaxView {
            fields: &self.fields,
            entry: self.owner_index.entry(route)?,
            namespace: Some(self.namespace),
        })
    }

    pub fn owner_view_for_key(
        &self,
        owner: &StableCheckOwnerKey,
    ) -> Option<UnitOwnerSyntaxView<'_>> {
        if owner.source_unit_id() != &self.source_unit_id {
            return None;
        }
        match owner {
            StableCheckOwnerKey::UnitRoot(_) => self.owner_view(&UnitOwnerRoute::UnitRoot),
            StableCheckOwnerKey::Item(owner) => {
                self.owner_view(&UnitOwnerRoute::Item(owner.item_route.clone()))
            }
        }
    }

    pub fn stable_check_owner_for_statement(
        &self,
        statement_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let (namespace, local) = __parser_unpack_syntax_node_id(statement_id)?;
        if namespace != self.namespace {
            return None;
        }
        let statement = UnitLocalStatementId::__parser_new(local)?;
        stable_check_owner_key_for_slot(
            &self.source_unit_id,
            &self.item_index,
            self.owner_index.statement_owner(statement)?,
        )
    }

    pub fn stable_check_owner_for_expression(
        &self,
        expression_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let (namespace, local) = __parser_unpack_syntax_node_id(expression_id)?;
        if namespace != self.namespace {
            return None;
        }
        let expression = UnitLocalExpressionId::__parser_new(local)?;
        stable_check_owner_key_for_slot(
            &self.source_unit_id,
            &self.item_index,
            self.owner_index.expression_owner(expression)?,
        )
    }

    pub fn stable_statement_key(&self, statement_id: usize) -> Option<StableStatementKey> {
        let (namespace, local) = __parser_unpack_syntax_node_id(statement_id)?;
        if namespace != self.namespace {
            return None;
        }
        let statement = UnitLocalStatementId::__parser_new(local)?;
        Some(StableStatementKey {
            source_unit_id: self.source_unit_id.clone(),
            route: self.owner_index.statement_route(statement)?.clone(),
        })
    }

    pub fn stable_expression_key_local(&self, expression_id: usize) -> Option<StableExpressionKey> {
        Some(StableExpressionKey {
            source_unit_id: self.source_unit_id.clone(),
            route_digest_v1: *self
                .expression_route_digests_v1
                .get(expression_id)?
                .as_ref()?,
        })
    }

    pub fn stable_definition_keys(&self) -> impl Iterator<Item = StableDefinitionKey> + '_ {
        self.item_index
            .definitions()
            .map(|entry| StableDefinitionKey {
                source_unit_id: self.source_unit_id.clone(),
                item_route: entry.route.clone(),
            })
    }

    pub fn stable_occurrence_key(&self, expression_id: usize) -> Option<StableOccurrenceKey> {
        let (namespace, local) = __parser_unpack_syntax_node_id(expression_id)?;
        if namespace != self.namespace {
            return None;
        }
        Some(StableOccurrenceKey {
            source_unit_id: self.source_unit_id.clone(),
            route: self.occurrence_routes.get(local)?.clone()?,
        })
    }

    pub fn stable_expression_key(&self, expression_id: usize) -> Option<StableExpressionKey> {
        let (namespace, local) = __parser_unpack_syntax_node_id(expression_id)?;
        if namespace != self.namespace {
            return None;
        }
        self.stable_expression_key_local(local)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectItemIndexEntry {
    pub owner_key: StableOwnerKey,
    pub definition_key: Option<StableDefinitionKey>,
    pub source_unit_id: SourceUnitId,
    pub route: StableItemRoute,
    pub local_statement_id: usize,
    pub statement_id: usize,
    pub kind: UnitItemKind,
    pub names: Vec<String>,
    pub parameters: Vec<UnitItemParameter>,
}

/// Body-insensitive item headers over unit-native project syntax.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ProjectItemIndex {
    pub entries: Vec<ProjectItemIndexEntry>,
}

impl ProjectItemIndex {
    pub fn owners(&self) -> impl Iterator<Item = &ProjectItemIndexEntry> {
        self.entries.iter()
    }

    pub fn definitions(&self) -> impl Iterator<Item = &ProjectItemIndexEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.definition_key.is_some())
    }
}

/// One source unit's position inside the canonical project source map.
///
/// AST locations remain unit-local. Global offsets are computed through this
/// compact map only for the legacy checked/artifact projection that still
/// requires them; no AST node is rewritten when an earlier unit changes size.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectSourceUnitLayout {
    pub source_unit_id: SourceUnitId,
    pub path: String,
    pub start_line: usize,
    pub start_byte: usize,
    pub source_len: usize,
    pub line_count: usize,
    pub module: Option<String>,
    pub namespace: SyntaxUnitNamespace,
}

/// Opaque unit-native project syntax snapshot.
///
/// This is the production checker input. It owns only `Arc` unit snapshots,
/// body-insensitive indexes, and a compact source map. It contains no global
/// AST arena, concatenated source buffer, or rebased statement/expression
/// copy.
#[derive(Clone, Debug)]
pub struct ProjectSyntaxSnapshot {
    fields: std::sync::Arc<ProjectSyntaxSnapshotFields>,
}

#[derive(Debug)]
struct ProjectSyntaxSnapshotFields {
    source_bundle_digest_v1: SourceBundleDigestV1,
    path: String,
    units: Vec<std::sync::Arc<UnitSyntaxSnapshot>>,
    unit_by_namespace: BTreeMap<SyntaxUnitNamespace, usize>,
    source_layouts: Vec<ProjectSourceUnitLayout>,
    item_index: ProjectItemIndex,
    functions: Vec<String>,
    operators: Vec<String>,
    expression_offsets: Vec<usize>,
    statement_offsets: Vec<usize>,
    expression_count: usize,
    check_expression_count: usize,
    statement_count: usize,
}

impl ProjectSyntaxSnapshot {
    pub fn from_unit_snapshots(
        entrypoint: &str,
        mut units: Vec<std::sync::Arc<UnitSyntaxSnapshot>>,
    ) -> Result<Self, ParseError> {
        units.sort_by(|left, right| {
            left.source_unit_id
                .as_str()
                .as_bytes()
                .cmp(right.source_unit_id.as_str().as_bytes())
        });
        let bundle = CanonicalSourceBundleV1::new(
            entrypoint,
            units
                .iter()
                .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
        )
        .map_err(|error| source_bundle_parse_error(entrypoint, error))?;
        let path = bundle.entrypoint().to_owned();
        let source_bundle_digest_v1 = bundle.digest();
        drop(bundle);

        let mut unit_by_namespace = BTreeMap::new();
        let mut source_layouts = Vec::with_capacity(units.len());
        let mut item_index = ProjectItemIndex::default();
        let mut functions = Vec::new();
        let mut operators = Vec::new();
        let mut expression_count = 0usize;
        let mut check_expression_count = 0usize;
        let mut statement_count = 0usize;
        let mut expression_offsets = Vec::with_capacity(units.len());
        let mut statement_offsets = Vec::with_capacity(units.len());
        let mut next_line = 1usize;
        let mut next_byte = 0usize;

        for (unit_index, unit) in units.iter().enumerate() {
            if unit.path != unit.source_unit_id.as_str() {
                return Err(parsed_source_unit_invariant_error(
                    &unit.path,
                    "unit syntax path does not match its SourceUnitId",
                ));
            }
            if unit_by_namespace
                .insert(unit.namespace, unit_index)
                .is_some()
            {
                return Err(parsed_source_unit_invariant_error(
                    &unit.path,
                    "unit syntax namespace collides inside the project snapshot",
                ));
            }
            expression_offsets.push(expression_count);
            statement_offsets.push(statement_count);
            expression_count = expression_count
                .checked_add(unit.ast.expressions.len())
                .ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        &unit.path,
                        "project expression count overflows usize",
                    )
                })?;
            check_expression_count = check_expression_count
                .checked_add(
                    unit.owner_index
                        .entries()
                        .iter()
                        .map(|entry| entry.expressions.len())
                        .sum::<usize>(),
                )
                .ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        &unit.path,
                        "project check-expression count overflows usize",
                    )
                })?;
            let mut expected_local_statement = 0usize;
            let local_statement_count = unit_native_statement_count(
                &unit.path,
                &unit.ast.statements,
                unit.namespace,
                &mut expected_local_statement,
            )?;
            statement_count = statement_count
                .checked_add(local_statement_count)
                .ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        &unit.path,
                        "project statement count overflows usize",
                    )
                })?;
            let line_count = unit.source.lines().count().max(1);
            source_layouts.push(ProjectSourceUnitLayout {
                source_unit_id: unit.source_unit_id.clone(),
                path: unit.path.clone(),
                start_line: next_line,
                start_byte: next_byte,
                source_len: unit.source.len(),
                line_count,
                module: unit.module.clone(),
                namespace: unit.namespace,
            });
            for entry in &unit.item_index.entries {
                let statement_id =
                    __parser_pack_syntax_node_id(unit.namespace, entry.local_statement_id)
                        .ok_or_else(|| {
                            parsed_source_unit_invariant_error(
                                &unit.path,
                                "item statement id exceeds the unit syntax bound",
                            )
                        })?;
                let mut names = entry.names.clone();
                if entry.kind == UnitItemKind::Function
                    && let Some(module) = unit.module.as_deref()
                {
                    for name in &mut names {
                        if !name.contains('/') {
                            *name = format!("{module}/{name}");
                        }
                    }
                }
                item_index.entries.push(ProjectItemIndexEntry {
                    owner_key: StableOwnerKey {
                        source_unit_id: unit.source_unit_id.clone(),
                        item_route: entry.route.clone(),
                    },
                    definition_key: entry.is_definition().then(|| StableDefinitionKey {
                        source_unit_id: unit.source_unit_id.clone(),
                        item_route: entry.route.clone(),
                    }),
                    source_unit_id: unit.source_unit_id.clone(),
                    route: entry.route.clone(),
                    local_statement_id: entry.local_statement_id,
                    statement_id,
                    kind: entry.kind,
                    names,
                    parameters: entry.parameters.clone(),
                });
            }
            collect_raw_declared_functions_with_work(
                &unit.ast.statements,
                &ParseWorkRecorder::disabled(),
            )
            .into_iter()
            .for_each(|function| functions.push(function));
            for item in &unit.ast.items {
                for operator in &item.operators {
                    if !operators.iter().any(|existing| existing == operator) {
                        operators.push(operator.clone());
                    }
                }
            }
            next_line = next_line.checked_add(line_count).ok_or_else(|| {
                parsed_source_unit_invariant_error(&unit.path, "project line count overflows usize")
            })?;
            next_byte = next_byte.checked_add(unit.source.len()).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "project source bytes overflow usize",
                )
            })?;
            if unit_index + 1 < units.len() && !unit.source.ends_with('\n') {
                next_byte = next_byte.checked_add(1).ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        &unit.path,
                        "project source separator overflows usize",
                    )
                })?;
            }
        }

        Ok(Self {
            fields: std::sync::Arc::new(ProjectSyntaxSnapshotFields {
                source_bundle_digest_v1,
                path,
                units,
                unit_by_namespace,
                source_layouts,
                item_index,
                functions,
                operators,
                expression_offsets,
                statement_offsets,
                expression_count,
                check_expression_count,
                statement_count,
            }),
        })
    }

    pub fn source_bundle_digest_v1(&self) -> SourceBundleDigestV1 {
        self.fields.source_bundle_digest_v1
    }

    pub fn path(&self) -> &str {
        &self.fields.path
    }

    pub fn kind(&self) -> ProgramKind {
        ProgramKind::Generic
    }

    pub fn units(&self) -> &[std::sync::Arc<UnitSyntaxSnapshot>] {
        &self.fields.units
    }

    pub fn source_layouts(&self) -> &[ProjectSourceUnitLayout] {
        &self.fields.source_layouts
    }

    pub fn item_index(&self) -> &ProjectItemIndex {
        &self.fields.item_index
    }

    pub fn stable_check_owner_keys(&self) -> impl Iterator<Item = StableCheckOwnerKey> + '_ {
        self.fields
            .units
            .iter()
            .flat_map(|unit| unit.stable_check_owner_keys())
    }

    pub fn owner_view(&self, owner: &StableCheckOwnerKey) -> Option<UnitOwnerSyntaxView<'_>> {
        let unit = self
            .fields
            .units
            .iter()
            .find(|unit| &unit.source_unit_id == owner.source_unit_id())?;
        match owner {
            StableCheckOwnerKey::UnitRoot(_) => unit.owner_view(&UnitOwnerRoute::UnitRoot),
            StableCheckOwnerKey::Item(owner) => {
                unit.owner_view(&UnitOwnerRoute::Item(owner.item_route.clone()))
            }
        }
    }

    pub fn functions(&self) -> &[String] {
        &self.fields.functions
    }

    pub fn operators(&self) -> &[String] {
        &self.fields.operators
    }

    pub fn expression_count(&self) -> usize {
        self.fields.expression_count
    }

    /// Reachable parser expressions assigned to exactly one type-check owner.
    /// The raw arena can retain parser-internal expressions that are no longer
    /// part of the final semantic graph; those rows are intentionally excluded.
    pub fn check_expression_count(&self) -> usize {
        self.fields.check_expression_count
    }

    pub fn statement_count(&self) -> usize {
        self.fields.statement_count
    }

    /// Dense checked-artifact slot for a project-local expression lookup id.
    ///
    /// Slots are a legacy/output projection only. Parser nodes and persistent
    /// request keys retain the unit namespace plus local id and are never
    /// rewritten when an earlier unit changes size.
    pub fn expression_slot(&self, expression_id: usize) -> Option<usize> {
        let (namespace, local) = __parser_unpack_syntax_node_id(expression_id)?;
        let unit = *self.fields.unit_by_namespace.get(&namespace)?;
        (local < self.fields.units.get(unit)?.ast.expressions.len())
            .then(|| self.fields.expression_offsets[unit].checked_add(local))
            .flatten()
    }

    pub fn expression_id_for_slot(&self, slot: usize) -> Option<usize> {
        if slot >= self.fields.expression_count {
            return None;
        }
        let unit = self
            .fields
            .expression_offsets
            .partition_point(|offset| *offset <= slot)
            .checked_sub(1)?;
        __parser_pack_syntax_node_id(
            self.fields.units.get(unit)?.namespace,
            slot.checked_sub(self.fields.expression_offsets[unit])?,
        )
    }

    pub fn statement_slot(&self, statement_id: usize) -> Option<usize> {
        let (namespace, local) = __parser_unpack_syntax_node_id(statement_id)?;
        let unit = *self.fields.unit_by_namespace.get(&namespace)?;
        let end = self
            .fields
            .statement_offsets
            .get(unit + 1)
            .copied()
            .unwrap_or(self.fields.statement_count);
        (local < end.checked_sub(self.fields.statement_offsets[unit])?)
            .then(|| self.fields.statement_offsets[unit].checked_add(local))
            .flatten()
    }

    pub fn statement_id_for_slot(&self, slot: usize) -> Option<usize> {
        if slot >= self.fields.statement_count {
            return None;
        }
        let unit = self
            .fields
            .statement_offsets
            .partition_point(|offset| *offset <= slot)
            .checked_sub(1)?;
        __parser_pack_syntax_node_id(
            self.fields.units.get(unit)?.namespace,
            slot.checked_sub(self.fields.statement_offsets[unit])?,
        )
    }

    pub fn expressions(&self) -> ProjectExpressionArena<'_> {
        ProjectExpressionArena { project: self }
    }

    pub fn root_statement_units(&self) -> ProjectRootStatementUnits<'_> {
        ProjectRootStatementUnits {
            units: self.fields.units.iter(),
        }
    }

    pub fn expression_source_layout(
        &self,
        expression_id: usize,
    ) -> Option<&ProjectSourceUnitLayout> {
        let (namespace, _) = __parser_unpack_syntax_node_id(expression_id)?;
        let index = *self.fields.unit_by_namespace.get(&namespace)?;
        self.fields.source_layouts.get(index)
    }

    pub fn stable_occurrence_key(&self, expression_id: usize) -> Option<StableOccurrenceKey> {
        let (namespace, _) = __parser_unpack_syntax_node_id(expression_id)?;
        let unit = self
            .fields
            .units
            .get(*self.fields.unit_by_namespace.get(&namespace)?)?;
        unit.stable_occurrence_key(expression_id)
    }

    pub fn stable_check_owner_for_statement(
        &self,
        statement_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let (namespace, _) = __parser_unpack_syntax_node_id(statement_id)?;
        let unit = self
            .fields
            .units
            .get(*self.fields.unit_by_namespace.get(&namespace)?)?;
        unit.stable_check_owner_for_statement(statement_id)
    }

    pub fn stable_check_owner_for_expression(
        &self,
        expression_id: usize,
    ) -> Option<StableCheckOwnerKey> {
        let (namespace, _) = __parser_unpack_syntax_node_id(expression_id)?;
        let unit = self
            .fields
            .units
            .get(*self.fields.unit_by_namespace.get(&namespace)?)?;
        unit.stable_check_owner_for_expression(expression_id)
    }

    pub fn stable_statement_key(&self, statement_id: usize) -> Option<StableStatementKey> {
        let (namespace, _) = __parser_unpack_syntax_node_id(statement_id)?;
        let unit = self
            .fields
            .units
            .get(*self.fields.unit_by_namespace.get(&namespace)?)?;
        unit.stable_statement_key(statement_id)
    }

    pub fn stable_expression_key(&self, expression_id: usize) -> Option<StableExpressionKey> {
        let (namespace, _) = __parser_unpack_syntax_node_id(expression_id)?;
        let unit = self
            .fields
            .units
            .get(*self.fields.unit_by_namespace.get(&namespace)?)?;
        unit.stable_expression_key(expression_id)
    }

    pub fn global_line(&self, syntax_id: usize, local_line: usize) -> Option<usize> {
        let layout = self.expression_source_layout(syntax_id)?;
        layout.start_line.checked_add(local_line.checked_sub(1)?)
    }

    pub fn global_byte(&self, syntax_id: usize, local_byte: usize) -> Option<usize> {
        let layout = self.expression_source_layout(syntax_id)?;
        (local_byte <= layout.source_len)
            .then(|| layout.start_byte.checked_add(local_byte))
            .flatten()
    }
}

fn unit_native_statement_count(
    path: &str,
    statements: &[AstStatement],
    namespace: SyntaxUnitNamespace,
    expected_local: &mut usize,
) -> Result<usize, ParseError> {
    let mut count = 0usize;
    for statement in statements {
        let Some((actual_namespace, local)) = __parser_unpack_syntax_node_id(statement.id) else {
            return Err(parsed_source_unit_invariant_error(
                path,
                "unit-native statement has an untagged id",
            ));
        };
        if actual_namespace != namespace || local != *expected_local {
            return Err(parsed_source_unit_invariant_error(
                path,
                format!(
                    "unit-native statement id {} does not match namespace {} local id {}",
                    statement.id,
                    namespace.get(),
                    *expected_local,
                ),
            ));
        }
        *expected_local = expected_local.checked_add(1).ok_or_else(|| {
            parsed_source_unit_invariant_error(path, "unit statement count overflows usize")
        })?;
        count = count.checked_add(1).ok_or_else(|| {
            parsed_source_unit_invariant_error(path, "unit statement count overflows usize")
        })?;
        count = count
            .checked_add(unit_native_statement_count(
                path,
                &statement.children,
                namespace,
                expected_local,
            )?)
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(path, "unit statement count overflows usize")
            })?;
    }
    Ok(count)
}

#[derive(Clone, Copy)]
pub struct ProjectExpressionArena<'a> {
    project: &'a ProjectSyntaxSnapshot,
}

impl<'a> ProjectExpressionArena<'a> {
    pub fn len(self) -> usize {
        self.project.expression_count()
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn get(self, expression_id: usize) -> Option<&'a AstExpr> {
        let (namespace, local) = __parser_unpack_syntax_node_id(expression_id)?;
        let unit = self
            .project
            .fields
            .units
            .get(*self.project.fields.unit_by_namespace.get(&namespace)?)?;
        let expression = unit.ast.expressions.get(local)?;
        (expression.id == expression_id).then_some(expression)
    }

    pub fn iter(self) -> ProjectExpressionIter<'a> {
        ProjectExpressionIter {
            units: self.project.fields.units.iter(),
            expressions: None,
        }
    }
}

pub struct ProjectExpressionIter<'a> {
    units: std::slice::Iter<'a, std::sync::Arc<UnitSyntaxSnapshot>>,
    expressions: Option<std::slice::Iter<'a, AstExpr>>,
}

impl<'a> Iterator for ProjectExpressionIter<'a> {
    type Item = &'a AstExpr;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(expression) = self.expressions.as_mut().and_then(Iterator::next) {
                return Some(expression);
            }
            self.expressions = Some(self.units.next()?.ast.expressions.iter());
        }
    }
}

#[derive(Clone)]
pub struct ProjectRootStatementUnits<'a> {
    units: std::slice::Iter<'a, std::sync::Arc<UnitSyntaxSnapshot>>,
}

impl<'a> Iterator for ProjectRootStatementUnits<'a> {
    type Item = &'a [AstStatement];

    fn next(&mut self) -> Option<Self::Item> {
        self.units.next().map(|unit| unit.ast.statements.as_slice())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Deterministic parser work recorded by the explicit profiled entrypoints.
///
/// These counters describe loops and node visits performed by the parser. They
/// deliberately contain no wall-clock time, allocation estimates, or values
/// derived after parsing from the size of the returned AST.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ParseWorkCounters {
    /// Source units that reached the parser's per-unit preflight.
    pub source_units_attempted: usize,
    /// Source units whose context-independent syntax artifact was issued.
    pub source_units_parsed: usize,
    /// Source units whose previously parsed artifact was reused by a session.
    pub source_units_reused: usize,
    /// Source bytes inspected by lexer scanning and position tracking.
    pub source_bytes_inspected: usize,
    /// Token visits performed while constructing and validating syntax.
    pub token_inspections: usize,
    /// Logical symbol visits performed while constructing and validating syntax.
    pub symbol_inspections: usize,
    /// Statement visits performed by construction, linking, validation, and assembly.
    pub statement_visits: usize,
    /// Expression visits performed by construction, normalization, validation, and assembly.
    pub expression_visits: usize,
    /// Token, line, item, statement, and expression nodes rebased during project assembly.
    pub nodes_rebased: usize,
    /// Token, item, statement, and expression visits owned by syntax validation.
    pub validation_visits: usize,
}

impl ParseWorkCounters {
    pub fn accumulate(&mut self, other: Self) {
        self.source_units_attempted = self
            .source_units_attempted
            .saturating_add(other.source_units_attempted);
        self.source_units_parsed = self
            .source_units_parsed
            .saturating_add(other.source_units_parsed);
        self.source_units_reused = self
            .source_units_reused
            .saturating_add(other.source_units_reused);
        self.source_bytes_inspected = self
            .source_bytes_inspected
            .saturating_add(other.source_bytes_inspected);
        self.token_inspections = self
            .token_inspections
            .saturating_add(other.token_inspections);
        self.symbol_inspections = self
            .symbol_inspections
            .saturating_add(other.symbol_inspections);
        self.statement_visits = self.statement_visits.saturating_add(other.statement_visits);
        self.expression_visits = self
            .expression_visits
            .saturating_add(other.expression_visits);
        self.nodes_rebased = self.nodes_rebased.saturating_add(other.nodes_rebased);
        self.validation_visits = self
            .validation_visits
            .saturating_add(other.validation_visits);
    }

    pub fn record_reused_source_units(&mut self, count: usize) {
        self.source_units_reused = self.source_units_reused.saturating_add(count);
    }
}

/// Timer-free parser profile returned alongside success or failure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ParseProfile {
    pub work_counters: ParseWorkCounters,
}

struct ParseWorkRecorder {
    enabled: bool,
    source_units_attempted: Cell<usize>,
    source_units_parsed: Cell<usize>,
    source_units_reused: Cell<usize>,
    source_bytes_inspected: Cell<usize>,
    token_inspections: Cell<usize>,
    symbol_inspections: Cell<usize>,
    statement_visits: Cell<usize>,
    expression_visits: Cell<usize>,
    nodes_rebased: Cell<usize>,
    validation_visits: Cell<usize>,
}

impl ParseWorkRecorder {
    fn with_enabled(enabled: bool) -> Self {
        Self {
            enabled,
            source_units_attempted: Cell::new(0),
            source_units_parsed: Cell::new(0),
            source_units_reused: Cell::new(0),
            source_bytes_inspected: Cell::new(0),
            token_inspections: Cell::new(0),
            symbol_inspections: Cell::new(0),
            statement_visits: Cell::new(0),
            expression_visits: Cell::new(0),
            nodes_rebased: Cell::new(0),
            validation_visits: Cell::new(0),
        }
    }

    fn disabled() -> Self {
        Self::with_enabled(false)
    }

    fn enabled() -> Self {
        Self::with_enabled(true)
    }

    #[inline]
    fn batch(&self) -> ParseWorkBatch<'_> {
        ParseWorkBatch {
            work: self,
            token_inspections: 0,
            symbol_inspections: 0,
            statement_visits: 0,
            expression_visits: 0,
            validation_visits: 0,
        }
    }

    #[inline]
    fn add(&self, counter: &Cell<usize>, count: usize) {
        if self.enabled {
            counter.set(counter.get().saturating_add(count));
        }
    }

    #[inline]
    fn source_unit_attempted(&self) {
        self.add(&self.source_units_attempted, 1);
    }

    #[inline]
    fn source_unit_parsed(&self) {
        self.add(&self.source_units_parsed, 1);
    }

    #[inline]
    fn source_bytes(&self, count: usize) {
        self.add(&self.source_bytes_inspected, count);
    }

    #[inline]
    fn tokens(&self, count: usize) {
        self.add(&self.token_inspections, count);
    }

    #[inline]
    fn symbols(&self, count: usize) {
        self.add(&self.symbol_inspections, count);
    }

    #[inline]
    fn statement(&self) {
        self.add(&self.statement_visits, 1);
    }

    #[inline]
    fn expression(&self) {
        self.add(&self.expression_visits, 1);
    }

    #[inline]
    fn rebased_node(&self) {
        self.add(&self.nodes_rebased, 1);
    }

    fn finish(&self) -> ParseProfile {
        ParseProfile {
            work_counters: ParseWorkCounters {
                source_units_attempted: self.source_units_attempted.get(),
                source_units_parsed: self.source_units_parsed.get(),
                source_units_reused: self.source_units_reused.get(),
                source_bytes_inspected: self.source_bytes_inspected.get(),
                token_inspections: self.token_inspections.get(),
                symbol_inspections: self.symbol_inspections.get(),
                statement_visits: self.statement_visits.get(),
                expression_visits: self.expression_visits.get(),
                nodes_rebased: self.nodes_rebased.get(),
                validation_visits: self.validation_visits.get(),
            },
        }
    }
}

/// Function-local parser work that is merged into the shared recorder once.
///
/// Validation functions return at the first malformed construct, so a drop
/// guard is intentional: every early `?`/`return` flushes exactly the visits
/// completed before the diagnostic without paying one interior-mutable update
/// for every node in a large valid project.
struct ParseWorkBatch<'a> {
    work: &'a ParseWorkRecorder,
    token_inspections: usize,
    symbol_inspections: usize,
    statement_visits: usize,
    expression_visits: usize,
    validation_visits: usize,
}

impl ParseWorkBatch<'_> {
    #[inline]
    fn tokens(&mut self, count: usize) {
        self.token_inspections = self.token_inspections.saturating_add(count);
    }

    #[inline]
    fn symbols(&mut self, count: usize) {
        self.symbol_inspections = self.symbol_inspections.saturating_add(count);
    }

    #[inline]
    fn statement(&mut self) {
        self.statement_visits = self.statement_visits.saturating_add(1);
    }

    #[inline]
    fn expression(&mut self) {
        self.expression_visits = self.expression_visits.saturating_add(1);
    }

    #[inline]
    fn validation(&mut self) {
        self.validation_visits = self.validation_visits.saturating_add(1);
    }
}

impl Drop for ParseWorkBatch<'_> {
    fn drop(&mut self) {
        self.work.tokens(self.token_inspections);
        self.work.symbols(self.symbol_inspections);
        self.work
            .add(&self.work.statement_visits, self.statement_visits);
        self.work
            .add(&self.work.expression_visits, self.expression_visits);
        self.work
            .add(&self.work.validation_visits, self.validation_visits);
    }
}

/// Ephemeral facts shared by the ordered parser validation pipeline.
///
/// This index is deliberately neither serialized nor retained by
/// `ParsedProgram`: it removes repeated document-mask and source-token scans
/// while the existing validator call order remains the diagnostic authority.
struct ValidationIndex {
    semantic_token_ids: Box<[usize]>,
    semantic_token_line_ranges: Box<[(usize, usize)]>,
    semantic_item_ids: Box<[usize]>,
    drain_token_ids: Box<[usize]>,
    text_literal_spans: Box<[(usize, usize)]>,
    list_sites: Box<[(usize, usize)]>,
    bits_sites: Box<[(usize, usize)]>,
    bytes_sites: Box<[(usize, usize)]>,
    bits_encoded_spans: Box<[(usize, usize)]>,
}

#[derive(Clone, Copy)]
enum ValidationIndexScope {
    Boundary,
    Full,
}

impl ValidationIndex {
    fn build(ast: &AstProgram, work: &ParseWorkRecorder) -> Self {
        Self::build_scoped(ast, work, ValidationIndexScope::Full)
    }

    fn build_boundary(ast: &AstProgram, work: &ParseWorkRecorder) -> Self {
        Self::build_scoped(ast, work, ValidationIndexScope::Boundary)
    }

    fn build_scoped(
        ast: &AstProgram,
        work: &ParseWorkRecorder,
        scope: ValidationIndexScope,
    ) -> Self {
        let document_lines = document_line_mask_with_work(ast, work);
        let mut work_batch = work.batch();
        let mut text_literal_spans = ast
            .expressions
            .iter()
            .filter_map(|expression| {
                work_batch.expression();
                work_batch.validation();
                matches!(expression.kind, AstExprKind::TextLiteral(_))
                    .then_some((expression.start, expression.end))
            })
            .collect::<Vec<_>>();
        text_literal_spans.extend(text_literal_token_spans_with_work(&ast.tokens, work));
        let text_literal_spans = normalized_source_spans(text_literal_spans).into_boxed_slice();

        if matches!(scope, ValidationIndexScope::Boundary) {
            let semantic_item_ids = ast
                .items
                .iter()
                .enumerate()
                .find_map(|(item_id, item)| {
                    work_batch.validation();
                    (!document_lines.get(item.line).copied().unwrap_or(false)).then_some(item_id)
                })
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice();
            return Self {
                semantic_token_ids: Box::new([]),
                semantic_token_line_ranges: Box::new([]),
                semantic_item_ids,
                drain_token_ids: Box::new([]),
                text_literal_spans,
                list_sites: Box::new([]),
                bits_sites: Box::new([]),
                bytes_sites: Box::new([]),
                bits_encoded_spans: Box::new([]),
            };
        }

        let maximum_line = ast
            .tokens
            .last()
            .map_or(0, |token| token.line)
            .max(ast.lines.last().map_or(0, |line| line.line));
        let mut semantic_token_ids = Vec::new();
        let mut semantic_token_line_ranges = vec![(usize::MAX, usize::MAX); maximum_line + 1];
        let mut drain_token_ids = Vec::new();
        for (token_id, token) in ast.tokens.iter().enumerate() {
            work_batch.tokens(1);
            work_batch.validation();
            if !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::String)
                && !document_lines.get(token.line).copied().unwrap_or(false)
            {
                let position = semantic_token_ids.len();
                semantic_token_ids.push(token_id);
                if let Some((start, end)) = semantic_token_line_ranges.get_mut(token.line) {
                    if *start == usize::MAX {
                        *start = position;
                    }
                    *end = position + 1;
                }
            }
            if !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::Newline) {
                let containing_text =
                    containing_source_span(&text_literal_spans, token.start, token.end);
                if containing_text.is_none_or(|(start, _)| token.start == start) {
                    drain_token_ids.push(token_id);
                }
            }
        }

        let mut list_sites = Vec::new();
        for (line_id, line) in ast.lines.iter().enumerate() {
            work_batch.validation();
            if line.symbols.is_empty() || document_lines.get(line.line).copied().unwrap_or(false) {
                continue;
            }
            work_batch.symbols(line.symbols.len());
            if let Some(list_index) = line.symbols.iter().position(|lexeme| lexeme == "LIST") {
                list_sites.push((line_id, list_index));
            }
        }

        let mut semantic_item_ids = Vec::new();
        let mut bits_sites = Vec::new();
        let mut bytes_sites = Vec::new();
        for (item_id, item) in ast.items.iter().enumerate() {
            work_batch.validation();
            if !document_lines.get(item.line).copied().unwrap_or(false) {
                semantic_item_ids.push(item_id);
            }
            work_batch.symbols(item.symbols.len());
            for (symbol_index, symbol) in item.symbols.iter().enumerate() {
                match symbol.as_str() {
                    "BITS" => bits_sites.push((item_id, symbol_index)),
                    "BYTES" => bytes_sites.push((item_id, symbol_index)),
                    _ => {}
                }
            }
        }

        let bits_encoded_spans = bits_sites
            .iter()
            .filter_map(|(item_id, bits_index)| {
                let item = ast.items.get(*item_id)?;
                parse_bits_literal_tokens(bits_literal_tokens_at(&item.symbols, *bits_index))
                    .ok()
                    .and_then(|(_, _, _, base_index, suffix_index)| {
                        Some((
                            item.symbol_spans.get(*bits_index + base_index)?.0,
                            item.symbol_spans.get(*bits_index + suffix_index)?.1,
                        ))
                    })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            semantic_token_ids: semantic_token_ids.into_boxed_slice(),
            semantic_token_line_ranges: semantic_token_line_ranges.into_boxed_slice(),
            semantic_item_ids: semantic_item_ids.into_boxed_slice(),
            drain_token_ids: drain_token_ids.into_boxed_slice(),
            text_literal_spans,
            list_sites: list_sites.into_boxed_slice(),
            bits_sites: bits_sites.into_boxed_slice(),
            bytes_sites: bytes_sites.into_boxed_slice(),
            bits_encoded_spans,
        }
    }

    fn semantic_tokens<'a>(
        &'a self,
        ast: &'a AstProgram,
    ) -> impl Iterator<Item = &'a AstToken> + 'a {
        self.semantic_token_ids
            .iter()
            .filter_map(|index| ast.tokens.get(*index))
    }

    fn semantic_items<'a>(
        &'a self,
        ast: &'a AstProgram,
    ) -> impl Iterator<Item = &'a ParserItem> + 'a {
        self.semantic_item_ids
            .iter()
            .filter_map(|index| ast.items.get(*index))
    }

    fn semantic_tokens_on_line<'a>(
        &'a self,
        ast: &'a AstProgram,
        line: usize,
    ) -> impl Iterator<Item = &'a AstToken> + 'a {
        let range = self
            .semantic_token_line_ranges
            .get(line)
            .copied()
            .filter(|(start, end)| *start != usize::MAX && *end != usize::MAX)
            .unwrap_or((0, 0));
        self.semantic_token_ids[range.0..range.1]
            .iter()
            .filter_map(|index| ast.tokens.get(*index))
    }

    fn drain_tokens<'a>(&'a self, ast: &'a AstProgram) -> impl Iterator<Item = &'a AstToken> + 'a {
        self.drain_token_ids
            .iter()
            .filter_map(|index| ast.tokens.get(*index))
    }

    fn token_at_start<'a>(
        &self,
        ast: &'a AstProgram,
        start: usize,
        work: &ParseWorkRecorder,
    ) -> Option<&'a AstToken> {
        let mut work_batch = work.batch();
        let mut left = 0usize;
        let mut right = ast.tokens.len();
        while left < right {
            let middle = left + (right - left) / 2;
            work_batch.tokens(1);
            work_batch.validation();
            if ast.tokens[middle].start < start {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        ast.tokens.get(left).filter(|token| token.start == start)
    }
}

fn document_line_mask_with_work(ast: &AstProgram, work: &ParseWorkRecorder) -> Vec<bool> {
    fn mark(statement: &AstStatement, lines: &mut [bool], work: &mut ParseWorkBatch<'_>) {
        work.statement();
        work.validation();
        if let Some(line) = lines.get_mut(statement.line) {
            *line = true;
        }
        for child in &statement.children {
            mark(child, lines, work);
        }
    }

    let mut work_batch = work.batch();
    let Some(document) = ast.statements.iter().find(|statement| {
        work_batch.statement();
        work_batch.validation();
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "document"
        )
    }) else {
        return Vec::new();
    };
    let maximum_line = ast
        .tokens
        .last()
        .map_or(document.line, |token| token.line)
        .max(document.line);
    let mut lines = vec![false; maximum_line.saturating_add(1)];
    mark(document, &mut lines, &mut work_batch);
    lines
}

/// Parses one source unit without applying entrypoint/module context.
///
/// This is the cacheable raw parser boundary. It accepts multiline syntax
/// wholly contained by the unit, but requires lexical strings and explicit
/// delimiters to close before EOF and rejects a leading pipeline that would
/// need an input from another unit.
pub fn parse_source_unit(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<ParsedSourceUnit, ParseError> {
    parse_source_unit_with_work(path, source, &ParseWorkRecorder::disabled())
}

/// Parses one independent source unit and returns deterministic work even when
/// parsing fails.
pub fn parse_source_unit_profiled(
    path: impl Into<String>,
    source: impl Into<String>,
) -> (Result<ParsedSourceUnit, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let parsed = parse_source_unit_with_work(path, source, &work);
    (parsed, work.finish())
}

/// Parses one project source unit with the exact context-independent boundary
/// used by [`parse_project`]. Project-wide source policy is intentionally
/// deferred until the cached units are assembled.
pub fn parse_project_source_unit(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<ParsedSourceUnit, ParseError> {
    parse_project_source_unit_with_work(path, source, &ParseWorkRecorder::disabled())
}

/// Profiled cacheable project-unit parser. This is the persistent compiler
/// session's parse miss route; cache hits do not call it.
pub fn parse_project_source_unit_profiled(
    path: impl Into<String>,
    source: impl Into<String>,
) -> (Result<ParsedSourceUnit, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let parsed = parse_project_source_unit_with_work(path, source, &work);
    (parsed, work.finish())
}

/// Parse a project directly into immutable unit-native syntax.
///
/// Unlike [`parse_project`], this route never concatenates source, clones AST
/// nodes into one arena, rebases unit-local positions, or performs a second
/// whole-bundle AST validation. It is the cold counterpart of the persistent
/// compiler session's retained-unit route.
pub fn parse_project_syntax(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
) -> Result<ProjectSyntaxSnapshot, ParseError> {
    parse_project_syntax_with_work(entrypoint, files, &ParseWorkRecorder::disabled())
}

pub fn parse_project_syntax_profiled(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
) -> (Result<ProjectSyntaxSnapshot, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let parsed = parse_project_syntax_with_work(entrypoint, files, &work);
    (parsed, work.finish())
}

fn parse_project_syntax_with_work(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
    work: &ParseWorkRecorder,
) -> Result<ProjectSyntaxSnapshot, ParseError> {
    let entrypoint = entrypoint.into();
    let files = files.into_iter().collect::<Vec<_>>();
    let bundle = CanonicalSourceBundleV1::new(
        &entrypoint,
        files
            .iter()
            .map(|(path, source)| SourceBundleUnit::new(path, source)),
    )
    .map_err(|error| source_bundle_parse_error(&entrypoint, error))?;
    let entrypoint = bundle.entrypoint().to_owned();
    let mut raw_units = Vec::with_capacity(bundle.units().len());
    for unit in bundle.units() {
        work.source_unit_attempted();
        reject_reserved_module_path(unit.path())?;
        let source_unit_id = SourceUnitId::from_path(unit.path())
            .map_err(|error| source_unit_parse_error(unit.path(), error))?;
        let (parsed, validation_index) = parse_normalized_source_unit_syntax(
            source_unit_id,
            unit.source().to_owned(),
            ParserTrace::from_environment(),
            ValidationIndexScope::Full,
            work,
        )?;
        validate_project_source_unit_with_work(&parsed, &validation_index, work)?;
        raw_units.push(parsed);
    }
    drop(bundle);
    let link_keys = project_unit_link_keys(
        &entrypoint,
        raw_units
            .iter()
            .map(|unit| (unit.source_unit_id.clone(), unit.declared_functions.clone())),
    )?;
    let mut units = Vec::with_capacity(raw_units.len());
    for unit in raw_units {
        let link_key = link_keys
            .get(&unit.source_unit_id)
            .cloned()
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(&unit.path, "unit has no project link key")
            })?;
        units.push(std::sync::Arc::new(into_unit_syntax_snapshot_with_work(
            unit, link_key, work,
        )?));
    }
    ProjectSyntaxSnapshot::from_unit_snapshots(&entrypoint, units)
}

fn parse_project_source_unit_with_work(
    path: impl Into<String>,
    source: impl Into<String>,
    work: &ParseWorkRecorder,
) -> Result<ParsedSourceUnit, ParseError> {
    work.source_unit_attempted();
    let input_path = path.into();
    let source = source.into();
    let source_unit_id = SourceUnitId::from_path(&input_path)
        .map_err(|error| source_unit_parse_error(&input_path, error))?;
    let path = source_unit_id.as_str().to_owned();
    reject_reserved_module_path(&path)?;
    let (parsed, validation_index) = parse_normalized_source_unit_syntax(
        source_unit_id,
        source,
        ParserTrace::from_environment(),
        ValidationIndexScope::Full,
        work,
    )?;
    validate_project_source_unit_with_work(&parsed, &validation_index, work)?;
    Ok(parsed)
}

fn parse_source_unit_with_work(
    path: impl Into<String>,
    source: impl Into<String>,
    work: &ParseWorkRecorder,
) -> Result<ParsedSourceUnit, ParseError> {
    work.source_unit_attempted();
    let input_path = path.into();
    let source = source.into();
    let source_unit_id = SourceUnitId::from_path(&input_path)
        .map_err(|error| source_unit_parse_error(&input_path, error))?;
    let path = source_unit_id.as_str().to_owned();
    reject_reserved_module_path(&path)?;

    let (parsed, validation_index) = parse_normalized_source_unit_syntax(
        source_unit_id,
        source,
        ParserTrace::from_environment(),
        ValidationIndexScope::Full,
        work,
    )?;
    validate_source_syntax_with_work(&parsed.path, &parsed.ast, &validation_index, work)?;
    validate_list_capacities_with_work(&parsed.path, &parsed.ast, &validation_index, work)?;
    validate_no_hidden_identity_leak_with_work(&parsed.path, &parsed.ast, &validation_index, work)?;
    Ok(parsed)
}

/// Complete the source-policy checks owned by a project parser unit before it
/// can become an opaque [`ParsedSourceUnit`]. Project linking is deliberately
/// absent from this function: module qualification and packed lookup ids are
/// structure-preserving projections over this already-validated artifact.
fn validate_project_source_unit_with_work(
    parsed: &ParsedSourceUnit,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    validate_source_syntax_with_work(&parsed.path, &parsed.ast, validation_index, work)?;
    validate_list_capacities_with_work(&parsed.path, &parsed.ast, validation_index, work)?;
    validate_no_reducer_style_update_with_work(&parsed.path, &parsed.ast, validation_index, work)?;
    validate_no_hidden_identity_leak_with_work(&parsed.path, &parsed.ast, validation_index, work)?;
    Ok(())
}

/// Parses context-independent syntax for one already-normalized source unit.
///
/// The caller chooses a boundary-only index for the independent assembled
/// oracle or a complete index for the unit-native parser seal. In both cases,
/// source-unit boundary validation happens here before any project linking.
fn parse_normalized_source_unit_syntax(
    source_unit_id: SourceUnitId,
    source: String,
    trace: ParserTrace,
    validation_scope: ValidationIndexScope,
    work: &ParseWorkRecorder,
) -> Result<(ParsedSourceUnit, ValidationIndex), ParseError> {
    let path = source_unit_id.as_str().to_owned();
    let ast = parse_ast_traced(&path, &source, trace, work)?;
    let validation_index = match validation_scope {
        ValidationIndexScope::Boundary => ValidationIndex::build_boundary(&ast, work),
        ValidationIndexScope::Full => ValidationIndex::build(&ast, work),
    };
    validate_source_unit_boundary_with_work(&path, &source, &ast, &validation_index, work)?;
    let declared_functions = collect_raw_declared_functions_with_work(&ast.statements, work);
    let item_index = build_unit_item_index(&ast.statements);
    let expression_identities = build_unit_expression_identities(&path, &ast, &item_index)?;
    work.source_unit_parsed();

    Ok((
        ParsedSourceUnit::from_parser_fields(ParsedSourceUnitFields {
            source_unit_id,
            path,
            source,
            ast,
            declared_functions,
            item_index,
            owner_index: expression_identities.owner_index,
            occurrence_routes: expression_identities.occurrence_routes,
            expression_route_digests_v1: expression_identities.expression_route_digests_v1,
        }),
        validation_index,
    ))
}

pub fn parse_source(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<ParsedProgram, ParseError> {
    parse_source_with_work(path, source, &ParseWorkRecorder::disabled())
}

/// Parses one complete source bundle and returns deterministic parser work.
pub fn parse_source_profiled(
    path: impl Into<String>,
    source: impl Into<String>,
) -> (Result<ParsedProgram, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let parsed = parse_source_with_work(path, source, &work);
    (parsed, work.finish())
}

fn parse_source_with_work(
    path: impl Into<String>,
    source: impl Into<String>,
    work: &ParseWorkRecorder,
) -> Result<ParsedProgram, ParseError> {
    let path = path.into();
    let source = source.into();
    let bundle = CanonicalSourceBundleV1::new(
        &path,
        [SourceBundleUnit::new(path.as_str(), source.as_str())],
    )
    .map_err(|error| source_bundle_parse_error(&path, error))?;
    parse_canonical_source_bundle(bundle, work)
}

/// Parses a project whose first argument is the logical project-relative path
/// of the entrypoint unit.
pub fn parse_project(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
) -> Result<ParsedProgram, ParseError> {
    parse_project_with_work(entrypoint, files, &ParseWorkRecorder::disabled())
}

/// Parses a project and returns deterministic parser work alongside either
/// the accepted opaque artifact or the parse error.
pub fn parse_project_profiled(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
) -> (Result<ParsedProgram, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let parsed = parse_project_with_work(entrypoint, files, &work);
    (parsed, work.finish())
}

fn parse_project_with_work(
    entrypoint: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
    work: &ParseWorkRecorder,
) -> Result<ParsedProgram, ParseError> {
    let entrypoint = entrypoint.into();
    let files = files.into_iter().collect::<Vec<_>>();
    let bundle = CanonicalSourceBundleV1::new(
        &entrypoint,
        files
            .iter()
            .map(|(path, source)| SourceBundleUnit::new(path.as_str(), source.as_str())),
    )
    .map_err(|error| source_bundle_parse_error(&entrypoint, error))?;
    parse_canonical_source_bundle(bundle, work)
}

fn source_unit_parse_error(path: &str, error: SourceBundleError) -> ParseError {
    ParseError {
        path: path.to_owned(),
        line: None,
        column: None,
        message: format!("invalid source unit: {error}"),
    }
}

fn source_bundle_parse_error(
    entrypoint: &str,
    error: boon_contract::SourceBundleError,
) -> ParseError {
    ParseError {
        path: entrypoint.to_owned(),
        line: None,
        column: None,
        message: format!("invalid source bundle: {error}"),
    }
}

fn parsed_source_unit_invariant_error(path: &str, detail: impl fmt::Display) -> ParseError {
    ParseError {
        path: path.to_owned(),
        line: None,
        column: None,
        message: format!("invalid parsed source unit invariant: {detail}"),
    }
}

#[cfg(test)]
fn dense_statement_count(path: &str, statements: &[AstStatement]) -> Result<usize, ParseError> {
    dense_statement_count_with_work(path, statements, &ParseWorkRecorder::disabled())
}

fn dense_statement_count_with_work(
    path: &str,
    statements: &[AstStatement],
    work: &ParseWorkRecorder,
) -> Result<usize, ParseError> {
    fn visit(
        path: &str,
        statements: &[AstStatement],
        next_id: &mut usize,
        work: &ParseWorkRecorder,
    ) -> Result<(), ParseError> {
        for statement in statements {
            work.statement();
            if statement.id != *next_id {
                return Err(parsed_source_unit_invariant_error(
                    path,
                    format!(
                        "statement id {} is not the expected dense id {}",
                        statement.id, *next_id
                    ),
                ));
            }
            *next_id = (*next_id).checked_add(1).ok_or_else(|| {
                parsed_source_unit_invariant_error(path, "statement count overflows usize")
            })?;
            visit(path, &statement.children, next_id, work)?;
        }
        Ok(())
    }

    let mut count = 0usize;
    visit(path, statements, &mut count, work)?;
    Ok(count)
}

#[derive(Clone, Copy)]
struct SourceUnitAstRebase<'a> {
    path: &'a str,
    byte_offset: usize,
    line_offset: usize,
    expression_offset: usize,
    statement_offset: usize,
    local_source_len: usize,
    local_line_count: usize,
    local_expression_count: usize,
    local_statement_count: usize,
    work: &'a ParseWorkRecorder,
}

impl SourceUnitAstRebase<'_> {
    fn fail(&self, detail: impl fmt::Display) -> ParseError {
        parsed_source_unit_invariant_error(self.path, detail)
    }

    fn checked_offset(
        &self,
        value: usize,
        offset: usize,
        field: &str,
    ) -> Result<usize, ParseError> {
        value
            .checked_add(offset)
            .ok_or_else(|| self.fail(format!("{field} overflows usize while rebasing")))
    }

    fn line(&self, value: usize, field: &str) -> Result<usize, ParseError> {
        if value == 0 || value > self.local_line_count {
            return Err(self.fail(format!(
                "{field} line {value} is outside local lines 1..={}",
                self.local_line_count
            )));
        }
        self.checked_offset(value, self.line_offset, field)
    }

    fn span(&self, start: &mut usize, end: &mut usize, field: &str) -> Result<(), ParseError> {
        if *start > *end || *end > self.local_source_len {
            return Err(self.fail(format!(
                "{field} span {}..{} is outside local source bytes 0..={}",
                *start, *end, self.local_source_len
            )));
        }
        let rebased_start = self.checked_offset(*start, self.byte_offset, field)?;
        let rebased_end = self.checked_offset(*end, self.byte_offset, field)?;
        *start = rebased_start;
        *end = rebased_end;
        Ok(())
    }

    fn expression_id(&self, id: &mut usize, field: &str) -> Result<(), ParseError> {
        if *id >= self.local_expression_count {
            return Err(self.fail(format!(
                "{field} expression id {} is outside local arena length {}",
                *id, self.local_expression_count
            )));
        }
        *id = self.checked_offset(*id, self.expression_offset, field)?;
        Ok(())
    }

    fn optional_expression_id(
        &self,
        id: &mut Option<usize>,
        field: &str,
    ) -> Result<(), ParseError> {
        if let Some(id) = id {
            self.expression_id(id, field)?;
        }
        Ok(())
    }

    fn expression_ids(&self, ids: &mut [usize], field: &str) -> Result<(), ParseError> {
        for id in ids {
            self.expression_id(id, field)?;
        }
        Ok(())
    }

    fn statement_id(&self, id: &mut usize, field: &str) -> Result<(), ParseError> {
        if *id >= self.local_statement_count {
            return Err(self.fail(format!(
                "{field} statement id {} is outside local statement count {}",
                *id, self.local_statement_count
            )));
        }
        *id = self.checked_offset(*id, self.statement_offset, field)?;
        Ok(())
    }

    fn parser_line(&self, line: &mut ParserLine) -> Result<(), ParseError> {
        self.work.rebased_node();
        if line.symbols.len() != line.symbol_spans.len() {
            return Err(self.fail(format!(
                "parser line {} has {} symbols but {} symbol spans",
                line.line,
                line.symbols.len(),
                line.symbol_spans.len()
            )));
        }
        line.line = self.line(line.line, "parser line")?;
        self.span(&mut line.start, &mut line.end, "parser line")?;
        for span in &mut line.symbol_spans {
            self.span(&mut span.0, &mut span.1, "parser line symbol")?;
        }
        Ok(())
    }

    fn parser_item(&self, item: &mut ParserItem) -> Result<(), ParseError> {
        self.work.rebased_node();
        if item.symbols.len() != item.symbol_spans.len() {
            return Err(self.fail(format!(
                "parser item on line {} has {} symbols but {} symbol spans",
                item.line,
                item.symbols.len(),
                item.symbol_spans.len()
            )));
        }
        item.line = self.line(item.line, "parser item")?;
        self.span(&mut item.start, &mut item.end, "parser item")?;
        for span in &mut item.symbol_spans {
            self.span(&mut span.0, &mut span.1, "parser item symbol")?;
        }
        Ok(())
    }

    fn token(&self, token: &mut AstToken) -> Result<(), ParseError> {
        self.work.rebased_node();
        if token.column == 0 {
            return Err(self.fail("token column must be one-based"));
        }
        token.line = self.line(token.line, "token")?;
        self.span(&mut token.start, &mut token.end, "token")?;
        Ok(())
    }

    fn statement(&self, statement: &mut AstStatement) -> Result<(), ParseError> {
        self.work.statement();
        self.work.rebased_node();
        self.statement_id(&mut statement.id, "statement")?;
        statement.line = self.line(statement.line, "statement")?;
        self.span(&mut statement.start, &mut statement.end, "statement")?;
        self.optional_expression_id(&mut statement.expr, "statement expression")?;
        if let AstStatementKind::Function { parameters, .. } = &mut statement.kind {
            for parameter in parameters {
                self.span(
                    &mut parameter.start,
                    &mut parameter.end,
                    "function parameter",
                )?;
            }
        }
        for child in &mut statement.children {
            self.statement(child)?;
        }
        Ok(())
    }

    fn call_arg(&self, arg: &mut AstCallArg) -> Result<(), ParseError> {
        self.expression_id(&mut arg.value, "call argument value")?;
        self.span(&mut arg.start, &mut arg.end, "call argument")
    }

    fn pass_context(&self, pass: &mut AstPassContext) -> Result<(), ParseError> {
        self.expression_id(&mut pass.value, "PASS value")?;
        self.span(&mut pass.start, &mut pass.end, "PASS context")
    }

    fn record_field(&self, field: &mut AstRecordField) -> Result<(), ParseError> {
        self.expression_id(&mut field.value, "record field value")?;
        self.span(&mut field.start, &mut field.end, "record field")
    }

    fn expression(&self, expression: &mut AstExpr) -> Result<(), ParseError> {
        self.work.expression();
        self.work.rebased_node();
        self.expression_id(&mut expression.id, "expression")?;
        expression.line = self.line(expression.line, "expression")?;
        self.span(&mut expression.start, &mut expression.end, "expression")?;
        self.optional_expression_id(&mut expression.linked_input, "linked input")?;

        match &mut expression.kind {
            AstExprKind::TextTemplate { segments } => {
                for segment in segments {
                    if let AstTextSegment::Dynamic { value } = segment {
                        self.expression_id(value, "text template dynamic value")?;
                    }
                }
            }
            AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
                for field in fields {
                    self.record_field(field)?;
                }
            }
            AstExprKind::Flush { payload } => {
                self.optional_expression_id(payload, "FLUSH payload")?;
            }
            AstExprKind::Call { args, pass, .. } => {
                for arg in args {
                    self.call_arg(arg)?;
                }
                if let Some(pass) = pass {
                    self.pass_context(pass)?;
                }
            }
            AstExprKind::Pipe {
                input,
                args,
                pass,
                arms,
                ..
            } => {
                self.expression_id(input, "pipe input")?;
                for arg in args {
                    self.call_arg(arg)?;
                }
                if let Some(pass) = pass {
                    self.pass_context(pass)?;
                }
                self.expression_ids(arms, "pipe arm")?;
            }
            AstExprKind::Draining { input } => {
                self.expression_id(input, "DRAINING input")?;
            }
            AstExprKind::Hold { initial, .. } => {
                self.expression_id(initial, "HOLD initial value")?;
            }
            AstExprKind::Latest { branches } => {
                self.expression_ids(branches, "LATEST branch")?;
            }
            AstExprKind::When { input, arms } => {
                self.expression_id(input, "WHEN input")?;
                self.expression_ids(arms, "WHEN arm")?;
            }
            AstExprKind::Then { input, output } => {
                self.expression_id(input, "THEN input")?;
                self.optional_expression_id(output, "THEN output")?;
            }
            AstExprKind::Infix { left, right, .. } => {
                self.expression_id(left, "infix left operand")?;
                self.expression_id(right, "infix right operand")?;
            }
            AstExprKind::MatchArm { output, .. } => {
                self.optional_expression_id(output, "match-arm output")?;
            }
            AstExprKind::Block { bindings, result } => {
                for binding in bindings {
                    let AstBlockBindingDeclaration::Local { statement } = &mut binding.declaration
                    else {
                        return Err(self.fail(
                            "raw BLOCK binding contains an owner-normalized child declaration",
                        ));
                    };
                    self.statement_id(statement, "block binding statement")?;
                    self.expression_id(&mut binding.value, "block binding value")?;
                    self.span(&mut binding.start, &mut binding.end, "block binding")?;
                }
                self.optional_expression_id(result, "block result")?;
            }
            AstExprKind::ListLiteral { items, .. }
            | AstExprKind::BytesLiteral { items, .. }
            | AstExprKind::SetLiteral { items } => {
                self.expression_ids(items, "collection item")?;
            }
            AstExprKind::Arrow { left, output, .. } => {
                self.expression_id(left, "arrow left operand")?;
                self.optional_expression_id(output, "arrow output")?;
            }
            AstExprKind::MapEntry { key, value } => {
                self.expression_id(key, "map entry key")?;
                self.expression_id(value, "map entry value")?;
            }
            AstExprKind::MapLiteral { entries } => {
                self.expression_ids(entries, "map entry")?;
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
}

fn rebase_source_unit_ast(
    mut ast: AstProgram,
    rebase: SourceUnitAstRebase<'_>,
) -> Result<AstProgram, ParseError> {
    for (expected, expression) in ast.expressions.iter().enumerate() {
        rebase.work.expression();
        if expression.id != expected {
            return Err(rebase.fail(format!(
                "expression id {} is not the expected dense id {expected}",
                expression.id
            )));
        }
    }
    if ast.expressions.len() != rebase.local_expression_count {
        return Err(rebase.fail("expression arena length changed during assembly"));
    }
    let statement_count =
        dense_statement_count_with_work(rebase.path, &ast.statements, rebase.work)?;
    if statement_count != rebase.local_statement_count {
        return Err(rebase.fail("statement count changed during assembly"));
    }

    for token in &mut ast.tokens {
        rebase.token(token)?;
    }
    for line in &mut ast.lines {
        rebase.parser_line(line)?;
    }
    for item in &mut ast.items {
        rebase.parser_item(item)?;
    }
    for statement in &mut ast.statements {
        rebase.statement(statement)?;
    }
    for expression in ast.expressions.__parser_make_mut() {
        rebase.expression(expression)?;
    }
    Ok(ast)
}

fn eof_column(source: &str) -> Result<usize, ParseError> {
    source
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .chars()
        .count()
        .checked_add(1)
        .ok_or_else(|| {
            parsed_source_unit_invariant_error("<assembly>", "EOF column overflows usize")
        })
}

/// Canonicalizes and assembles previously parsed project units.
///
/// Persistent compiler sessions use this after reusing unchanged unit
/// artifacts and parsing only cache misses. The ordinary project parser calls
/// the canonical inner assembler directly because it already owns an ordered
/// source bundle.
pub fn assemble_parsed_source_units(
    entrypoint: &str,
    units: Vec<ParsedSourceUnit>,
) -> Result<ParsedProgram, ParseError> {
    assemble_parsed_source_units_with_work(entrypoint, units, &ParseWorkRecorder::disabled())
}

pub fn assemble_parsed_source_units_profiled(
    entrypoint: &str,
    units: Vec<ParsedSourceUnit>,
) -> (Result<ParsedProgram, ParseError>, ParseProfile) {
    let work = ParseWorkRecorder::enabled();
    let parsed = assemble_parsed_source_units_with_work(entrypoint, units, &work);
    (parsed, work.finish())
}

fn assemble_parsed_source_units_with_work(
    entrypoint: &str,
    mut units: Vec<ParsedSourceUnit>,
    work: &ParseWorkRecorder,
) -> Result<ParsedProgram, ParseError> {
    for unit in &units {
        if unit.path != unit.source_unit_id.as_str() {
            return Err(parsed_source_unit_invariant_error(
                &unit.path,
                format!(
                    "artifact path `{}` does not match source-unit id `{}`",
                    unit.path, unit.source_unit_id
                ),
            ));
        }
    }
    units.sort_by(|left, right| {
        left.source_unit_id
            .as_str()
            .as_bytes()
            .cmp(right.source_unit_id.as_str().as_bytes())
    });

    let bundle = CanonicalSourceBundleV1::new(
        entrypoint,
        units
            .iter()
            .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
    )
    .map_err(|error| source_bundle_parse_error(entrypoint, error))?;
    let entrypoint = bundle.entrypoint().to_owned();
    let source_bundle_digest_v1 = bundle.digest();
    drop(bundle);

    assemble_canonical_parsed_source_units(entrypoint, source_bundle_digest_v1, units, work)
}

/// Assembles source units already validated and ordered by a canonical bundle.
///
/// The public project route uses this directly so path sorting and source
/// hashing happen exactly once. The wrapper above remains useful for focused
/// invariant tests that deliberately supply arbitrary unit order.
fn assemble_canonical_parsed_source_units(
    entrypoint: String,
    source_bundle_digest_v1: SourceBundleDigestV1,
    units: Vec<ParsedSourceUnit>,
    work: &ParseWorkRecorder,
) -> Result<ParsedProgram, ParseError> {
    let trace = ParserTrace::from_environment();
    let assembly_started = trace.start();
    for unit in &units {
        if unit.path != unit.source_unit_id.as_str() {
            return Err(parsed_source_unit_invariant_error(
                &unit.path,
                format!(
                    "artifact path `{}` does not match source-unit id `{}`",
                    unit.path, unit.source_unit_id
                ),
            ));
        }
    }
    for adjacent in units.windows(2) {
        let left = adjacent[0].source_unit_id.as_str();
        let right = adjacent[1].source_unit_id.as_str();
        if left.as_bytes() >= right.as_bytes() {
            return Err(parsed_source_unit_invariant_error(
                right,
                format!("canonical source-unit order requires `{left}` to sort before `{right}`"),
            ));
        }
    }

    let mut statement_counts = Vec::with_capacity(units.len());
    let mut total_source_bytes = 0usize;
    let mut total_expression_count = 0usize;
    let mut total_statement_count = 0usize;
    let mut total_token_capacity = 0usize;
    let mut total_line_capacity = 0usize;
    let mut total_item_count = 0usize;
    for (index, unit) in units.iter().enumerate() {
        let statement_count =
            dense_statement_count_with_work(&unit.path, &unit.ast.statements, work)?;
        let declared_functions =
            collect_raw_declared_functions_with_work(&unit.ast.statements, work);
        if declared_functions != unit.declared_functions {
            return Err(parsed_source_unit_invariant_error(
                &unit.path,
                "raw declared-function metadata does not match the unit AST",
            ));
        }
        for (expected, expression) in unit.ast.expressions.iter().enumerate() {
            work.expression();
            if expression.id != expected {
                return Err(parsed_source_unit_invariant_error(
                    &unit.path,
                    format!(
                        "expression id {} is not the expected dense id {expected}",
                        expression.id
                    ),
                ));
            }
        }
        statement_counts.push(statement_count);
        total_statement_count = total_statement_count
            .checked_add(statement_count)
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled statement count overflows usize",
                )
            })?;
        total_expression_count = total_expression_count
            .checked_add(unit.ast.expressions.len())
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled expression count overflows usize",
                )
            })?;
        total_token_capacity = total_token_capacity
            .checked_add(unit.ast.tokens.len())
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled token count overflows usize",
                )
            })?;
        total_line_capacity = total_line_capacity
            .checked_add(unit.ast.lines.len())
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled parser-line count overflows usize",
                )
            })?;
        total_item_count = total_item_count
            .checked_add(unit.ast.items.len())
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled parser-item count overflows usize",
                )
            })?;
        total_source_bytes = total_source_bytes
            .checked_add(unit.source.len())
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled source length overflows usize",
                )
            })?;
        if index + 1 < units.len() && !unit.source.ends_with('\n') {
            total_source_bytes = total_source_bytes.checked_add(1).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled source separator overflows usize",
                )
            })?;
            total_token_capacity = total_token_capacity.checked_add(1).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled separator-token count overflows usize",
                )
            })?;
            total_line_capacity = total_line_capacity.checked_add(1).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    &unit.path,
                    "assembled separator-line capacity overflows usize",
                )
            })?;
        }
    }

    let modules = units
        .iter()
        .map(|unit| project_module_name_for_source_unit(&entrypoint, &unit.path))
        .collect::<Vec<_>>();
    let mut functions_by_module = BTreeMap::<String, BTreeSet<String>>::new();
    for (unit, module) in units.iter().zip(&modules) {
        let Some(module) = module else {
            continue;
        };
        functions_by_module
            .entry(module.clone())
            .or_default()
            .extend(unit.declared_functions.iter().cloned());
    }

    let unit_count = units.len();
    let mut source = String::new();
    source
        .try_reserve_exact(total_source_bytes)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled source bytes: {error}"),
            )
        })?;
    let mut files = Vec::new();
    files.try_reserve_exact(unit_count).map_err(|error| {
        parsed_source_unit_invariant_error(
            &entrypoint,
            format!("cannot reserve assembled source-file table: {error}"),
        )
    })?;
    let mut tokens = Vec::new();
    tokens
        .try_reserve_exact(total_token_capacity)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled token table: {error}"),
            )
        })?;
    let mut lines = Vec::new();
    lines
        .try_reserve_exact(total_line_capacity)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled parser-line table: {error}"),
            )
        })?;
    let mut items = Vec::new();
    items.try_reserve_exact(total_item_count).map_err(|error| {
        parsed_source_unit_invariant_error(
            &entrypoint,
            format!("cannot reserve assembled parser-item table: {error}"),
        )
    })?;
    let mut statements = Vec::new();
    statements
        .try_reserve_exact(total_statement_count)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled statement table: {error}"),
            )
        })?;
    let mut expressions = Vec::new();
    expressions
        .try_reserve_exact(total_expression_count)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled expression arena: {error}"),
            )
        })?;
    let mut occurrence_routes = Vec::new();
    occurrence_routes
        .try_reserve_exact(total_expression_count)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled occurrence routes: {error}"),
            )
        })?;
    let mut expression_route_digests_v1 = Vec::new();
    expression_route_digests_v1
        .try_reserve_exact(total_expression_count)
        .map_err(|error| {
            parsed_source_unit_invariant_error(
                &entrypoint,
                format!("cannot reserve assembled expression identities: {error}"),
            )
        })?;
    let mut next_line = 1usize;
    let mut statement_offset = 0usize;

    for (index, ((unit, local_statement_count), module)) in units
        .into_iter()
        .zip(statement_counts.into_iter())
        .zip(modules.into_iter())
        .enumerate()
    {
        let ParsedSourceUnitFields {
            source_unit_id: _source_unit_id,
            path,
            source: unit_source,
            mut ast,
            declared_functions: _declared_functions,
            item_index: _item_index,
            owner_index: _owner_index,
            occurrence_routes: unit_occurrence_routes,
            expression_route_digests_v1: unit_expression_route_digests_v1,
        } = unit.fields;
        if let Some(module) = module.as_deref() {
            namespace_source_unit_module(&mut ast, module, &functions_by_module, work);
        }
        let local_line_count = unit_source.lines().count().max(1);
        let start_line = next_line;
        let line_offset = start_line.checked_sub(1).ok_or_else(|| {
            parsed_source_unit_invariant_error(&path, "source start line underflows")
        })?;
        let byte_offset = source.len();
        let expression_offset = expressions.len();
        let local_expression_count = ast.expressions.len();
        if unit_occurrence_routes.len() != local_expression_count {
            return Err(parsed_source_unit_invariant_error(
                &path,
                "occurrence-route count does not match the expression arena",
            ));
        }
        if unit_expression_route_digests_v1.len() != local_expression_count {
            return Err(parsed_source_unit_invariant_error(
                &path,
                "expression-identity count does not match the expression arena",
            ));
        }
        let rebase = SourceUnitAstRebase {
            path: &path,
            byte_offset,
            line_offset,
            expression_offset,
            statement_offset,
            local_source_len: unit_source.len(),
            local_line_count,
            local_expression_count,
            local_statement_count,
            work,
        };
        let ast = rebase_source_unit_ast(ast, rebase)?;
        let AstProgram {
            tokens: mut unit_tokens,
            lines: mut unit_lines,
            items: unit_items,
            statements: unit_statements,
            expressions: unit_expressions,
        } = ast;

        source.push_str(&unit_source);
        tokens.append(&mut unit_tokens);
        lines.append(&mut unit_lines);
        items.extend(unit_items);
        statements.extend(unit_statements);
        expressions.extend(unit_expressions);
        occurrence_routes.extend(unit_occurrence_routes);
        expression_route_digests_v1.extend(unit_expression_route_digests_v1);

        let needs_separator = index + 1 < unit_count && !unit_source.ends_with('\n');
        if needs_separator {
            let separator_start = byte_offset.checked_add(unit_source.len()).ok_or_else(|| {
                parsed_source_unit_invariant_error(&path, "separator byte start overflows usize")
            })?;
            let separator_end = separator_start.checked_add(1).ok_or_else(|| {
                parsed_source_unit_invariant_error(&path, "separator byte end overflows usize")
            })?;
            let separator_line = rebase.line(local_line_count, "unit separator")?;
            let separator_column = eof_column(&unit_source).map_err(|mut error| {
                error.path.clone_from(&path);
                error
            })?;
            tokens.push(AstToken {
                kind: AstTokenKind::Newline,
                lexeme: "\n".to_owned(),
                line: separator_line,
                column: separator_column,
                start: separator_start,
                end: separator_end,
            });
            if let Some(line) = lines
                .last_mut()
                .filter(|line| line.line == separator_line && line.end == separator_start)
            {
                line.end = separator_end;
            } else {
                lines.push(ParserLine {
                    line: separator_line,
                    indent: separator_column.saturating_sub(1),
                    symbols: Vec::new().into(),
                    symbol_spans: Vec::new(),
                    start: separator_start,
                    end: separator_end,
                });
            }
            source.push('\n');
        }

        files.push(ParsedSourceFile {
            module,
            path,
            source: unit_source,
            start_line,
        });
        next_line = next_line.checked_add(local_line_count).ok_or_else(|| {
            parsed_source_unit_invariant_error(
                files
                    .last()
                    .map_or(entrypoint.as_str(), |file| file.path.as_str()),
                "assembled line count overflows usize",
            )
        })?;
        statement_offset = statement_offset
            .checked_add(local_statement_count)
            .ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    files
                        .last()
                        .map_or(entrypoint.as_str(), |file| file.path.as_str()),
                    "assembled statement offset overflows usize",
                )
            })?;
    }

    if source.len() != total_source_bytes
        || tokens.len() != total_token_capacity
        || items.len() != total_item_count
        || expressions.len() != total_expression_count
        || occurrence_routes.len() != total_expression_count
        || expression_route_digests_v1.len() != total_expression_count
        || statement_offset != total_statement_count
    {
        return Err(parsed_source_unit_invariant_error(
            &entrypoint,
            "assembled totals do not match their checked capacities",
        ));
    }

    let ast = AstProgram {
        tokens,
        lines,
        items,
        statements,
        expressions: expressions.into(),
    };
    let validation_index = ValidationIndex::build(&ast, work);
    let validations_started = trace.start();
    let kind = (|| {
        validate_source_syntax_with_work(&entrypoint, &ast, &validation_index, work)?;
        validate_balanced_brackets_with_work(&entrypoint, &ast, work)?;
        validate_list_capacities_with_work(&entrypoint, &ast, &validation_index, work)?;
        validate_no_reducer_style_update_with_work(&entrypoint, &ast, &validation_index, work)?;
        let kind = detect_program_kind();
        validate_no_hidden_identity_leak_with_work(&entrypoint, &ast, &validation_index, work)?;
        Ok::<_, ParseError>(kind)
    })()
    .map_err(|error| project_source_error(error, &entrypoint, &files))?;
    trace.phase(
        "canonical_bundle",
        "validations",
        validations_started,
        || {
            format!(
                "files={} root_statements={} expressions={}",
                files.len(),
                ast.statements.len(),
                ast.expressions.len(),
            )
        },
    );
    let functions = collect_functions_with_work(&ast, &validation_index, work);
    let operators = collect_operators_with_work(&ast, &validation_index, work);
    let expressions = ast.expressions.clone();
    let parsed = ParsedProgram::from_parser_fields(ParsedProgramFields {
        source_bundle_digest_v1,
        path: entrypoint,
        source,
        files,
        kind,
        expressions,
        functions,
        operators,
        occurrence_routes,
        expression_route_digests_v1,
        ast,
    });
    trace.phase(
        "canonical_bundle",
        "unit_assembly",
        assembly_started,
        || {
            format!(
                "files={} source_bytes={} tokens={} items={} expressions={}",
                parsed.files.len(),
                parsed.source.len(),
                parsed.ast.tokens.len(),
                parsed.ast.items.len(),
                parsed.ast.expressions.len(),
            )
        },
    );
    Ok(parsed)
}

/// Parses every canonical source unit independently, then applies project
/// context once during assembly. No concatenated-source parser fallback is
/// retained: a source-unit boundary is a hard syntax boundary.
fn parse_canonical_source_bundle(
    bundle: CanonicalSourceBundleV1<'_>,
    work: &ParseWorkRecorder,
) -> Result<ParsedProgram, ParseError> {
    let trace = ParserTrace::from_environment();
    let total_started = trace.start();
    let entrypoint = bundle.entrypoint().to_owned();
    let source_bundle_digest_v1 = bundle.digest();
    for unit in bundle.units() {
        work.source_unit_attempted();
        reject_reserved_module_path(unit.path())?;
    }

    let unit_parsing_started = trace.start();
    let units = bundle
        .units()
        .iter()
        .map(|unit| {
            let source_unit_id = SourceUnitId::from_path(unit.path())
                .map_err(|error| source_unit_parse_error(unit.path(), error))?;
            parse_normalized_source_unit_syntax(
                source_unit_id,
                unit.source().to_owned(),
                trace,
                ValidationIndexScope::Boundary,
                work,
            )
            .map(|(unit, _validation_index)| unit)
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    trace.phase(
        "canonical_bundle",
        "unit_parsing",
        unit_parsing_started,
        || {
            format!(
                "units={} source_bytes={} expressions={}",
                units.len(),
                units.iter().map(|unit| unit.source.len()).sum::<usize>(),
                units
                    .iter()
                    .map(|unit| unit.ast.expressions.len())
                    .sum::<usize>(),
            )
        },
    );

    drop(bundle);
    let parsed =
        assemble_canonical_parsed_source_units(entrypoint, source_bundle_digest_v1, units, work)?;
    trace.phase("canonical_bundle", "total", total_started, || {
        format!(
            "files={} source_bytes={} tokens={} items={} expressions={}",
            parsed.files.len(),
            parsed.source.len(),
            parsed.ast.tokens.len(),
            parsed.ast.items.len(),
            parsed.ast.expressions.len(),
        )
    });
    Ok(parsed)
}

fn project_source_error(
    mut error: ParseError,
    project_path: &str,
    files: &[ParsedSourceFile],
) -> ParseError {
    if error.path != project_path {
        return error;
    }
    let Some(global_line) = error.line else {
        return error;
    };
    let Some(file) = files
        .iter()
        .filter(|file| file.start_line <= global_line)
        .max_by_key(|file| file.start_line)
    else {
        return error;
    };
    error.path.clone_from(&file.path);
    error.line = Some(
        global_line
            .saturating_sub(file.start_line)
            .saturating_add(1),
    );
    error
}

/// Return the authored module namespace contributed by one project source unit.
///
/// The entrypoint is the root program and therefore has no module namespace.
/// Other files contribute a module only when their file stem starts with an
/// uppercase character. Compiler request planning uses this same function so
/// its per-module dependency partitions cannot drift from parser linking.
pub fn project_module_name_for_source_unit(entry_path: &str, file_path: &str) -> Option<String> {
    if entry_path == file_path {
        return None;
    }
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|stem| stem.to_str())?;
    if stem.chars().next().is_some_and(char::is_uppercase) {
        Some(stem.to_owned())
    } else {
        None
    }
}

fn reject_reserved_module_path(path: &str) -> Result<(), ParseError> {
    let module = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str());
    if module.is_some_and(is_reserved_standard_root) {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: format!(
                "`{}` is a reserved Boon standard namespace and cannot be declared by an application",
                module.unwrap_or_default()
            ),
        });
    }
    Ok(())
}

fn namespace_source_unit_module(
    ast: &mut AstProgram,
    module: &str,
    functions_by_module: &BTreeMap<String, BTreeSet<String>>,
    work: &ParseWorkRecorder,
) {
    namespace_statement_functions(&mut ast.statements, module, work);
    namespace_expr_functions(
        ast.expressions.__parser_make_mut(),
        module,
        functions_by_module,
        work,
    );
    namespace_parser_items(&mut ast.items, module, functions_by_module, work);
}

fn module_function_name(
    module: &str,
    function: &str,
    functions_by_module: &BTreeMap<String, BTreeSet<String>>,
) -> Option<String> {
    if function.contains('/') {
        return None;
    }
    functions_by_module
        .get(module)
        .is_some_and(|functions| functions.contains(function))
        .then(|| format!("{module}/{function}"))
}

fn namespace_statement_functions(
    statements: &mut [AstStatement],
    module: &str,
    work: &ParseWorkRecorder,
) {
    for statement in statements {
        work.statement();
        if let AstStatementKind::Function { name, .. } = &mut statement.kind
            && !name.contains('/')
        {
            *name = format!("{module}/{name}");
        }
        namespace_statement_functions(&mut statement.children, module, work);
    }
}

fn namespace_expr_functions(
    expressions: &mut [AstExpr],
    module: &str,
    functions_by_module: &BTreeMap<String, BTreeSet<String>>,
    work: &ParseWorkRecorder,
) {
    for expr in expressions {
        work.expression();
        match &mut expr.kind {
            AstExprKind::Call { function, .. } => {
                if let Some(namespaced) =
                    module_function_name(module, function, functions_by_module)
                {
                    *function = namespaced;
                }
            }
            AstExprKind::Pipe { op, .. } => {
                if let Some(namespaced) = module_function_name(module, op, functions_by_module) {
                    *op = namespaced;
                }
            }
            _ => {}
        }
    }
}

fn namespace_parser_items(
    items: &mut [ParserItem],
    module: &str,
    functions_by_module: &BTreeMap<String, BTreeSet<String>>,
    work: &ParseWorkRecorder,
) {
    for item in items {
        work.symbols(item.symbols.len());
        if let Some(function) = &mut item.function
            && !function.contains('/')
        {
            *function = format!("{module}/{function}");
        }
        for operator in &mut item.operators {
            if let Some(namespaced) = module_function_name(module, operator, functions_by_module) {
                *operator = namespaced;
            }
        }
    }
}

pub fn parsed_document(program: &ParsedProgram) -> Option<DocumentAst> {
    document_statement(&program.ast)
        .cloned()
        .map(|root| DocumentAst {
            root,
            expressions: program.ast.expressions.to_vec(),
        })
}

pub fn parsed_scene(program: &ParsedProgram) -> Option<DocumentAst> {
    scene_statement(&program.ast)
        .cloned()
        .map(|root| DocumentAst {
            root,
            expressions: program.ast.expressions.to_vec(),
        })
}

pub fn format_source(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<String, ParseError> {
    let path = path.into();
    let source = source.into();
    parse_source(path, source.clone())?;
    Ok(format_source_text(&source))
}

pub fn format_source_unit(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<String, ParseError> {
    let path = path.into();
    let source = source.into();
    let ast = parse_ast(&path, &source)?;
    let work = ParseWorkRecorder::disabled();
    let validation_index = ValidationIndex::build(&ast, &work);
    validate_source_syntax_with_work(&path, &ast, &validation_index, &work)?;
    validate_balanced_brackets_with_work(&path, &ast, &work)?;
    validate_list_capacities_with_work(&path, &ast, &validation_index, &work)?;
    Ok(format_source_text(&source))
}

fn format_source_text(source: &str) -> String {
    let mut formatted_lines = Vec::new();
    let mut previous_blank = false;
    for line in source.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.is_empty() {
            if !previous_blank {
                formatted_lines.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;
        let content = trimmed_end.trim_start_matches([' ', '\t']);
        let raw_indent_columns = trimmed_end
            .chars()
            .take_while(|character| *character == ' ' || *character == '\t')
            .map(|character| if character == '\t' { 4 } else { 1 })
            .sum::<usize>();
        let indent_columns = if raw_indent_columns > 0 {
            // Parser-gated indentation normalization: every non-empty source line
            // keeps its block depth, but mixed/two-space indentation is rewritten to
            // the canonical four-column editor grid after the parser has accepted
            // the source.
            raw_indent_columns.div_ceil(4) * 4
        } else {
            raw_indent_columns
        };
        formatted_lines.push(format!("{}{}", " ".repeat(indent_columns), content));
    }
    formatted_lines = compact_format_bracket_blocks(formatted_lines);
    while formatted_lines.last().is_some_and(|line| line.is_empty()) {
        formatted_lines.pop();
    }
    let mut formatted = formatted_lines.join("\n");
    formatted.push('\n');
    formatted
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FormatNode {
    Blank,
    Line {
        indent: usize,
        content: String,
    },
    BracketBlock {
        indent: usize,
        prefix: String,
        children: Vec<FormatNode>,
    },
}

fn compact_format_bracket_blocks(lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    let nodes = parse_format_nodes(&lines, &mut index, None);
    let mut formatted = Vec::new();
    render_format_nodes(&nodes, &mut formatted);
    formatted
}

fn parse_format_nodes(
    lines: &[String],
    index: &mut usize,
    close_indent: Option<usize>,
) -> Vec<FormatNode> {
    let mut nodes = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.is_empty() {
            nodes.push(FormatNode::Blank);
            *index += 1;
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let content = line[indent..].to_owned();
        if close_indent == Some(indent) && content == "]" {
            *index += 1;
            break;
        }
        if let Some(prefix) = format_bracket_block_prefix(&content) {
            *index += 1;
            let children = parse_format_nodes(lines, index, Some(indent));
            nodes.push(FormatNode::BracketBlock {
                indent,
                prefix,
                children,
            });
        } else {
            nodes.push(FormatNode::Line { indent, content });
            *index += 1;
        }
    }
    nodes
}

fn format_bracket_block_prefix(content: &str) -> Option<String> {
    let prefix = content.strip_suffix('[')?.trim_end();
    if prefix.contains("--") {
        return None;
    }
    Some(prefix.to_owned())
}

fn render_format_nodes(nodes: &[FormatNode], output: &mut Vec<String>) {
    let nonblank = nodes
        .iter()
        .filter(|node| !matches!(node, FormatNode::Blank))
        .collect::<Vec<_>>();
    let object_of_objects = nonblank.len() > 1
        && nonblank
            .iter()
            .all(|node| matches!(node, FormatNode::BracketBlock { .. }));
    let mut previous_multiline = false;
    let mut rendered_any = false;
    for node in nodes {
        if matches!(node, FormatNode::Blank) {
            if !object_of_objects {
                push_format_blank(output);
            }
            continue;
        }
        let multiline = format_node_inline_text(node).is_none();
        if object_of_objects && rendered_any && (previous_multiline || multiline) {
            push_format_blank(output);
        }
        render_format_node(node, output);
        previous_multiline = multiline;
        rendered_any = true;
    }
}

fn render_format_node(node: &FormatNode, output: &mut Vec<String>) {
    match node {
        FormatNode::Blank => push_format_blank(output),
        FormatNode::Line { indent, content } => {
            output.push(format!("{}{}", " ".repeat(*indent), content));
        }
        FormatNode::BracketBlock {
            indent,
            prefix,
            children,
        } => {
            if let Some(inline) = format_node_inline_text(node) {
                output.push(format!("{}{}", " ".repeat(*indent), inline));
                return;
            }
            output.push(format!("{}{}", " ".repeat(*indent), bracket_open(prefix)));
            render_format_nodes(children, output);
            output.push(format!("{}]", " ".repeat(*indent)));
        }
    }
}

fn push_format_blank(output: &mut Vec<String>) {
    if output.last().is_some_and(|line| line.is_empty()) {
        return;
    }
    output.push(String::new());
}

fn format_node_inline_text(node: &FormatNode) -> Option<String> {
    const MAX_INLINE_CHARS: usize = 96;
    let text = format_node_inline_text_unbounded(node)?;
    (text.chars().count() <= MAX_INLINE_CHARS).then_some(text)
}

fn format_node_inline_text_unbounded(node: &FormatNode) -> Option<String> {
    match node {
        FormatNode::Blank => None,
        FormatNode::Line { content, .. } => {
            if content.starts_with("--") {
                None
            } else {
                Some(content.clone())
            }
        }
        FormatNode::BracketBlock {
            prefix, children, ..
        } => {
            let nonblank = children
                .iter()
                .filter(|child| !matches!(child, FormatNode::Blank))
                .collect::<Vec<_>>();
            match nonblank.as_slice() {
                [] => Some(bracket_inline(prefix, "")),
                [child] => {
                    let child = format_node_inline_text_unbounded(child)?;
                    Some(bracket_inline(prefix, &child))
                }
                _ => None,
            }
        }
    }
}

fn bracket_open(prefix: &str) -> String {
    if prefix.is_empty() {
        "[".to_owned()
    } else {
        format!("{prefix} [")
    }
}

fn bracket_inline(prefix: &str, inner: &str) -> String {
    if prefix.is_empty() {
        format!("[{inner}]")
    } else if inner.is_empty() {
        format!("{prefix} []")
    } else {
        format!("{prefix} [{inner}]")
    }
}

pub fn parse_ast(path: &str, source: &str) -> Result<AstProgram, ParseError> {
    parse_ast_traced(
        path,
        source,
        ParserTrace::from_environment(),
        &ParseWorkRecorder::disabled(),
    )
}

fn parse_ast_traced(
    path: &str,
    source: &str,
    trace: ParserTrace,
    work: &ParseWorkRecorder,
) -> Result<AstProgram, ParseError> {
    let total_started = trace.start();
    let lex_started = trace.start();
    let tokens = lex_source_with_work(path, source, work)?;
    trace.phase("ast", "lex", lex_started, || {
        format!("source_bytes={} tokens={}", source.len(), tokens.len())
    });

    let line_merging_started = trace.start();
    let text_body_line_ranges = text_literal_body_line_ranges_with_work(&tokens, work);
    let lines = parser_lines(&tokens, work);
    let item_lines = merge_multiline_bytes_lines(lines.clone(), &text_body_line_ranges, work);
    let item_lines = merge_multiline_drain_lines(item_lines, &text_body_line_ranges, work);
    let item_lines =
        merge_multiline_call_expression_lines(item_lines, &text_body_line_ranges, work);
    trace.phase("ast", "line_merging", line_merging_started, || {
        format!(
            "logical_lines={} merged_item_lines={} text_body_line_ranges={}",
            lines.len(),
            item_lines.len(),
            text_body_line_ranges.len(),
        )
    });

    let items_started = trace.start();
    let item_line_count = item_lines.len();
    let items = parser_items(item_lines, &text_body_line_ranges, work);
    trace.phase("ast", "items", items_started, || {
        format!("item_lines={} items={}", item_line_count, items.len())
    });

    let ast_tree_started = trace.start();
    let mut expressions = Vec::new();
    let mut statements = ast_statement_tree(&items, &mut expressions, source, work);
    trace.phase("ast", "ast_tree", ast_tree_started, || {
        format!(
            "items={} root_statements={} expressions={}",
            items.len(),
            statements.len(),
            expressions.len(),
        )
    });

    let linking_started = trace.start();
    resolve_statement_arrow_contexts(&statements, &mut expressions, ArrowContext::None, work);
    link_multiline_expression_structure(&mut statements, &mut expressions, work);
    normalize_unlinked_unary_negation(&mut expressions, work);
    trace.phase("ast", "linking_normalization", linking_started, || {
        format!(
            "root_statements={} expressions={}",
            statements.len(),
            expressions.len(),
        )
    });

    let validations_started = trace.start();
    validate_pipeline_inputs(path, source, &expressions, work)?;
    let ast = AstProgram {
        tokens,
        lines,
        items,
        statements,
        expressions: expressions.into(),
    };
    validate_match_patterns(path, &ast, work)?;
    trace.phase("ast", "validations", validations_started, || {
        format!(
            "tokens={} root_statements={} expressions={}",
            ast.tokens.len(),
            ast.statements.len(),
            ast.expressions.len(),
        )
    });
    trace.phase("ast", "total", total_started, || {
        format!(
            "source_bytes={} tokens={} logical_lines={} items={} root_statements={} expressions={}",
            source.len(),
            ast.tokens.len(),
            ast.lines.len(),
            ast.items.len(),
            ast.statements.len(),
            ast.expressions.len(),
        )
    });
    Ok(ast)
}

#[derive(Clone, Copy)]
enum ArrowContext {
    None,
    Selector,
    Map,
}

fn resolve_statement_arrow_contexts(
    statements: &[AstStatement],
    expressions: &mut [AstExpr],
    context: ArrowContext,
    work: &ParseWorkRecorder,
) {
    for statement in statements {
        work.statement();
        if let Some(expression) = statement.expr {
            match context {
                ArrowContext::None => {}
                ArrowContext::Selector => consume_arrow_as_match_arm(expression, expressions),
                ArrowContext::Map => consume_arrow_as_map_entry(expression, expressions),
            }
        }
        let child_context = statement
            .expr
            .map(|expression| statement_structure_owner(expression, expressions))
            .and_then(|owner| expressions.get(owner))
            .map_or(ArrowContext::None, |expression| match &expression.kind {
                AstExprKind::When { .. } => ArrowContext::Selector,
                AstExprKind::Pipe { op, .. } if op == "WHILE" => ArrowContext::Selector,
                AstExprKind::MapLiteral { .. } => ArrowContext::Map,
                _ => ArrowContext::None,
            });
        resolve_statement_arrow_contexts(&statement.children, expressions, child_context, work);
    }
}

fn link_multiline_expression_structure(
    statements: &mut [AstStatement],
    expressions: &mut Vec<AstExpr>,
    work: &ParseWorkRecorder,
) {
    link_multiline_expression_structure_with_input(statements, expressions, None, work);
}

fn link_multiline_expression_structure_with_input(
    statements: &mut [AstStatement],
    expressions: &mut Vec<AstExpr>,
    mut previous: Option<usize>,
    work: &ParseWorkRecorder,
) {
    for statement in statements.iter_mut() {
        work.statement();
        if let Some(target) = statement_pipeline_continuation_target(statement, expressions)
            && let Some(input) = previous
            && let Some(expression) = expressions.get_mut(target)
        {
            expression.linked_input = Some(input);
            if let AstExprKind::Infix { left, .. } = &mut expression.kind {
                *left = input;
            }
        }
        let child_input = statement_child_pipeline_input(statement, expressions);
        link_multiline_expression_structure_with_input(
            &mut statement.children,
            expressions,
            child_input,
            work,
        );
        if let Some(base) = statement_structure_pipeline_base(statement, expressions) {
            relink_direct_structure_pipeline(&statement.children, expressions, base);
        }
        if let Some(output) = leading_pipeline_continuation_result(&statement.children, expressions)
        {
            replace_statement_inline_output(statement, expressions, output);
        }
        materialize_statement_structure(statement, expressions);
        let value = statement_value_expression(statement, expressions);
        if let Some(value) = value {
            previous = Some(value);
        }
    }
}

fn statement_pipeline_continuation_target(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    if matches!(
        statement.kind,
        AstStatementKind::Function { .. }
            | AstStatementKind::Field { .. }
            | AstStatementKind::Source { field: Some(_), .. }
            | AstStatementKind::Hold { field: Some(_), .. }
            | AstStatementKind::List { field: Some(_), .. }
    ) {
        return None;
    }
    pipeline_placeholder_target(statement.expr?, expressions)
}

fn normalize_unlinked_unary_negation(expressions: &mut [AstExpr], work: &ParseWorkRecorder) {
    let zero_expressions = expressions
        .iter()
        .filter_map(|expression| {
            work.expression();
            let AstExprKind::Infix { left, op, .. } = &expression.kind else {
                return None;
            };
            (expression.linked_input.is_none()
                && op == "-"
                && expressions
                    .get(*left)
                    .is_some_and(|left| matches!(left.kind, AstExprKind::Delimiter)))
            .then_some(*left)
        })
        .collect::<BTreeSet<_>>();
    for expression in zero_expressions {
        if let Some(expression) = expressions.get_mut(expression) {
            expression.kind = AstExprKind::Number("0".to_owned());
        }
    }
}

fn statement_is_pipeline_continuation(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    statement_pipeline_continuation_target(statement, expressions).is_some()
}

fn pipeline_placeholder_target(expr_id: usize, expressions: &[AstExpr]) -> Option<usize> {
    let expression = expressions.get(expr_id)?;
    if expression.linked_input.is_some() && matches!(expression.kind, AstExprKind::Infix { .. }) {
        return Some(expr_id);
    }
    let input = match &expression.kind {
        AstExprKind::Pipe { input, .. }
        | AstExprKind::Then { input, .. }
        | AstExprKind::When { input, .. }
        | AstExprKind::Draining { input }
        | AstExprKind::Hold { initial: input, .. } => *input,
        AstExprKind::Infix { left, .. } => *left,
        AstExprKind::MatchArm {
            output: Some(_), ..
        }
        | AstExprKind::Arrow {
            output: Some(_), ..
        } => return None,
        _ => return None,
    };
    if expressions
        .get(input)
        .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
    {
        Some(expr_id)
    } else {
        pipeline_placeholder_target(input, expressions)
    }
}

fn statement_child_pipeline_input(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    let owner = statement_structure_owner(statement.expr?, expressions);
    match &expressions.get(owner)?.kind {
        AstExprKind::MatchArm { output, .. }
        | AstExprKind::Arrow { output, .. }
        | AstExprKind::Then { output, .. } => *output,
        AstExprKind::Block { .. }
        | AstExprKind::Object(_)
        | AstExprKind::ListLiteral { .. }
        | AstExprKind::BytesLiteral { .. }
        | AstExprKind::MapLiteral { .. }
        | AstExprKind::SetLiteral { .. }
        | AstExprKind::Flush { .. }
        | AstExprKind::Hold { .. }
        | AstExprKind::Latest { .. }
        | AstExprKind::When { .. } => None,
        AstExprKind::Pipe { op, .. } if op == "WHILE" => None,
        _ => Some(owner),
    }
}

fn statement_structure_pipeline_base(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<usize> {
    let owner = statement_structure_owner(statement.expr?, expressions);
    expressions.get(owner).and_then(|expression| {
        matches!(
            &expression.kind,
            AstExprKind::Block { .. }
                | AstExprKind::Object(_)
                | AstExprKind::ListLiteral { .. }
                | AstExprKind::BytesLiteral { .. }
                | AstExprKind::MapLiteral { .. }
                | AstExprKind::SetLiteral { .. }
                | AstExprKind::Flush { .. }
                | AstExprKind::Hold { .. }
                | AstExprKind::Latest { .. }
                | AstExprKind::When { .. }
        )
        .then_some(owner)
        .or_else(|| {
            matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "WHILE")
                .then_some(owner)
        })
    })
}

fn relink_direct_structure_pipeline(
    statements: &[AstStatement],
    expressions: &mut [AstExpr],
    base: usize,
) {
    let structural_predecessor_indents = statements
        .iter()
        .filter(|statement| {
            statement_pipeline_continuation_target(statement, expressions).is_none()
        })
        .filter_map(|statement| {
            statement_value_expression(statement, expressions)
                .map(|expression| (expression, statement.indent))
        })
        .collect::<BTreeMap<_, _>>();
    let mut previous = base;
    for statement in statements {
        let Some(target) = statement_pipeline_continuation_target(statement, expressions) else {
            continue;
        };
        // Recursive lexical linking runs before the enclosing structure is
        // materialized. For an inline multiline LIST/record, that provisional
        // predecessor can therefore be the last structural child rather than
        // the structure itself. Replace exactly those child edges while
        // preserving a genuinely local BLOCK/HOLD body predecessor.
        if let Some(expression) = expressions.get_mut(target).filter(|expression| {
            expression.linked_input.is_none()
                || expression
                    .linked_input
                    .and_then(|input| structural_predecessor_indents.get(&input))
                    .is_some_and(|predecessor_indent| *predecessor_indent > statement.indent)
        }) {
            expression.linked_input = Some(previous);
            if let AstExprKind::Infix { left, .. } = &mut expression.kind {
                *left = previous;
            }
        }
        if let Some(value) = statement_value_expression(statement, expressions) {
            previous = value;
        }
    }
}

fn leading_pipeline_continuation_result(
    statements: &[AstStatement],
    expressions: &[AstExpr],
) -> Option<usize> {
    let mut result = None;
    for statement in statements {
        if statement_pipeline_continuation_target(statement, expressions).is_none() {
            break;
        }
        result = statement_value_expression(statement, expressions);
    }
    result
}

fn replace_statement_inline_output(
    statement: &AstStatement,
    expressions: &mut [AstExpr],
    output: usize,
) {
    let Some(expr_id) = statement.expr else {
        return;
    };
    let owner = statement_structure_owner(expr_id, expressions);
    let Some(expression) = expressions.get_mut(owner) else {
        return;
    };
    match &mut expression.kind {
        AstExprKind::MatchArm {
            output: arm_output, ..
        }
        | AstExprKind::Arrow {
            output: arm_output, ..
        }
        | AstExprKind::Then {
            output: arm_output, ..
        } if arm_output.is_some() => *arm_output = Some(output),
        _ => {}
    }
}

fn validate_pipeline_inputs(
    path: &str,
    source: &str,
    expressions: &[AstExpr],
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for expression in expressions {
        work_batch.expression();
        work_batch.validation();
        let input = match &expression.kind {
            AstExprKind::Pipe { input, .. }
            | AstExprKind::Then { input, .. }
            | AstExprKind::When { input, .. }
            | AstExprKind::Draining { input }
            | AstExprKind::Hold { initial: input, .. }
            | AstExprKind::Infix { left: input, .. } => *input,
            _ => continue,
        };
        if expressions
            .get(input)
            .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
            && expression.linked_input.is_none()
        {
            let line_start = source
                .get(..expression.start)
                .and_then(|prefix| prefix.rfind('\n'))
                .map_or(0, |newline| newline + 1);
            let column = source
                .get(line_start..expression.start)
                .map_or(1, |prefix| prefix.chars().count() + 1);
            return Err(error(
                path,
                expression.line,
                column,
                "pipeline continuation has no preceding value in its lexical sequence",
            ));
        }
    }
    Ok(())
}

fn materialize_statement_structure(statement: &mut AstStatement, expressions: &mut [AstExpr]) {
    let child_values = statement_sequence_values(&statement.children, expressions);
    let child_result = child_values.last().copied();
    let structural_child_values = statement
        .children
        .iter()
        .filter(|child| !statement_is_pipeline_continuation(child, expressions))
        .filter_map(|child| statement_value_expression(child, expressions))
        .collect::<Vec<_>>();
    let structural_child_result = structural_child_values.last().copied();

    let Some(expr_id) = statement.expr else {
        return;
    };
    let expr_id = statement_structure_owner(expr_id, expressions);
    let selector_parent = expressions.get(expr_id).is_some_and(|expression| {
        matches!(expression.kind, AstExprKind::When { .. })
            || matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "WHILE")
    });
    let map_parent = expressions
        .get(expr_id)
        .is_some_and(|expression| matches!(expression.kind, AstExprKind::MapLiteral { .. }));
    if selector_parent {
        for child in &structural_child_values {
            consume_arrow_as_match_arm(*child, expressions);
        }
    } else if map_parent {
        for child in &structural_child_values {
            consume_arrow_as_map_entry(*child, expressions);
        }
    }
    let child_arms = structural_child_values
        .iter()
        .copied()
        .filter(|expr_id| {
            expressions
                .get(*expr_id)
                .is_some_and(|expr| matches!(expr.kind, AstExprKind::MatchArm { .. }))
        })
        .collect::<Vec<_>>();
    let map_entries = structural_child_values
        .iter()
        .copied()
        .filter(|expr_id| {
            expressions
                .get(*expr_id)
                .is_some_and(|expr| matches!(expr.kind, AstExprKind::MapEntry { .. }))
        })
        .collect::<Vec<_>>();
    let block_bindings = statement
        .children
        .iter()
        .filter_map(|child| {
            let name = statement_binding_name(child)?.to_owned();
            Some(AstBlockBinding {
                name,
                declaration: AstBlockBindingDeclaration::Local {
                    statement: child.id,
                },
                value: statement_value_expression(child, expressions)?,
                start: child.start,
                end: child.end,
            })
        })
        .collect::<Vec<_>>();
    let record_fields = statement
        .children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| {
            let (name, spread) = match &child.kind {
                AstStatementKind::Spread => (format!("__spread_{index}"), true),
                _ => (statement_binding_name(child)?.to_owned(), false),
            };
            Some(AstRecordField {
                name,
                value: statement_value_expression(child, expressions)?,
                start: child.start,
                end: child.end,
                spread,
            })
        })
        .collect::<Vec<_>>();

    let Some(expression) = expressions.get_mut(expr_id) else {
        return;
    };
    match &mut expression.kind {
        AstExprKind::When { arms, .. } => {
            if arms.is_empty() {
                *arms = child_arms;
            }
        }
        AstExprKind::Latest { branches } => {
            if branches.is_empty() {
                *branches = structural_child_values;
            }
        }
        AstExprKind::Pipe { op, arms, .. } if op == "WHILE" => {
            if arms.is_empty() {
                *arms = child_arms;
            }
        }
        AstExprKind::Then { output, .. }
        | AstExprKind::MatchArm { output, .. }
        | AstExprKind::Arrow { output, .. } => {
            if output.is_none() {
                *output = child_result;
            }
        }
        AstExprKind::Flush { payload } => {
            if payload.is_none() {
                *payload = structural_child_result;
            }
        }
        AstExprKind::Block { bindings, result } => {
            if bindings.is_empty() {
                *bindings = block_bindings;
            }
            if result.is_none() {
                *result = child_result;
            }
        }
        AstExprKind::Object(fields) => {
            if fields.is_empty() {
                *fields = record_fields;
            }
        }
        AstExprKind::ListLiteral { items, .. } if items.is_empty() => {
            *items = structural_child_values;
        }
        AstExprKind::SetLiteral { items } if items.is_empty() => {
            *items = structural_child_values;
        }
        AstExprKind::MapLiteral { entries } if entries.is_empty() => {
            *entries = map_entries;
        }
        _ => {}
    }
}

fn consume_arrow_as_match_arm(expression: usize, expressions: &mut [AstExpr]) {
    let Some(AstExprKind::Arrow {
        left,
        pattern,
        output,
    }) = expressions
        .get(expression)
        .map(|expression| expression.kind.clone())
    else {
        return;
    };
    discard_pattern_expression_tree(left, expressions);
    if let Some(expression) = expressions.get_mut(expression) {
        expression.kind = AstExprKind::MatchArm { pattern, output };
    }
}

fn discard_pattern_expression_tree(root: usize, expressions: &mut [AstExpr]) {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let children = expressions
            .get(expression)
            .map(|expression| match &expression.kind {
                AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
                    fields.iter().map(|field| field.value).collect()
                }
                AstExprKind::ListLiteral { items, .. }
                | AstExprKind::BytesLiteral { items, .. }
                | AstExprKind::SetLiteral { items } => items.clone(),
                AstExprKind::MapLiteral { entries } => entries.clone(),
                AstExprKind::MapEntry { key, value } => vec![*key, *value],
                _ => Vec::new(),
            })
            .unwrap_or_default();
        pending.extend(children);
        if let Some(expression) = expressions.get_mut(expression) {
            expression.kind = AstExprKind::Delimiter;
        }
    }
}

fn consume_arrow_as_map_entry(expression: usize, expressions: &mut [AstExpr]) {
    let Some(AstExprKind::Arrow {
        left,
        output: Some(value),
        ..
    }) = expressions
        .get(expression)
        .map(|expression| expression.kind.clone())
    else {
        return;
    };
    if let Some(expression) = expressions.get_mut(expression) {
        expression.kind = AstExprKind::MapEntry { key: left, value };
    }
}

fn statement_structure_owner(expr_id: usize, expressions: &[AstExpr]) -> usize {
    fn structural_children(expression: &AstExpr) -> Vec<usize> {
        match &expression.kind {
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            }
            | AstExprKind::Arrow {
                output: Some(output),
                ..
            } => vec![*output],
            AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => {
                fields.iter().map(|field| field.value).collect()
            }
            AstExprKind::ListLiteral { items, .. }
            | AstExprKind::BytesLiteral { items, .. }
            | AstExprKind::SetLiteral { items } => items.clone(),
            AstExprKind::MapLiteral { entries } => entries.clone(),
            AstExprKind::MapEntry { key, value } => vec![*key, *value],
            AstExprKind::Call { args, .. } => args.iter().map(|argument| argument.value).collect(),
            AstExprKind::Pipe { input, args, .. } => std::iter::once(*input)
                .chain(args.iter().map(|argument| argument.value))
                .collect(),
            _ => Vec::new(),
        }
    }

    let Some(root) = expressions.get(expr_id) else {
        return expr_id;
    };
    let root_end = root.end;
    let mut current = expr_id;
    let mut visited = BTreeSet::new();
    while visited.insert(current) {
        let Some(expression) = expressions.get(current) else {
            break;
        };
        let Some(child) = structural_children(expression)
            .into_iter()
            .filter(|child| {
                expressions.get(*child).is_some_and(|candidate| {
                    candidate.end == root_end
                        && expression_owns_statement_children(*child, expressions)
                })
            })
            .max_by_key(|child| {
                expressions
                    .get(*child)
                    .map_or((0, 0), |candidate| (candidate.start, candidate.id))
            })
        else {
            break;
        };
        current = child;
    }
    current
}

fn expression_owns_statement_children(expr_id: usize, expressions: &[AstExpr]) -> bool {
    expressions.get(expr_id).is_some_and(|expression| {
        matches!(
            &expression.kind,
            AstExprKind::Block { .. }
                | AstExprKind::Object(_)
                | AstExprKind::ListLiteral { .. }
                | AstExprKind::BytesLiteral { .. }
                | AstExprKind::MapLiteral { .. }
                | AstExprKind::SetLiteral { .. }
                | AstExprKind::Flush { .. }
                | AstExprKind::Hold { .. }
                | AstExprKind::Latest { .. }
                | AstExprKind::When { .. }
                | AstExprKind::Then { .. }
        ) || matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "WHILE")
    })
}

fn statement_sequence_values(statements: &[AstStatement], expressions: &[AstExpr]) -> Vec<usize> {
    let mut values = Vec::new();
    for statement in statements {
        let Some(value) = statement_value_expression(statement, expressions) else {
            continue;
        };
        if statement_is_pipeline_continuation(statement, expressions) && !values.is_empty() {
            *values.last_mut().expect("non-empty values") = value;
        } else {
            values.push(value);
        }
    }
    values
}

fn statement_value_expression(statement: &AstStatement, expressions: &[AstExpr]) -> Option<usize> {
    // A function declaration owns an executable body but contributes no value
    // at its declaration site. Never let its body tail leak into an enclosing
    // BLOCK/LIST or statement sequence as an implicit structural result.
    if matches!(statement.kind, AstStatementKind::Function { .. }) {
        return None;
    }
    if statement
        .expr
        .and_then(|expr_id| expressions.get(expr_id))
        .is_some_and(|expr| {
            matches!(
                &expr.kind,
                AstExprKind::Block { .. }
                    | AstExprKind::Object(_)
                    | AstExprKind::ListLiteral { .. }
                    | AstExprKind::BytesLiteral { .. }
                    | AstExprKind::MapLiteral { .. }
                    | AstExprKind::SetLiteral { .. }
                    | AstExprKind::Flush { .. }
                    | AstExprKind::Hold { .. }
                    | AstExprKind::Latest { .. }
                    | AstExprKind::When { .. }
                    | AstExprKind::Then { .. }
                    | AstExprKind::MatchArm { .. }
                    | AstExprKind::Arrow { .. }
            ) || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "WHILE")
        })
    {
        return statement.expr;
    }
    statement_sequence_values(&statement.children, expressions)
        .last()
        .copied()
        .or(statement.expr)
}

fn statement_binding_name(statement: &AstStatement) -> Option<&str> {
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::Hold {
            field: Some(name), ..
        }
        | AstStatementKind::List {
            field: Some(name), ..
        } => Some(name),
        AstStatementKind::Function { .. }
        | AstStatementKind::Source { field: None, .. }
        | AstStatementKind::Hold { field: None, .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block
        | AstStatementKind::Spread
        | AstStatementKind::Expression => None,
    }
}

/// Tokenizes Boon source with the same lexer used by the parser.
///
/// Editor tooling uses this surface so highlighting and semantic inspection do
/// not need a second, drifting approximation of the language grammar.
pub fn lex_source(path: &str, source: &str) -> Result<Vec<AstToken>, ParseError> {
    lex_source_with_work(path, source, &ParseWorkRecorder::disabled())
}

fn lex_source_with_work(
    path: &str,
    source: &str,
    work: &ParseWorkRecorder,
) -> Result<Vec<AstToken>, ParseError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::<AstToken>::with_capacity(source.len() / 6);
    let mut cursor = 0usize;
    let mut line = 1usize;
    let mut column = 1usize;

    while cursor < bytes.len() {
        let start = cursor;
        let token_line = line;
        let token_column = column;
        while bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        {
            cursor += 1;
        }
        if cursor == bytes.len() {
            work.source_bytes(cursor.saturating_sub(start));
            if tokens.is_empty() {
                return Err(ParseError {
                    path: path.to_owned(),
                    line: None,
                    column: None,
                    message: format!("syntax error near {cursor}..{cursor}"),
                });
            }
            // Horizontal padding belongs to the preceding token, just as it
            // did in the grammar-combinator lexer.
            let token = tokens.last_mut().expect("non-empty token stream");
            token.end = cursor;
            if matches!(
                token.kind,
                AstTokenKind::String | AstTokenKind::Comment | AstTokenKind::Newline
            ) {
                token.lexeme = source[token.start..cursor].to_owned();
            }
            advance_lexer_position(&source[start..cursor], &mut line, &mut column, work);
            break;
        }

        let semantic_start = cursor;
        let kind = match bytes[cursor] {
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'/')
                }) {
                    cursor += 1;
                }
                AstTokenKind::Identifier
            }
            byte if byte.is_ascii_digit() => {
                cursor += 1;
                while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                    cursor += 1;
                }
                AstTokenKind::Number
            }
            b'"' => {
                let mut scan = cursor + 1;
                let mut close = None;
                while scan < bytes.len() {
                    match bytes[scan] {
                        b'\\' => {
                            scan += 1;
                            if scan < bytes.len() {
                                let character = source[scan..]
                                    .chars()
                                    .next()
                                    .expect("valid UTF-8 character boundary");
                                scan += character.len_utf8();
                            }
                        }
                        b'"' => {
                            close = Some(scan + 1);
                            break;
                        }
                        _ => {
                            let character = source[scan..]
                                .chars()
                                .next()
                                .expect("valid UTF-8 character boundary");
                            scan += character.len_utf8();
                        }
                    }
                }
                if let Some(close) = close {
                    cursor = close;
                    AstTokenKind::String
                } else {
                    cursor += 1;
                    AstTokenKind::Unknown
                }
            }
            b'-' if bytes.get(cursor + 1) == Some(&b'-') => {
                cursor += 2;
                while bytes.get(cursor).is_some_and(|byte| *byte != b'\n') {
                    cursor += 1;
                }
                AstTokenKind::Comment
            }
            first
                if matches!(
                    (first, bytes.get(cursor + 1).copied()),
                    (b'=', Some(b'>'))
                        | (b'|', Some(b'>'))
                        | (b'=', Some(b'='))
                        | (b'>', Some(b'='))
                        | (b'<', Some(b'='))
                        | (b'!', Some(b'='))
                ) =>
            {
                cursor += 2;
                AstTokenKind::Operator
            }
            byte if matches!(
                byte,
                b'>' | b'<' | b'=' | b'|' | b'+' | b'-' | b'%' | b'*' | b'/'
            ) =>
            {
                cursor += 1;
                AstTokenKind::Operator
            }
            byte if matches!(
                byte,
                b'[' | b']' | b'{' | b'}' | b'(' | b')' | b':' | b',' | b'.' | b'$' | b'#'
            ) =>
            {
                cursor += 1;
                AstTokenKind::Symbol
            }
            b'\n' => {
                cursor += 1;
                AstTokenKind::Newline
            }
            _ => {
                let character = source[cursor..]
                    .chars()
                    .next()
                    .expect("valid UTF-8 character boundary");
                cursor += character.len_utf8();
                AstTokenKind::Unknown
            }
        };
        while bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        {
            cursor += 1;
        }

        let raw_lexeme = &source[start..cursor];
        work.source_bytes(cursor.saturating_sub(start));
        let lexeme = match kind {
            AstTokenKind::String | AstTokenKind::Comment | AstTokenKind::Newline => raw_lexeme,
            _ => source[semantic_start..cursor]
                .trim_matches(|character| matches!(character, ' ' | '\t' | '\r')),
        };
        tokens.push(AstToken {
            kind,
            lexeme: lexeme.to_owned(),
            line: token_line,
            column: token_column,
            start,
            end: cursor,
        });
        advance_lexer_position(&source[start..cursor], &mut line, &mut column, work);
    }

    Ok(tokens)
}

fn advance_lexer_position(
    source: &str,
    line: &mut usize,
    column: &mut usize,
    work: &ParseWorkRecorder,
) {
    for character in source.chars() {
        work.source_bytes(character.len_utf8());
        if character == '\n' {
            *line += 1;
            *column = 1;
        } else {
            *column += 1;
        }
    }
}

fn document_statement(ast: &AstProgram) -> Option<&AstStatement> {
    ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "document"
        )
    })
}

fn scene_statement(ast: &AstProgram) -> Option<&AstStatement> {
    ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "scene"
        )
    })
}

fn parser_lines(tokens: &[AstToken], work: &ParseWorkRecorder) -> Vec<ParserLine> {
    let mut lines = Vec::new();
    let mut current_line = None;
    let mut indent = 0usize;
    let mut start = 0usize;
    let mut end = 0usize;
    let mut symbols = Vec::new();
    let mut symbol_spans = Vec::new();
    for token in tokens {
        work.tokens(1);
        if current_line != Some(token.line) {
            if let Some(line) = current_line {
                lines.push(ParserLine {
                    line,
                    indent,
                    symbols: std::mem::take(&mut symbols).into(),
                    symbol_spans: std::mem::take(&mut symbol_spans),
                    start,
                    end,
                });
            }
            current_line = Some(token.line);
            indent = token.column.saturating_sub(1);
            start = token.start;
        }
        end = token.end;
        if !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::Newline)
            && !token.lexeme.is_empty()
        {
            symbols.push(token.lexeme.clone());
            symbol_spans.push((token.start, token.end));
        }
    }
    if let Some(line) = current_line {
        lines.push(ParserLine {
            line,
            indent,
            symbols: symbols.into(),
            symbol_spans,
            start,
            end,
        });
    }
    lines
}

fn merge_multiline_bytes_lines(
    lines: Vec<ParserLine>,
    text_body_line_ranges: &[(usize, usize)],
    work: &ParseWorkRecorder,
) -> Vec<ParserLine> {
    merge_multiline_braced_lines(lines, text_body_line_ranges, unclosed_bytes_body_open, work)
}

fn merge_multiline_drain_lines(
    lines: Vec<ParserLine>,
    text_body_line_ranges: &[(usize, usize)],
    work: &ParseWorkRecorder,
) -> Vec<ParserLine> {
    merge_multiline_braced_lines(lines, text_body_line_ranges, unclosed_drain_body_open, work)
}

fn merge_multiline_call_expression_lines(
    lines: Vec<ParserLine>,
    text_body_line_ranges: &[(usize, usize)],
    work: &ParseWorkRecorder,
) -> Vec<ParserLine> {
    let mut merged = Vec::with_capacity(lines.len());
    let mut remaining = lines.into_iter().peekable();
    let mut replay = Vec::new();
    let mut delimiter_stack = Vec::new();
    while let Some(line) = replay.pop().or_else(|| remaining.next()) {
        work.symbols(line.symbols.len());
        if line_is_in_ranges(line.line, text_body_line_ranges) {
            merged.push(line);
            continue;
        }
        let Some(open) = line.symbols.iter().position(|symbol| symbol == "(") else {
            merged.push(line);
            continue;
        };
        delimiter_stack.clear();
        delimiter_stack.push(")");
        let (progress, inspected) =
            advance_delimiter_stack(&line.symbols, open + 1, &mut delimiter_stack);
        work.symbols(inspected);
        match progress {
            DelimiterProgress::Closed | DelimiterProgress::Mismatched => {
                merged.push(line);
                continue;
            }
            DelimiterProgress::Open => {}
        }

        let mut buffered = Vec::new();
        let mut outer_closed = false;
        while !outer_closed {
            let Some(next) = replay.last().or_else(|| remaining.peek()) else {
                break;
            };
            if line_is_in_ranges(next.line, text_body_line_ranges) {
                break;
            }
            let next = replay
                .pop()
                .or_else(|| remaining.next())
                .expect("peeked multiline expression line");
            let (progress, inspected) =
                advance_delimiter_stack(&next.symbols, 0, &mut delimiter_stack);
            work.symbols(inspected);
            match progress {
                DelimiterProgress::Open => {}
                DelimiterProgress::Closed => outer_closed = true,
                DelimiterProgress::Mismatched => {
                    buffered.push(next);
                    break;
                }
            }
            buffered.push(next);
        }
        if !outer_closed {
            replay.extend(buffered.into_iter().rev());
            merged.push(line);
            continue;
        }
        let mut expression_line = line;
        for next in buffered {
            if multiline_expression_needs_separator(&expression_line.symbols, &next.symbols) {
                let separator = expression_line
                    .symbol_spans
                    .last()
                    .map(|(_, end)| (*end, *end))
                    .unwrap_or((expression_line.end, expression_line.end));
                expression_line.symbols.push(",".to_owned());
                expression_line.symbol_spans.push(separator);
            }
            expression_line.end = next.end;
            expression_line.symbols.extend(next.symbols);
            expression_line.symbol_spans.extend(next.symbol_spans);
        }
        merged.push(expression_line);
    }
    merged
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DelimiterProgress {
    Open,
    Closed,
    Mismatched,
}

/// Advance one multiline delimiter stack across a symbol slice.
///
/// The returned count is the exact number of symbols inspected, allowing
/// tests to prove that a sequence of physical lines is scanned once rather
/// than repeatedly rescanning a growing prefix.
fn advance_delimiter_stack(
    symbols: &[String],
    start: usize,
    stack: &mut Vec<&'static str>,
) -> (DelimiterProgress, usize) {
    for (offset, symbol) in symbols.iter().skip(start).enumerate() {
        match symbol.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop() != Some(symbol.as_str()) {
                    return (DelimiterProgress::Mismatched, offset + 1);
                }
                if stack.is_empty() {
                    return (DelimiterProgress::Closed, offset + 1);
                }
            }
            _ => {}
        }
    }
    (DelimiterProgress::Open, symbols.len().saturating_sub(start))
}

fn multiline_expression_needs_separator(current: &[String], next: &[String]) -> bool {
    let Some(previous) = current.last().map(String::as_str) else {
        return false;
    };
    let Some(next) = next.first().map(String::as_str) else {
        return false;
    };
    !matches!(previous, "(" | "[" | "{" | ":" | "," | "|>" | "=>")
        && !matches!(next, ")" | "]" | "}" | "," | "|>" | "=>")
}

fn merge_multiline_braced_lines(
    lines: Vec<ParserLine>,
    text_body_line_ranges: &[(usize, usize)],
    unclosed_body_open: fn(&[String]) -> Option<usize>,
    work: &ParseWorkRecorder,
) -> Vec<ParserLine> {
    let mut merged = Vec::with_capacity(lines.len());
    let mut remaining = lines.into_iter().peekable();
    while let Some(mut current) = remaining.next() {
        work.symbols(current.symbols.len());
        if line_is_in_ranges(current.line, text_body_line_ranges) {
            merged.push(current);
            continue;
        }
        let Some(body_open) = unclosed_body_open(&current.symbols) else {
            merged.push(current);
            continue;
        };
        loop {
            work.symbols(current.symbols.len().saturating_sub(body_open));
            if matching_close(&current.symbols, body_open).is_some() {
                break;
            }
            let Some(next) = remaining.peek() else {
                break;
            };
            if line_is_in_ranges(next.line, text_body_line_ranges) {
                break;
            }
            let next = remaining.next().expect("peeked multiline braced line");
            current.end = next.end;
            current.symbols.extend(next.symbols);
            current.symbol_spans.extend(next.symbol_spans);
        }
        merged.push(current);
    }
    merged
}

fn line_is_in_ranges(line: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line >= *start && line <= *end)
}

fn unclosed_bytes_body_open(symbols: &[String]) -> Option<usize> {
    let bytes = symbols.iter().position(|symbol| symbol == "BYTES")?;
    let body_open = match symbols.get(bytes + 1).map(String::as_str) {
        Some("{") => bytes + 1,
        Some("[") => {
            let size_close = matching_close(symbols, bytes + 1)?;
            (symbols.get(size_close + 1).map(String::as_str) == Some("{"))
                .then_some(size_close + 1)?
        }
        _ => return None,
    };
    matching_close(symbols, body_open)
        .is_none()
        .then_some(body_open)
}

fn unclosed_drain_body_open(symbols: &[String]) -> Option<usize> {
    let drain = symbols.iter().position(|symbol| symbol == "DRAIN")?;
    let body_open =
        (symbols.get(drain + 1).map(String::as_str) == Some("{")).then_some(drain + 1)?;
    matching_close(symbols, body_open)
        .is_none()
        .then_some(body_open)
}

fn parser_items(
    lines: Vec<ParserLine>,
    text_body_line_ranges: &[(usize, usize)],
    work: &ParseWorkRecorder,
) -> Vec<ParserItem> {
    lines
        .into_iter()
        .filter(|line| {
            work.symbols(line.symbols.len());
            !text_body_line_ranges
                .iter()
                .any(|(start, end)| line.line >= *start && line.line <= *end)
        })
        .filter(|line| !line.symbols.is_empty())
        .map(|line| parser_item(line, work))
        .collect()
}

fn parser_item(line: ParserLine, work: &ParseWorkRecorder) -> ParserItem {
    let ParserLine {
        line,
        indent,
        symbols,
        symbol_spans,
        start,
        end,
    } = line;
    work.symbols(symbols.len());
    let field = ast_field_name(&symbols).map(ToOwned::to_owned);
    let function = (symbols.first().map(String::as_str) == Some("FUNCTION"))
        .then(|| symbols.get(1).cloned())
        .flatten();
    let source_event = ast_insource_slice_event(&symbols).map(ToOwned::to_owned);
    let hold = ast_hold_name(&symbols).map(ToOwned::to_owned);
    let list_capacity = ast_list_capacity(&symbols);
    let opens_scope = ast_opens_scope(&symbols);
    let closes_scope =
        symbols.len() == 1 && matches!(symbols.first().map(String::as_str), Some("}" | "]" | ")"));
    let operators = ast_expression_operators(&symbols);
    let is_list = symbols.iter().any(|lexeme| is_list_constructor(lexeme))
        && find_top_level_pipe(&symbols).is_none();
    ParserItem {
        line,
        indent,
        start,
        end,
        source_event,
        hold,
        list_capacity,
        opens_scope,
        closes_scope,
        operators,
        symbols,
        symbol_spans,
        field,
        example: None,
        function,
        is_list,
    }
}

fn is_list_constructor(lexeme: &str) -> bool {
    matches!(lexeme, "LIST" | "List/range")
}

fn ast_statement_tree(
    items: &[ParserItem],
    expressions: &mut Vec<AstExpr>,
    source: &str,
    work: &ParseWorkRecorder,
) -> Vec<AstStatement> {
    let mut index = 0usize;
    let mut next_id = 0usize;
    ast_statement_block(
        items,
        &mut index,
        0,
        expressions,
        &mut next_id,
        source,
        work,
    )
}

fn ast_statement_block(
    items: &[ParserItem],
    index: &mut usize,
    min_indent: usize,
    expressions: &mut Vec<AstExpr>,
    next_id: &mut usize,
    source: &str,
    work: &ParseWorkRecorder,
) -> Vec<AstStatement> {
    let mut statements = Vec::new();
    while let Some(item) = items.get(*index) {
        if item.indent < min_indent {
            break;
        }
        if item.closes_scope {
            *index += 1;
            continue;
        }
        let indent = item.indent;
        let mut statement = ast_statement(item, expressions, *next_id, source, work);
        *next_id += 1;
        *index += 1;
        if item.opens_scope || items.get(*index).is_some_and(|next| next.indent > indent) {
            statement.children =
                ast_statement_block(items, index, indent + 1, expressions, next_id, source, work);
        }
        statements.push(statement);
    }
    statements
}

fn ast_statement(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    id: usize,
    source: &str,
    work: &ParseWorkRecorder,
) -> AstStatement {
    work.statement();
    let is_semantic_block = item.symbols.first().map(String::as_str) == Some("BLOCK")
        && item.symbols.last().map(String::as_str) == Some("{");
    let kind = if let Some(function) = item.function.clone() {
        AstStatementKind::Function {
            name: function,
            parameters: ast_function_parameters(&item.symbols, item),
        }
    } else if item.has_lexeme("SOURCE") {
        AstStatementKind::Source {
            field: item.field.clone(),
            event: item.source_event.clone(),
        }
    } else if item.has_lexeme("HOLD") {
        AstStatementKind::Hold {
            field: item.field.clone(),
            name: item.hold.clone(),
        }
    } else if item.is_list {
        AstStatementKind::List {
            field: item.field.clone(),
            capacity: item.list_capacity,
        }
    } else if let Some(field) = item.field.clone() {
        AstStatementKind::Field { name: field }
    } else if is_semantic_block
        || matches!(item.symbols.as_slice(), [one] if matches!(one.as_str(), "[" | "{" | "(" | "]" | "}" | ")"))
    {
        AstStatementKind::Block
    } else if item
        .symbols
        .starts_with(&[".".to_owned(), ".".to_owned(), ".".to_owned()])
    {
        AstStatementKind::Spread
    } else {
        AstStatementKind::Expression
    };
    let expr = if matches!(kind, AstStatementKind::Function { .. }) {
        None
    } else if matches!(kind, AstStatementKind::Block) && !is_semantic_block {
        (item.symbols.first().map(String::as_str) == Some("[")).then(|| {
            push_ast_expr(
                item,
                expressions,
                AstExprKind::Object(Vec::new()),
                item.start,
                item.end,
            )
        })
    } else if item.field.is_some()
        && item.symbols.get(1).map(String::as_str) == Some(":")
        && item.symbols.get(2).map(String::as_str) == Some("[")
        && item.symbols.len() == 3
    {
        Some(push_ast_expr(
            item,
            expressions,
            AstExprKind::Object(Vec::new()),
            item.start,
            item.end,
        ))
    } else {
        let expr_tokens = statement_expression_tokens(item);
        (!expr_tokens.is_empty()).then(|| parse_ast_expr(expr_tokens, item, expressions, source))
    };
    AstStatement {
        id,
        line: item.line,
        indent: item.indent,
        start: item.start,
        end: item.end,
        kind,
        expr,
        children: Vec::new(),
    }
}

fn statement_expression_tokens(item: &ParserItem) -> &[String] {
    if item.field.is_some() && item.symbols.get(1).map(String::as_str) == Some(":") {
        if matches!(
            item.symbols.get(2).map(String::as_str),
            Some("[") | Some("{")
        ) && item.symbols.len() == 3
        {
            return &[];
        }
        return &item.symbols[2..];
    }
    &item.symbols
}

fn parse_ast_expr(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> usize {
    let kind = ast_expr_kind(tokens, item, expressions, source);
    let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
    push_ast_expr(item, expressions, kind, start, end)
}

fn push_ast_expr(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    kind: AstExprKind,
    start: usize,
    end: usize,
) -> usize {
    let id = expressions.len();
    expressions.push(AstExpr {
        id,
        line: item.line,
        start,
        end,
        linked_input: None,
        kind,
    });
    id
}

fn span_for_tokens(tokens: &[String], item: &ParserItem) -> Option<(usize, usize)> {
    if tokens.is_empty() {
        return None;
    }
    item.symbols
        .windows(tokens.len())
        .position(|window| window == tokens)
        .and_then(|start_index| {
            let end_index = start_index + tokens.len() - 1;
            Some((
                item.symbol_spans.get(start_index)?.0,
                item.symbol_spans.get(end_index)?.1,
            ))
        })
}

fn ast_expr_kind(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> AstExprKind {
    if tokens.is_empty() {
        return AstExprKind::Delimiter;
    }
    if tokens.len() > 3 && tokens[0] == "." && tokens[1] == "." && tokens[2] == "." {
        return ast_expr_kind(&tokens[3..], item, expressions, source);
    }
    if tokens
        .iter()
        .all(|token| matches!(token.as_str(), "[" | "]" | "{" | "}" | "(" | ")"))
    {
        return AstExprKind::Delimiter;
    }
    if tokens.first().map(String::as_str) == Some("(")
        && matching_close(tokens, 0) == Some(tokens.len() - 1)
    {
        return ast_expr_kind(&tokens[1..tokens.len() - 1], item, expressions, source);
    }
    if tokens == ["SOURCE"] {
        return AstExprKind::Source;
    }
    if let Some(number) = ast_number_literal(tokens) {
        return AstExprKind::Number(number);
    }
    if let Some((radix, digits, value)) = ast_byte_literal(tokens, item, source) {
        return AstExprKind::ByteLiteral {
            radix,
            digits,
            value,
        };
    }
    if let Some((width, radix, digits)) = ast_bits_literal(tokens, item, source) {
        return AstExprKind::BitsLiteral {
            width,
            radix,
            digits,
        };
    }
    if let Some(arrow) = find_top_level_token(tokens, "=>") {
        let pattern = &tokens[..arrow];
        return AstExprKind::Arrow {
            left: parse_ast_expr(pattern, item, expressions, source),
            pattern: ast_match_pattern(pattern, item, source),
            output: (!tokens[arrow + 1..].is_empty())
                .then(|| parse_ast_expr(&tokens[arrow + 1..], item, expressions, source)),
        };
    }
    if let Some(value) = string_literal_value(tokens) {
        return AstExprKind::StringLiteral(value);
    }
    if let Some(text) = parsed_text_literal(tokens, item, source) {
        if let Some(segments) =
            ast_text_template_segments(&text.value, text.value_start, item, expressions, source)
        {
            return AstExprKind::TextTemplate { segments };
        }
        return AstExprKind::TextLiteral(text.value);
    }
    if tokens == ["Text/empty", "(", ")"] {
        return AstExprKind::TextLiteral(String::new());
    }
    if tokens.first().map(String::as_str) == Some("FLUSH") {
        let payload = if tokens.get(1).map(String::as_str) == Some("{")
            && matching_close(tokens, 1) == Some(tokens.len() - 1)
        {
            (!tokens[2..tokens.len() - 1].is_empty())
                .then(|| parse_ast_expr(&tokens[2..tokens.len() - 1], item, expressions, source))
        } else {
            None
        };
        return AstExprKind::Flush { payload };
    }
    if tokens.first().map(String::as_str) == Some("BLOCK")
        && tokens.last().map(String::as_str) == Some("{")
    {
        return AstExprKind::Block {
            bindings: Vec::new(),
            result: None,
        };
    }
    if let Some(path) = ast_drain_path(tokens) {
        return AstExprKind::Drain { path };
    }
    if let Some(pipe) = find_top_level_pipe(tokens) {
        return ast_pipe_expr_kind(tokens, pipe, item, expressions, source);
    }
    if tokens.first().map(String::as_str) == Some("LATEST") {
        return AstExprKind::Latest {
            branches: ast_latest_branches(tokens, item, expressions, source),
        };
    }
    if tokens.first().map(String::as_str) == Some("LIST") {
        return AstExprKind::ListLiteral {
            capacity: ast_list_capacity(tokens),
            items: ast_list_items(tokens, item, expressions, source),
        };
    }
    if tokens.first().map(String::as_str) == Some("MAP") {
        return AstExprKind::MapLiteral {
            entries: ast_map_entries(tokens, item, expressions, source),
        };
    }
    if tokens.first().map(String::as_str) == Some("SET") {
        return AstExprKind::SetLiteral {
            items: ast_collection_items(tokens, item, expressions, source),
        };
    }
    if let Some((size, items)) = ast_bytes_literal(tokens, item, expressions, source) {
        return AstExprKind::BytesLiteral { size, items };
    }
    if tokens.first().map(String::as_str) == Some("[")
        && tokens.last().map(String::as_str) == Some("]")
    {
        return AstExprKind::Object(ast_record_fields(tokens, item, expressions, source));
    }
    if tokens.first().map(String::as_str) == Some("[")
        && tokens.get(2).map(String::as_str) == Some(":")
        && tokens.len() > 3
    {
        let value = parse_ast_expr(&tokens[3..], item, expressions, source);
        let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
        return AstExprKind::Object(vec![AstRecordField {
            name: tokens[1].clone(),
            value,
            start,
            end,
            spread: false,
        }]);
    }
    if tokens.len() >= 3
        && tokens.get(1).map(String::as_str) == Some("[")
        && tokens.last().map(String::as_str) == Some("]")
        && tokens
            .first()
            .is_some_and(|token| value_starts_uppercase_identifier(token))
    {
        return AstExprKind::TaggedObject {
            tag: tokens[0].clone(),
            fields: ast_record_fields(&tokens[1..], item, expressions, source),
        };
    }
    if let Some((op, right)) = split_leading_infix(tokens) {
        let left = parse_ast_expr(&[], item, expressions, source);
        let right = parse_ast_expr(right, item, expressions, source);
        return AstExprKind::Infix {
            left,
            op: op.to_owned(),
            right,
        };
    }
    if let Some((left, op, right)) = split_infix(tokens) {
        let left = parse_ast_expr(left, item, expressions, source);
        let right = parse_ast_expr(right, item, expressions, source);
        return AstExprKind::Infix {
            left,
            op: op.to_owned(),
            right,
        };
    }
    if let Some((input_tokens, field)) = split_postfix_field_access(tokens) {
        let input = parse_ast_expr(input_tokens, item, expressions, source);
        return AstExprKind::Pipe {
            input,
            op: format!("Field/{field}"),
            args: Vec::new(),
            pass: None,
            arms: Vec::new(),
        };
    }
    if let Some((function, args, pass)) = ast_call(tokens, item, expressions, source) {
        return AstExprKind::Call {
            function,
            args,
            pass,
        };
    }
    if tokens.len() == 1 && split_role_value_head(&tokens[0]).is_some() {
        AstExprKind::Path(path_segments(tokens))
    } else if tokens.len() == 1 && is_name(&tokens[0]) {
        let token = tokens[0].clone();
        if value_starts_uppercase_identifier(&token) {
            AstExprKind::Tag(token)
        } else {
            AstExprKind::Identifier(token)
        }
    } else if tokens.iter().any(|token| token == ".") {
        AstExprKind::Path(path_segments(tokens))
    } else {
        AstExprKind::Unknown(tokens.to_vec())
    }
}

fn ast_match_pattern(tokens: &[String], item: &ParserItem, source: &str) -> AstMatchPattern {
    if tokens == ["__"] {
        return AstMatchPattern::Wildcard;
    }
    if tokens == ["NaN"] {
        return AstMatchPattern::Invalid {
            message: "`NaN` is not a Number or a valid match pattern; handle typed parse and find results instead".to_owned(),
        };
    }
    if let Some(value) = ast_number_literal(tokens) {
        return AstMatchPattern::Number { value };
    }
    if let Some(value) = string_literal_value(tokens) {
        return AstMatchPattern::Text { value };
    }
    if let Some(value) = text_literal_value(tokens, item, source) {
        return AstMatchPattern::Text { value };
    }
    if let Some((width, radix, digits)) = ast_bits_literal(tokens, item, source) {
        return AstMatchPattern::Bits {
            width,
            radix,
            digits,
        };
    }
    if tokens.len() == 1 && matches!(tokens[0].as_str(), "FLUSH" | "FLUSHED" | "SKIP" | "SOURCE") {
        return AstMatchPattern::Invalid {
            message: "private flow-control states cannot be matched as public values".to_owned(),
        };
    }
    if tokens.len() == 1
        && matches!(
            tokens[0].as_str(),
            "BITS" | "BYTES" | "LIST" | "MAP" | "NUMBER" | "SET" | "TEXT"
        )
    {
        return AstMatchPattern::Invalid {
            message: invalid_match_pattern_message(tokens),
        };
    }
    if let Some(name) = tokens.first().filter(|_| tokens.len() == 1) {
        if value_starts_uppercase_identifier(name) {
            return AstMatchPattern::Tag {
                name: name.clone(),
                fields: Vec::new(),
            };
        }
        if is_name(name) {
            return AstMatchPattern::Binding { name: name.clone() };
        }
    }
    if let Some(name) = tokens.first().filter(|name| {
        value_starts_uppercase_identifier(name)
            && !matches!(
                name.as_str(),
                "BITS" | "BYTES" | "LIST" | "MAP" | "NUMBER" | "SET" | "TEXT"
            )
    }) && tokens.get(1).map(String::as_str) == Some("[")
        && matching_close(tokens, 1) == tokens.len().checked_sub(1)
    {
        return match ast_tag_pattern_fields(&tokens[2..tokens.len() - 1]) {
            Ok(fields) => AstMatchPattern::Tag {
                name: name.clone(),
                fields,
            },
            Err(message) => AstMatchPattern::Invalid { message },
        };
    }
    AstMatchPattern::Invalid {
        message: invalid_match_pattern_message(tokens),
    }
}

fn ast_tag_pattern_fields(tokens: &[String]) -> Result<Vec<String>, String> {
    if tokens.is_empty() {
        return Err(
            "tag payload patterns require at least one lowercase payload field binding".to_owned(),
        );
    }
    if tokens
        .iter()
        .any(|token| matches!(token.as_str(), "[" | "]" | "{" | "}" | ":" | "=>"))
    {
        return Err(
            "tag payload patterns do not support renaming, nesting, or comparison; use `Tag[field, ...]`"
                .to_owned(),
        );
    }
    if tokens.len() % 2 == 0 {
        return Err(
            "tag payload patterns must use `Tag[field, ...]` with comma-separated lowercase payload field bindings"
                .to_owned(),
        );
    }
    let mut fields = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if index % 2 == 1 {
            if token != "," {
                return Err(
                    "tag payload patterns do not support renaming, nesting, or comparison; use `Tag[field, ...]`"
                        .to_owned(),
                );
            }
            continue;
        }
        if !is_name(token)
            || !token
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase())
        {
            return Err(format!(
                "tag payload binding `{token}` must be a lowercase field name"
            ));
        }
        if fields.contains(token) {
            return Err(format!(
                "tag payload field `{token}` cannot be bound more than once"
            ));
        }
        fields.push(token.clone());
    }
    Ok(fields)
}

fn invalid_match_pattern_message(tokens: &[String]) -> String {
    let rendered = tokens.join(" ");
    match tokens.first().map(String::as_str) {
        None => "match arms require a pattern before `=>`".to_owned(),
        Some("[") => {
            "object patterns are unsupported; match an ordinary Tag and bind explicit payload fields"
                .to_owned()
        }
        Some("LIST") => {
            "LIST patterns are unsupported; use explicit list operations before matching".to_owned()
        }
        Some("MAP") => {
            "MAP patterns are unsupported; use explicit map operations before matching".to_owned()
        }
        Some("SET") => {
            "SET patterns are unsupported; use explicit set operations before matching".to_owned()
        }
        Some("BITS") => "BITS patterns must be one exact fixed-width BITS literal".to_owned(),
        Some("BYTES" | "NUMBER" | "TEXT") => {
            "runtime type patterns are unsupported; match an exact literal or an ordinary Tag"
                .to_owned()
        }
        Some("{") => {
            "dynamic comparison patterns are unsupported; compare explicitly inside the arm"
                .to_owned()
        }
        Some(name) if value_starts_uppercase_identifier(name) => {
            "tag payload patterns must use `Tag[field, ...]` with lowercase payload field bindings"
                .to_owned()
        }
        _ => format!(
            "unsupported match pattern `{rendered}`; use `__`, a lowercase whole-value binding, an exact literal, a bare Tag, or `Tag[field, ...]`"
        ),
    }
}

fn ast_drain_path(tokens: &[String]) -> Option<AstDrainPath> {
    if tokens.first().map(String::as_str) != Some("DRAIN")
        || tokens.get(1).map(String::as_str) != Some("{")
        || matching_close(tokens, 1) != Some(tokens.len().checked_sub(1)?)
    {
        return None;
    }
    drain_path_from_symbols(&tokens[2..tokens.len() - 1])
}

fn drain_path_from_symbols(symbols: &[String]) -> Option<AstDrainPath> {
    if symbols.is_empty() || symbols.len().is_multiple_of(2) {
        return None;
    }
    let mut segments = Vec::with_capacity(symbols.len().div_ceil(2));
    for (index, symbol) in symbols.iter().enumerate() {
        if index % 2 == 1 {
            if symbol != "." {
                return None;
            }
            continue;
        }
        if !is_drain_path_segment(symbol) {
            return None;
        }
        segments.push(symbol.clone());
    }
    let root = segments.remove(0);
    if root == "PASSED" {
        return (!segments.is_empty()).then_some(AstDrainPath::Passed { fields: segments });
    }
    if value_starts_uppercase_identifier(&root) {
        return None;
    }
    if segments.is_empty() {
        Some(AstDrainPath::Binding { name: root })
    } else {
        Some(AstDrainPath::Field {
            binding: root,
            fields: segments,
        })
    }
}

fn is_drain_path_segment(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && is_name(value)
}

fn ast_number_literal(tokens: &[String]) -> Option<String> {
    match tokens {
        [value] if value.chars().all(|ch| ch.is_ascii_digit()) => Some(value.clone()),
        [left, dot, right]
            if dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Some(format!("{left}.{right}"))
        }
        [left, dot, right @ ..]
            if dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && !right.is_empty()
                && right
                    .iter()
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            Some(format!("{left}.{}", right.join("")))
        }
        [minus, value] if minus == "-" && value.chars().all(|ch| ch.is_ascii_digit()) => {
            Some(format!("-{value}"))
        }
        [minus, left, dot, right]
            if minus == "-"
                && dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Some(format!("-{left}.{right}"))
        }
        [minus, left, dot, right @ ..]
            if minus == "-"
                && dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && !right.is_empty()
                && right
                    .iter()
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            Some(format!("-{left}.{}", right.join("")))
        }
        _ => None,
    }
}

fn ast_byte_literal(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<(u8, String, u8)> {
    let [base, suffix] = tokens else {
        return None;
    };
    if !matches!(base.as_str(), "2" | "8" | "10" | "16") || !suffix.starts_with('u') {
        return None;
    }
    let adjacent_text = format!("{base}{suffix}");
    let adjacent_in_source = source
        .get(item.start..item.end)
        .is_some_and(|item_source| item_source.contains(&adjacent_text));
    if !adjacent_in_source {
        return None;
    }
    parse_byte_literal_parts(base, suffix).ok()
}

fn parse_byte_literal_parts(base: &str, suffix: &str) -> Result<(u8, String, u8), String> {
    let radix = match base {
        "2" => 2,
        "8" => 8,
        "10" => 10,
        "16" => 16,
        _ => {
            return Err("byte literal base must be one of `2`, `8`, `10`, or `16`".to_owned());
        }
    };
    let Some(digits) = suffix.strip_prefix('u') else {
        return Err("byte literal must use explicit base notation such as `16uFF`".to_owned());
    };
    if digits.is_empty() {
        return Err("byte literal must include digits after `u`".to_owned());
    }
    if !digits.chars().all(|ch| ch.is_digit(radix as u32)) {
        return Err(format!(
            "byte literal `{base}u{digits}` contains digits outside base {radix}"
        ));
    }
    let value = u16::from_str_radix(digits, radix as u32).map_err(|_| {
        format!("byte literal `{base}u{digits}` could not be parsed in base {radix}")
    })?;
    if value > u8::MAX as u16 {
        return Err(format!(
            "byte literal `{base}u{digits}` evaluates to {value}, but bytes must be 0..255"
        ));
    }
    Ok((radix, digits.to_owned(), value as u8))
}

fn ast_bits_literal(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<(u32, u32, String)> {
    if tokens.first().map(String::as_str) != Some("BITS") {
        return None;
    }
    let (width, radix, digits, _, _) = parse_bits_literal_tokens(tokens).ok()?;
    let (start, end) = span_for_tokens(tokens, item)?;
    if source
        .get(start..end)
        .is_none_or(|literal| !literal.contains(&format!("{radix}u{digits}")))
    {
        return None;
    }
    Some((width, radix, digits))
}

fn parse_bits_literal_tokens(
    tokens: &[String],
) -> Result<(u32, u32, String, usize, usize), String> {
    if tokens.first().map(String::as_str) != Some("BITS") {
        return Err("fixed-width bit literals start with `BITS`".to_owned());
    }
    if tokens.get(1).map(String::as_str) != Some("[") {
        return Err("BITS requires a positive compile-time width in `BITS[N]`".to_owned());
    }
    let width_close =
        matching_close(tokens, 1).ok_or_else(|| "BITS width is missing closing `]`".to_owned())?;
    let width = match &tokens[2..width_close] {
        [width] => width
            .parse::<u32>()
            .map_err(|_| "BITS width must be one positive compile-time integer".to_owned())?,
        _ => {
            return Err("BITS width must be one positive compile-time integer".to_owned());
        }
    };
    if width == 0 {
        return Err("BITS width must be positive; `BITS[0]` is invalid".to_owned());
    }
    let body_open = width_close + 1;
    if tokens.get(body_open).map(String::as_str) != Some("{") {
        return Err("BITS width must be followed by `{ radixudigits }`".to_owned());
    }
    let body_close = matching_close(tokens, body_open)
        .ok_or_else(|| "BITS literal is missing closing `}`".to_owned())?;
    if body_close + 1 != tokens.len() {
        return Err("BITS literal must contain exactly one encoded integer token".to_owned());
    }
    let [base, suffix] = &tokens[body_open + 1..body_close] else {
        return Err("BITS body must contain exactly one token such as `2u1010`".to_owned());
    };
    let radix = base
        .parse::<u32>()
        .map_err(|_| "BITS literal radix must be an integer from 2 through 36".to_owned())?;
    if !(2..=36).contains(&radix) {
        return Err(format!(
            "BITS literal radix must be between 2 and 36, found {radix}"
        ));
    }
    let Some(digits) = suffix.strip_prefix('u') else {
        return Err("BITS literal must use explicit radix notation such as `16uFF`".to_owned());
    };
    if digits.is_empty() {
        return Err("BITS literal must include digits after `u`".to_owned());
    }
    let normalized = digits.chars().filter(|ch| *ch != '_').collect::<String>();
    if normalized.is_empty() {
        return Err("BITS literal must include at least one non-underscore digit".to_owned());
    }
    if !normalized.chars().all(|ch| ch.is_digit(radix)) {
        return Err(format!(
            "BITS literal `{radix}u{digits}` contains digits outside radix {radix}"
        ));
    }
    Ok((
        width,
        radix,
        digits.to_owned(),
        body_open + 1,
        body_open + 2,
    ))
}

fn ast_pipe_expr_kind(
    tokens: &[String],
    pipe: usize,
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> AstExprKind {
    let input = parse_ast_expr(&tokens[..pipe], item, expressions, source);
    let op = tokens
        .get(pipe + 1)
        .cloned()
        .unwrap_or_else(|| "pipe".to_owned());
    if op == "."
        && pipe + 3 == tokens.len()
        && tokens.get(pipe + 2).is_some_and(|field| is_name(field))
    {
        return AstExprKind::Pipe {
            input,
            op: format!("Field/{}", tokens[pipe + 2]),
            args: Vec::new(),
            pass: None,
            arms: Vec::new(),
        };
    }
    if op == "DRAINING" && pipe + 2 == tokens.len() {
        return AstExprKind::Draining { input };
    }
    if op == "HOLD" {
        let name = tokens
            .get(pipe + 2)
            .cloned()
            .unwrap_or_else(|| "hold".to_owned());
        push_inline_hold_latest_exprs(&tokens[pipe + 3..], item, expressions, source);
        return AstExprKind::Hold {
            initial: input,
            name,
        };
    }
    if op == "WHEN" {
        let arms = parse_inline_match_arms(&tokens[pipe + 1..], item, expressions, source);
        return AstExprKind::When { input, arms };
    }
    if op == "THEN" {
        return AstExprKind::Then {
            input,
            output: ast_operator_block_expr(&tokens[pipe + 1..], item, expressions, source),
        };
    }
    let (args, pass) = ast_call_args_after_operator(&tokens[pipe + 1..], item, expressions, source);
    let arms = if op == "WHILE" {
        parse_inline_match_arms(&tokens[pipe + 1..], item, expressions, source)
    } else {
        Vec::new()
    };
    AstExprKind::Pipe {
        input,
        op,
        args,
        pass,
        arms,
    }
}

fn ast_record_fields(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<AstRecordField> {
    split_top_level(&tokens[1..tokens.len() - 1], ",")
        .into_iter()
        .enumerate()
        .filter_map(|(index, part)| {
            if part.starts_with(&[".".to_owned(), ".".to_owned(), ".".to_owned()]) && part.len() > 3
            {
                let (start, end) = span_for_tokens(part, item).unwrap_or((item.start, item.end));
                return Some(AstRecordField {
                    name: format!("__spread_{index}"),
                    value: parse_ast_expr(&part[3..], item, expressions, source),
                    start,
                    end,
                    spread: true,
                });
            }
            if part.len() < 3 || part.get(1).map(String::as_str) != Some(":") {
                return None;
            }
            let (start, end) = span_for_tokens(part, item).unwrap_or((item.start, item.end));
            Some(AstRecordField {
                name: part[0].clone(),
                value: parse_ast_expr(&part[2..], item, expressions, source),
                start,
                end,
                spread: false,
            })
        })
        .collect()
}

fn ast_call(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<(String, Vec<AstCallArg>, Option<AstPassContext>)> {
    let open = tokens.iter().position(|token| token == "(")?;
    if open == 0 {
        return None;
    }
    let function = tokens[..open].join("");
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    let arg_tokens = if close > open {
        &tokens[open + 1..close]
    } else {
        &[]
    };
    let (args, pass) = ast_call_parts(arg_tokens, item, expressions, source);
    Some((function, args, pass))
}

fn ast_call_args_after_operator(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> (Vec<AstCallArg>, Option<AstPassContext>) {
    let Some(open) = tokens.iter().position(|token| token == "(") else {
        return (Vec::new(), None);
    };
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    let arg_tokens = if close > open {
        &tokens[open + 1..close]
    } else {
        &[]
    };
    ast_call_parts(arg_tokens, item, expressions, source)
}

fn ast_call_parts(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> (Vec<AstCallArg>, Option<AstPassContext>) {
    let mut args = Vec::new();
    let mut pass = None;
    let parts = split_top_level(tokens, ",");
    for (index, part) in parts.iter().copied().enumerate() {
        if part.first().map(String::as_str) == Some("PASS")
            && part.get(1).map(String::as_str) == Some(":")
        {
            let (start, end) = span_for_tokens(part, item).unwrap_or((item.start, item.end));
            let value = parse_ast_expr(&part[2..], item, expressions, source);
            if pass.is_none() {
                pass = Some(AstPassContext {
                    value,
                    start,
                    end,
                    final_clause: index + 1 == parts.len(),
                });
            } else {
                args.push(AstCallArg {
                    kind: AstCallArgKind::Named,
                    name: "PASS".to_owned(),
                    value,
                    start,
                    end,
                });
            }
        } else if let Some(arg) = ast_call_arg(part, item, expressions, source) {
            args.push(arg);
        }
    }
    (args, pass)
}

fn ast_operator_block_expr(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<usize> {
    let open = tokens.iter().position(|token| token == "{")?;
    let close = matching_close(tokens, open)?;
    (close > open + 1).then(|| parse_ast_expr(&tokens[open + 1..close], item, expressions, source))
}

fn push_inline_hold_latest_exprs(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) {
    let Some(latest) = tokens.iter().position(|token| token == "LATEST") else {
        return;
    };
    let _ = parse_ast_expr(&tokens[latest..], item, expressions, source);
}

fn ast_latest_branches(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    let inner = &tokens[open + 1..close];
    let Some(token_offset) = item
        .symbols
        .windows(tokens.len())
        .position(|window| window == tokens)
        .map(|offset| offset + open + 1)
    else {
        return Vec::new();
    };
    let first_offset = item.symbol_spans.get(token_offset).map(|span| span.0);
    let base_indent = first_offset.map_or(0, |offset| source_line_indent(source, offset));
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    for (index, token) in inner.iter().enumerate() {
        let token = token.as_str();
        if depth == 0 && token == "," {
            if start < index {
                ranges.push(start..index);
            }
            start = index + 1;
            continue;
        }
        if index > start
            && depth == 0
            && !latest_continuation_token(token)
            && latest_token_starts_peer_line(
                item,
                source,
                token_offset + index - 1,
                token_offset + index,
                base_indent,
            )
        {
            ranges.push(start..index);
            start = index;
        }
        match token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if start < inner.len() {
        ranges.push(start..inner.len());
    }
    ranges
        .into_iter()
        .filter(|range| range.start < range.end)
        .map(|range| parse_ast_expr(&inner[range], item, expressions, source))
        .collect()
}

fn latest_token_starts_peer_line(
    item: &ParserItem,
    source: &str,
    previous: usize,
    current: usize,
    base_indent: usize,
) -> bool {
    let Some(previous_end) = item.symbol_spans.get(previous).map(|span| span.1) else {
        return false;
    };
    let Some(current_start) = item.symbol_spans.get(current).map(|span| span.0) else {
        return false;
    };
    source
        .get(previous_end..current_start)
        .is_some_and(|gap| gap.contains('\n'))
        && source_line_indent(source, current_start) <= base_indent
}

fn latest_continuation_token(token: &str) -> bool {
    matches!(
        token,
        "|>" | "+" | "-" | "*" | "/" | "%" | "==" | "!=" | ">" | "<" | ">=" | "<="
    )
}

fn source_line_indent(source: &str, offset: usize) -> usize {
    let line_start = source
        .get(..offset)
        .and_then(|prefix| prefix.rfind('\n'))
        .map_or(0, |newline| newline + 1);
    source
        .get(line_start..offset)
        .unwrap_or_default()
        .chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .count()
}

fn parse_inline_match_arms(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter(|part| part.iter().any(|token| token == "=>"))
        .map(|part| {
            let expression = parse_ast_expr(part, item, expressions, source);
            consume_arrow_as_match_arm(expression, expressions);
            expression
        })
        .collect()
}

fn ast_call_arg(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<AstCallArg> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.get(1).map(String::as_str) == Some(":") {
        let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
        return Some(AstCallArg {
            kind: AstCallArgKind::Named,
            name: tokens[0].clone(),
            value: parse_ast_expr(&tokens[2..], item, expressions, source),
            start,
            end,
        });
    }
    let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
    let name = if tokens.len() == 1 && is_name(&tokens[0]) {
        tokens[0].clone()
    } else {
        String::new()
    };
    Some(AstCallArg {
        kind: AstCallArgKind::BareBinding,
        name,
        value: parse_ast_expr(tokens, item, expressions, source),
        start,
        end,
    })
}

fn ast_function_parameters(tokens: &[String], item: &ParserItem) -> Vec<AstParameter> {
    let Some(open) = tokens.iter().position(|token| token == "(") else {
        return Vec::new();
    };
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, part)| {
            let name = part.first()?.clone();
            let kind = if part.get(1).map(String::as_str) == Some(":")
                && part.get(2).map(String::as_str) == Some("OUT")
            {
                AstParameterKind::Out
            } else {
                AstParameterKind::Value
            };
            let (start, end) = span_for_tokens(part, item).unwrap_or((item.start, item.end));
            Some(AstParameter {
                name,
                kind,
                ordinal,
                start,
                end,
            })
        })
        .collect()
}

fn find_top_level_pipe(tokens: &[String]) -> Option<usize> {
    let mut depth = 0i32;
    let mut pipe = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == "|>" && depth == 0 {
            pipe = Some(index);
        }
    }
    pipe
}

fn find_top_level_token(tokens: &[String], needle: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == needle && depth == 0 {
            return Some(index);
        }
    }
    None
}

fn split_infix(tokens: &[String]) -> Option<(&[String], &str, &[String])> {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            token
                if is_infix_operator(token)
                    && depth == 0
                    && index > 0
                    && index + 1 < tokens.len() =>
            {
                return Some((&tokens[..index], token, &tokens[index + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn split_leading_infix(tokens: &[String]) -> Option<(&str, &[String])> {
    let (operator, right) = tokens.split_first()?;
    (!right.is_empty() && is_infix_operator(operator)).then_some((operator, right))
}

fn is_infix_operator(token: &str) -> bool {
    matches!(
        token,
        "==" | ">" | "<" | ">=" | "<=" | "!=" | "+" | "-" | "*" | "/" | "%"
    )
}

fn split_postfix_field_access(tokens: &[String]) -> Option<(&[String], String)> {
    let mut depth = 0i32;
    let mut dot = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            "." if depth == 0 && index > 0 && index + 1 < tokens.len() => dot = Some(index),
            _ => {}
        }
    }
    let dot = dot?;
    let input = &tokens[..dot];
    if !input.iter().any(|token| token == ")") {
        return None;
    }
    let field_tokens = &tokens[dot + 1..];
    if field_tokens.is_empty()
        || field_tokens.iter().any(|token| token == ".")
        || !field_tokens.iter().all(|token| is_name(token))
    {
        return None;
    }
    Some((input, field_tokens.join("")))
}

fn matching_close(tokens: &[String], open: usize) -> Option<usize> {
    let close_token = match tokens.get(open).map(String::as_str)? {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let mut stack = vec![close_token];
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        match token.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop() != Some(token.as_str()) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level<'a>(tokens: &'a [String], separator: &str) -> Vec<&'a [String]> {
    let mut groups = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == separator && depth == 0 {
            groups.push(&tokens[start..index]);
            start = index + 1;
        }
    }
    if start < tokens.len() {
        groups.push(&tokens[start..]);
    }
    groups
}

fn path_segments(tokens: &[String]) -> Vec<String> {
    let mut segments = Vec::new();
    for token in tokens.iter().filter(|token| token.as_str() != ".") {
        if let Some((role, first)) = split_role_value_head(token) {
            segments.push(role.to_owned());
            segments.push(first.to_owned());
        } else if is_name(token) {
            segments.push(token.clone());
        }
    }
    segments
}

fn split_role_value_head(value: &str) -> Option<(&str, &str)> {
    let (role, first) = value.split_once('/')?;
    (is_program_role_root(role) && is_name(first)).then_some((role, first))
}

struct ParsedTextLiteral {
    value: String,
    value_start: usize,
}

fn text_literal_value(tokens: &[String], item: &ParserItem, source: &str) -> Option<String> {
    parsed_text_literal(tokens, item, source).map(|text| text.value)
}

fn parsed_text_literal(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<ParsedTextLiteral> {
    if tokens.first().map(String::as_str) != Some("TEXT")
        || tokens.get(1).map(String::as_str) != Some("{")
    {
        return None;
    }
    if tokens == ["TEXT", "{"] {
        return text_literal_source_value_from_start(item.start, source);
    }
    let close = tokens.iter().rposition(|token| token == "}")?;
    if close + 1 != tokens.len() {
        return None;
    }
    if let Some(text) = text_literal_source_value(tokens, item, source) {
        return Some(text);
    }
    Some(ParsedTextLiteral {
        value: join_text_literal_tokens(&tokens[2..close]),
        value_start: span_for_tokens(tokens, item)
            .and_then(|(start, end)| source.get(start..end).map(|slice| (start, slice)))
            .and_then(|(start, slice)| slice.find('{').map(|open| start + open + 1))
            .unwrap_or(item.start),
    })
}

fn text_literal_source_value(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<ParsedTextLiteral> {
    let (start, end) = span_for_tokens(tokens, item)?;
    let slice = source.get(start..end)?;
    parsed_text_literal_source_slice(slice, start)
}

fn parsed_text_literal_source_slice(slice: &str, slice_start: usize) -> Option<ParsedTextLiteral> {
    let text_start = slice.find("TEXT")?;
    let open = text_start + slice[text_start..].find('{')?;
    let content_start = open + 1;
    let mut depth = 1i32;
    for (offset, ch) in slice[content_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let raw = &slice[content_start..content_start + offset];
                    let value = raw.trim();
                    let leading = raw.len() - raw.trim_start().len();
                    return Some(ParsedTextLiteral {
                        value: value.to_owned(),
                        value_start: slice_start + content_start + leading,
                    });
                }
            }
            _ => {}
        }
    }
    None
}

fn text_literal_source_value_from_start(start: usize, source: &str) -> Option<ParsedTextLiteral> {
    let slice = source.get(start..)?;
    parsed_text_literal_source_slice(slice, start)
}

fn ast_text_template_segments(
    text: &str,
    text_start: usize,
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<Vec<AstTextSegment>> {
    let mut cursor = 0usize;
    let mut segments = Vec::new();
    while let Some(relative_open) = text[cursor..].find('{') {
        let open = cursor + relative_open;
        if open > cursor {
            segments.push(AstTextSegment::Static {
                value: text[cursor..open].to_owned(),
            });
        }
        let Some(relative_close) = text[open + 1..].find('}') else {
            let raw = text[open + 1..].trim();
            let leading = text[open + 1..].len() - text[open + 1..].trim_start().len();
            let start = text_start + open + 1 + leading;
            let value = push_ast_expr_at_source_span(
                item,
                expressions,
                AstExprKind::Unknown(vec![raw.to_owned()]),
                start,
                start + raw.len(),
                source,
            );
            segments.push(AstTextSegment::Dynamic { value });
            cursor = text.len();
            break;
        };
        let close = open + 1 + relative_close;
        let inner = &text[open + 1..close];
        let trimmed = inner.trim();
        let raw = trimmed.trim_start_matches('$');
        let parts = raw.split('.').collect::<Vec<_>>();
        let kind = if parts.len() == 1 && parts[0].contains('/') {
            let path = path_segments(&[parts[0].to_owned()]);
            if path.is_empty() {
                AstExprKind::Unknown(vec![raw.to_owned()])
            } else {
                AstExprKind::Path(path)
            }
        } else if parts.len() == 1 && is_name(parts[0]) {
            AstExprKind::Identifier(parts[0].to_owned())
        } else if !parts.is_empty() && parts.iter().all(|part| is_name(part)) {
            AstExprKind::Path(parts.into_iter().map(str::to_owned).collect())
        } else {
            AstExprKind::Unknown(vec![raw.to_owned()])
        };
        let leading = inner.len() - inner.trim_start().len();
        let dollar_prefix = trimmed.len() - raw.len();
        let start = text_start + open + 1 + leading + dollar_prefix;
        let value =
            push_ast_expr_at_source_span(item, expressions, kind, start, start + raw.len(), source);
        segments.push(AstTextSegment::Dynamic { value });
        cursor = close + 1;
    }
    if segments.is_empty() {
        return None;
    }
    if cursor < text.len() {
        segments.push(AstTextSegment::Static {
            value: text[cursor..].to_owned(),
        });
    }
    Some(segments)
}

fn push_ast_expr_at_source_span(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    kind: AstExprKind,
    start: usize,
    end: usize,
    source: &str,
) -> usize {
    let value = push_ast_expr(item, expressions, kind, start, end);
    expressions[value].line = source
        .get(..start)
        .map(|prefix| prefix.bytes().filter(|byte| *byte == b'\n').count() + 1)
        .unwrap_or(item.line);
    value
}

fn string_literal_value(tokens: &[String]) -> Option<String> {
    if tokens.len() != 1 {
        return None;
    }
    tokens[0]
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(unescape_string_literal)
}

fn unescape_string_literal(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                output.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn join_text_literal_tokens(tokens: &[String]) -> String {
    let mut output = String::new();
    let mut previous = "";
    for token in tokens {
        if output.is_empty() {
            output.push_str(token);
        } else if text_literal_needs_space(previous, token) {
            output.push(' ');
            output.push_str(token);
        } else {
            output.push_str(token);
        }
        previous = token;
    }
    output
}

fn text_literal_needs_space(previous: &str, current: &str) -> bool {
    if matches!(
        current,
        "[" | "(" | "{" | "]" | ")" | "}" | "," | "." | ":" | ";" | "%"
    ) {
        return false;
    }
    if matches!(previous, "[" | "(" | "{" | "." | ":" | "#" | "/" | "%") {
        return false;
    }
    if previous.chars().all(|ch| ch.is_ascii_digit())
        && current
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, 'x' | 'X'))
    {
        return false;
    }
    true
}

fn value_starts_uppercase_identifier(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn ast_field_name(symbols: &[String]) -> Option<&str> {
    if symbols.get(1).map(String::as_str) != Some(":") {
        return None;
    }
    let name = symbols.first()?.as_str();
    is_name(name).then_some(name)
}

fn ast_insource_slice_event(symbols: &[String]) -> Option<&str> {
    let open = symbols.iter().position(|lexeme| lexeme == "[")?;
    let event = symbols.get(open + 1)?.as_str();
    (symbols.get(open + 2).map(String::as_str) == Some(":")
        && symbols.iter().any(|lexeme| lexeme == "SOURCE")
        && is_name(event))
    .then_some(event)
}

fn ast_hold_name(symbols: &[String]) -> Option<&str> {
    let hold = symbols.iter().position(|lexeme| lexeme == "HOLD")?;
    symbols
        .get(hold + 1)
        .map(String::as_str)
        .filter(|name| is_name(name))
}

fn ast_list_capacity(symbols: &[String]) -> Option<usize> {
    let list = symbols.iter().position(|lexeme| lexeme == "LIST")?;
    (symbols.get(list + 1).map(String::as_str) == Some("["))
        .then(|| symbols.get(list + 2))?
        .and_then(|value| value.parse().ok())
}

fn ast_list_items(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    ast_collection_items(tokens, item, expressions, source)
}

fn ast_collection_items(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_ast_expr(part, item, expressions, source))
        .collect()
}

fn ast_map_entries(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| {
            let expression = parse_ast_expr(part, item, expressions, source);
            consume_arrow_as_map_entry(expression, expressions);
            expression
        })
        .collect()
}

fn ast_bytes_literal(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<(BytesSizeSyntax, Vec<usize>)> {
    if tokens.first().map(String::as_str) != Some("BYTES") {
        return None;
    }
    let (size, body_open) = match tokens.get(1).map(String::as_str) {
        Some("{") => (BytesSizeSyntax::Dynamic, 1),
        Some("[") => {
            let close = matching_close(tokens, 1)?;
            let size_tokens = &tokens[2..close];
            let size = match size_tokens {
                [value] if value == "__" => BytesSizeSyntax::Infer,
                [value] => BytesSizeSyntax::Fixed(value.parse().ok()?),
                _ => return None,
            };
            if tokens.get(close + 1).map(String::as_str) != Some("{") {
                return None;
            }
            (size, close + 1)
        }
        _ => return None,
    };
    let body_close = matching_close(tokens, body_open)?;
    if body_close + 1 != tokens.len() {
        return None;
    }
    let items = if body_close <= body_open + 1 {
        Vec::new()
    } else {
        split_top_level(&tokens[body_open + 1..body_close], ",")
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(|part| parse_ast_expr(part, item, expressions, source))
            .collect()
    };
    Some((size, items))
}

fn ast_opens_scope(symbols: &[String]) -> bool {
    matches!(
        symbols.last().map(String::as_str),
        Some(":") | Some("[") | Some("{")
    ) || symbols.windows(2).any(|window| {
        window[0] == ":" && window[1] == "[" && !symbols.iter().any(|lexeme| lexeme == "]")
    })
}

fn ast_expression_operators(symbols: &[String]) -> Vec<String> {
    expression_operators(symbols.iter().map(String::as_str))
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn detect_program_kind() -> ProgramKind {
    ProgramKind::Generic
}

fn validate_source_syntax_with_work(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let example_source = path.contains("/examples/") || path.starts_with("examples/");
    let mut work_batch = work.batch();
    for token in &ast.tokens {
        work_batch.tokens(1);
        work_batch.validation();
        if matches!(token.kind, AstTokenKind::String | AstTokenKind::Comment)
            || containing_source_span(&validation_index.text_literal_spans, token.start, token.end)
                .is_some()
        {
            continue;
        }
        if token.lexeme == "EXAMPLE" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`EXAMPLE` is not Boon syntax; put example identity in the manifest/dev metadata",
            ));
        }
        if token.lexeme == "#" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`#` comments are not supported in Boon source; use `--` comments",
            ));
        }
        if token.lexeme == "LINK" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`LINK` is not supported in boon-circuit examples; declare input ports with `SOURCE`",
            ));
        }
        if let Some(feature) = planned_rejected_feature_for_spelling(&token.lexeme) {
            return Err(error(
                path,
                token.line,
                token.column,
                &format!(
                    "`{}` belongs to planned language feature `{}` and is rejected until that feature is implemented",
                    token.lexeme, feature.id
                ),
            ));
        }
        if example_source && matches!(token.lexeme.as_str(), "bg" | "fill" | "true" | "false") {
            return Err(error(
                path,
                token.line,
                token.column,
                "Boon examples must use canonical names such as `background`, `Fill`, `True`, and `False`",
            ));
        }
    }
    for item in &ast.items {
        work_batch.validation();
        work_batch.symbols(item.symbols.len());
        for (index, window) in item.symbols.windows(2).enumerate() {
            work_batch.symbols(window.len());
            if matches!(window, [pipe, op] if pipe == "|>" && op == "LINK") {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "`|> LINK` is not supported; bind source ports through an Element constructor's `element.events` field",
                ));
            }
            if matches!(window, [pipe, op] if pipe == "|>" && op == "SOURCE") {
                return Err(error(
                    path,
                    item.line,
                    item.symbol_spans
                        .get(index + 1)
                        .map_or(item.indent + 1, |(start, _)| {
                            start.saturating_sub(item.start) + 1
                        }),
                    "`|> SOURCE { ... }` routing was removed; bind the source directly through the Element constructor's `element.events` field",
                ));
            }
            let op = window[1].as_str();
            if window[0] == "|>"
                && pipeline_operator_requires_call_parentheses(op)
                && !pipeline_field_projection(&item.symbols, index)
                && item.symbols.get(index + 2).map(String::as_str) != Some("(")
            {
                return Err(error(
                    path,
                    item.line,
                    item.symbol_spans
                        .get(index + 1)
                        .map_or(item.indent + 1, |(start, _)| {
                            start.saturating_sub(item.start) + 1
                        }),
                    &format!("pipeline function `{op}` must be called with parentheses: `{op}()`"),
                ));
            }
        }
    }
    validate_reserved_standard_namespaces(path, &ast.statements, work)?;
    validate_role_qualified_value_syntax(path, &ast.tokens, work)?;
    validate_function_parameter_syntax(path, ast, work)?;
    validate_call_entry_syntax(path, ast, work)?;
    if example_source && let Some(document) = document_statement(ast) {
        let document_is_canonical = document.expr.is_some_and(|expr_id| {
            ast.expressions.get(expr_id).is_some_and(|expr| {
                matches!(&expr.kind, AstExprKind::Call { function, .. } if function == "Document/new")
            })
        });
        if !document_is_canonical || statement_has_field(document, "kind", work) {
            return Err(error(
                path,
                document.line,
                document.indent + 1,
                "example documents must use `Document/new(root: Element/...)`",
            ));
        }
    }
    validate_drain_syntax(path, ast, validation_index, work)?;
    validate_bits_syntax(path, ast, validation_index, work)?;
    validate_bytes_syntax(path, ast, validation_index, work)?;
    Ok(())
}

fn planned_rejected_feature_for_spelling(
    spelling: &str,
) -> Option<&'static boon_syntax::LanguageFeatureSpec> {
    static INDEX: OnceLock<Box<[(&'static str, &'static boon_syntax::LanguageFeatureSpec)]>> =
        OnceLock::new();
    INDEX
        .get_or_init(|| {
            let mut index = Vec::new();
            for feature in LANGUAGE_FEATURE_REGISTRY {
                if feature.stage != LanguageFeatureStage::Planned
                    || feature.parse_expectation != LanguageFeatureParseExpectation::Reject
                {
                    continue;
                }
                for spelling in feature.spellings {
                    if !index.iter().any(|(existing, _)| existing == spelling) {
                        index.push((*spelling, feature));
                    }
                }
            }
            index.into_boxed_slice()
        })
        .iter()
        .find_map(|(candidate, feature)| (*candidate == spelling).then_some(*feature))
}

fn validate_match_patterns(
    path: &str,
    ast: &AstProgram,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for expression in &ast.expressions {
        work_batch.expression();
        work_batch.validation();
        if matches!(expression.kind, AstExprKind::Arrow { .. }) {
            let column = ast
                .tokens
                .iter()
                .find(|token| {
                    work_batch.tokens(1);
                    work_batch.validation();
                    token.line == expression.line && token.start >= expression.start
                })
                .map_or(1, |token| token.column);
            return Err(error(
                path,
                expression.line,
                column,
                "`=>` must be an entry of `MAP { ... }` or an arm of `WHEN`/`WHILE`",
            ));
        }
        let AstExprKind::MatchArm {
            pattern: AstMatchPattern::Invalid { message },
            ..
        } = &expression.kind
        else {
            continue;
        };
        let column = ast
            .tokens
            .iter()
            .find(|token| {
                work_batch.tokens(1);
                work_batch.validation();
                token.line == expression.line && token.start >= expression.start
            })
            .map_or(1, |token| token.column);
        return Err(error(path, expression.line, column, message));
    }
    Ok(())
}

fn validate_function_parameter_syntax(
    path: &str,
    ast: &AstProgram,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for item in &ast.items {
        work_batch.validation();
        work_batch.symbols(item.symbols.len());
        if item.function.is_none() {
            continue;
        }
        let Some(open) = item.symbols.iter().position(|symbol| symbol == "(") else {
            continue;
        };
        let close = matching_close(&item.symbols, open).unwrap_or(item.symbols.len());
        let mut names = BTreeSet::new();
        for part in split_top_level(&item.symbols[open + 1..close], ",") {
            if part.is_empty() {
                continue;
            }
            let valid = matches!(part, [name] if is_name(name))
                || matches!(part, [name, colon, out] if is_name(name) && colon == ":" && out == "OUT");
            let name = part.first().map(String::as_str).unwrap_or_default();
            let column = item
                .symbol_spans
                .iter()
                .zip(&item.symbols)
                .find_map(|((start, _), symbol)| (symbol == name).then_some(*start))
                .map_or(item.indent + 1, |start| {
                    start.saturating_sub(item.start) + 1
                });
            if !valid {
                return Err(error(
                    path,
                    item.line,
                    column,
                    "function parameters must be `name` or `name: OUT`",
                ));
            }
            if matches!(name, "PASS" | "PASSED" | "OUT") {
                return Err(error(
                    path,
                    item.line,
                    column,
                    "`PASS`, `PASSED`, and `OUT` are reserved and cannot be function parameter names",
                ));
            }
            if !names.insert(name.to_owned()) {
                return Err(error(
                    path,
                    item.line,
                    column,
                    &format!("duplicate function parameter `{name}`"),
                ));
            }
        }
    }
    Ok(())
}

fn validate_call_entry_syntax(
    path: &str,
    ast: &AstProgram,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for expr in &ast.expressions {
        work_batch.expression();
        work_batch.validation();
        let (function, args, pass) = match &expr.kind {
            AstExprKind::Call {
                function,
                args,
                pass,
            } => (function.as_str(), args.as_slice(), pass.as_ref()),
            AstExprKind::Pipe { op, args, pass, .. } => {
                (op.as_str(), args.as_slice(), pass.as_ref())
            }
            _ => continue,
        };
        let mut names = BTreeSet::new();
        for arg in args {
            let column = parser_column_for_offset(ast, expr.line, arg.start);
            if arg.is_bare_binding() && arg.name.is_empty() {
                return Err(error(
                    path,
                    expr.line,
                    column,
                    &format!(
                        "bare entry in `{function}` must be one identifier naming a declared OUT parameter; ordinary arguments use `name: expression`"
                    ),
                ));
            }
            if arg.name == "PASS" {
                return Err(error(
                    path,
                    expr.line,
                    column,
                    "`PASS` may appear only once as the final call clause",
                ));
            }
            if !names.insert(arg.name.as_str()) {
                return Err(error(
                    path,
                    expr.line,
                    column,
                    &format!("duplicate call entry `{}` in `{function}`", arg.name),
                ));
            }
        }
        if let Some(pass) = pass
            && !pass.final_clause
        {
            return Err(error(
                path,
                expr.line,
                parser_column_for_offset(ast, expr.line, pass.start),
                "`PASS` must be the final call clause",
            ));
        }
    }
    Ok(())
}

fn parser_column_for_offset(ast: &AstProgram, line: usize, offset: usize) -> usize {
    ast.lines
        .binary_search_by_key(&line, |candidate| candidate.line)
        .ok()
        .and_then(|index| ast.lines.get(index))
        .map_or(1, |candidate| offset.saturating_sub(candidate.start) + 1)
}

fn pipeline_operator_requires_call_parentheses(operator: &str) -> bool {
    !matches!(
        operator,
        "HOLD" | "LATEST" | "WHEN" | "WHILE" | "THEN" | "DRAIN" | "DRAINING" | "SOURCE"
    )
}

fn pipeline_field_projection(symbols: &[String], pipe: usize) -> bool {
    symbols.get(pipe).map(String::as_str) == Some("|>")
        && symbols.get(pipe + 1).map(String::as_str) == Some(".")
        && symbols.get(pipe + 2).is_some_and(|field| is_name(field))
        && symbols
            .get(pipe + 3)
            .is_none_or(|next| next.as_str() == "|>")
}

fn validate_reserved_standard_namespaces(
    path: &str,
    statements: &[AstStatement],
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for statement in statements {
        work_batch.statement();
        work_batch.validation();
        if let AstStatementKind::Field { name } = &statement.kind
            && is_reserved_standard_root(name)
        {
            return Err(error(
                path,
                statement.line,
                statement.indent + 1,
                &format!(
                    "`{name}` is a reserved Boon standard root and cannot be declared by an application"
                ),
            ));
        }
    }
    validate_reserved_standard_function_names(path, statements, &mut work_batch)
}

fn validate_reserved_standard_function_names(
    path: &str,
    statements: &[AstStatement],
    work: &mut ParseWorkBatch<'_>,
) -> Result<(), ParseError> {
    for statement in statements {
        work.statement();
        work.validation();
        if let AstStatementKind::Function { name, .. } = &statement.kind
            && name
                .split('/')
                .next()
                .is_some_and(is_reserved_standard_root)
        {
            return Err(error(
                path,
                statement.line,
                statement.indent + 1,
                &format!(
                    "`{name}` is in a reserved Boon standard namespace and cannot be declared by an application"
                ),
            ));
        }
        validate_reserved_standard_function_names(path, &statement.children, work)?;
    }
    Ok(())
}

fn validate_role_qualified_value_syntax(
    path: &str,
    tokens: &[AstToken],
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for window in tokens.windows(2) {
        work_batch.tokens(window.len());
        work_batch.validation();
        let [role, separator] = window else {
            continue;
        };
        if role.kind == AstTokenKind::Identifier
            && is_program_role_root(&role.lexeme)
            && separator.lexeme == "."
        {
            return Err(error(
                path,
                role.line,
                role.column,
                &format!(
                    "qualified role values use `{}/value.field`, not `{}.value.field`",
                    role.lexeme, role.lexeme
                ),
            ));
        }
    }
    Ok(())
}

fn validate_drain_syntax(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let tokens = validation_index.drain_tokens(ast).collect::<Vec<_>>();
    let mut work_batch = work.batch();

    let mut index = 0usize;
    while let Some(token) = tokens.get(index) {
        work_batch.tokens(1);
        work_batch.validation();
        if token.lexeme != "DRAIN" {
            index += 1;
            continue;
        }
        if tokens.get(index + 1).map(|token| token.lexeme.as_str()) != Some("{") {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` requires a `{ path }` body",
            ));
        }
        let Some(close) = matching_semantic_brace(&tokens, index + 1, work) else {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` is missing closing `}`",
            ));
        };
        let body = tokens[index + 2..close]
            .iter()
            .map(|token| {
                work_batch.tokens(1);
                work_batch.validation();
                token.lexeme.clone()
            })
            .collect::<Vec<_>>();
        if drain_path_from_symbols(&body).is_none() {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` body must contain exactly one named binding, field path, or `PASSED` path",
            ));
        }
        index = close + 1;
    }

    let depths = semantic_token_depths(&tokens, work);
    for (index, token) in tokens.iter().enumerate() {
        work_batch.tokens(1);
        work_batch.validation();
        if token.lexeme != "DRAINING" {
            continue;
        }
        if index
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
            .is_none_or(|previous| previous.lexeme != "|>")
            || !draining_pipe_has_input(&tokens, index)
        {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAINING` must be used as terminal `input |> DRAINING` syntax",
            ));
        }
        if draining_has_trailing_pipeline_syntax(&tokens, &depths, index) {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAINING` must be terminal in its pipeline",
            ));
        }
    }
    Ok(())
}

fn matching_semantic_brace(
    tokens: &[&AstToken],
    open: usize,
    work: &ParseWorkRecorder,
) -> Option<usize> {
    let mut work_batch = work.batch();
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        work_batch.tokens(1);
        work_batch.validation();
        match token.lexeme.as_str() {
            "{" => depth += 1,
            "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn semantic_token_depths(tokens: &[&AstToken], work: &ParseWorkRecorder) -> Vec<i32> {
    let mut work_batch = work.batch();
    let mut depth = 0i32;
    tokens
        .iter()
        .map(|token| {
            work_batch.tokens(1);
            work_batch.validation();
            let current = depth;
            match token.lexeme.as_str() {
                "[" | "{" | "(" => depth += 1,
                "]" | "}" | ")" => depth -= 1,
                _ => {}
            }
            current
        })
        .collect()
}

fn draining_pipe_has_input(tokens: &[&AstToken], draining: usize) -> bool {
    let Some(pipe) = draining.checked_sub(1) else {
        return false;
    };
    let Some(previous) = pipe.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    !matches!(
        previous.lexeme.as_str(),
        ":" | "," | "(" | "[" | "{" | "=>" | "|>"
    )
}

fn draining_has_trailing_pipeline_syntax(
    tokens: &[&AstToken],
    depths: &[i32],
    draining: usize,
) -> bool {
    let Some(marker) = tokens.get(draining) else {
        return false;
    };
    let marker_depth = depths.get(draining).copied().unwrap_or_default();
    let Some(next) = tokens.get(draining + 1) else {
        return false;
    };
    if next.line == marker.line {
        return match next.lexeme.as_str() {
            "," => marker_depth == 0,
            ")" | "]" | "}" => false,
            _ => true,
        };
    }
    let next_depth = depths.get(draining + 1).copied().unwrap_or_default();
    if matches!(next.lexeme.as_str(), ")" | "]" | "}") && next_depth <= marker_depth {
        return false;
    }
    next.lexeme == "|>" && next_depth == marker_depth
}

fn validate_bytes_syntax(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for window in ast.tokens.windows(2) {
        work_batch.tokens(window.len());
        work_batch.validation();
        let [base_token, suffix_token] = window else {
            continue;
        };
        if base_token.line != suffix_token.line
            || base_token.end != suffix_token.start
            || matches!(
                base_token.kind,
                AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
            )
            || matches!(
                suffix_token.kind,
                AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
            )
            || containing_source_span(
                &validation_index.text_literal_spans,
                base_token.start,
                base_token.end,
            )
            .is_some()
            || containing_source_span(
                &validation_index.text_literal_spans,
                suffix_token.start,
                suffix_token.end,
            )
            .is_some()
        {
            continue;
        }
        if !matches!(base_token.kind, AstTokenKind::Number) || !suffix_token.lexeme.starts_with('u')
        {
            continue;
        }
        if validation_index
            .bits_encoded_spans
            .iter()
            .any(|(start, end)| base_token.start == *start && suffix_token.end == *end)
        {
            continue;
        }
        parse_byte_literal_parts(&base_token.lexeme, &suffix_token.lexeme)
            .map_err(|message| error(path, base_token.line, base_token.column, message.as_str()))?;
    }
    for (item_id, bytes_index) in &validation_index.bytes_sites {
        let Some(item) = ast.items.get(*item_id) else {
            continue;
        };
        work_batch.validation();
        validate_bytes_site_syntax(
            path,
            item,
            *bytes_index,
            &validation_index.text_literal_spans,
            work,
        )?;
    }
    Ok(())
}

fn bits_literal_tokens_at(symbols: &[String], bits_index: usize) -> &[String] {
    let candidate = &symbols[bits_index..];
    let width_close = candidate
        .get(1)
        .filter(|token| token.as_str() == "[")
        .and_then(|_| matching_close(candidate, 1));
    let body_close = width_close
        .and_then(|close| (candidate.get(close + 1)?.as_str() == "{").then_some(close + 1))
        .and_then(|open| matching_close(candidate, open));
    &candidate[..body_close.map_or(candidate.len(), |close| close + 1)]
}

fn validate_bits_syntax(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for (item_id, bits_index) in &validation_index.bits_sites {
        let Some(item) = ast.items.get(*item_id) else {
            continue;
        };
        work_batch.validation();
        let token_start = item
            .symbol_spans
            .get(*bits_index)
            .map(|(start, _)| *start)
            .unwrap_or(item.start);
        if containing_source_span(
            &validation_index.text_literal_spans,
            token_start,
            token_start,
        )
        .is_some()
        {
            continue;
        }
        let literal = bits_literal_tokens_at(&item.symbols, *bits_index);
        work_batch.symbols(literal.len());
        let (_, radix, digits, base_index, suffix_index) = parse_bits_literal_tokens(literal)
            .map_err(|message| error(path, item.line, item.indent + 1, message.as_str()))?;
        let base_span = item
            .symbol_spans
            .get(*bits_index + base_index)
            .ok_or_else(|| {
                error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BITS literal base token is unavailable",
                )
            })?;
        let suffix_span = item
            .symbol_spans
            .get(*bits_index + suffix_index)
            .ok_or_else(|| {
                error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BITS literal digit token is unavailable",
                )
            })?;
        let base_token = validation_index
            .token_at_start(ast, base_span.0, work)
            .ok_or_else(|| {
                error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BITS literal base source token is unavailable",
                )
            })?;
        if base_token
            .start
            .checked_add(base_token.lexeme.len())
            .is_none_or(|end| end != suffix_span.0)
        {
            return Err(error(
                path,
                item.line,
                item.indent + 1,
                "BITS body must contain one adjacent encoded integer token such as `2u1010`; whitespace and fragments are invalid",
            ));
        }
        let _ = (radix, digits);
    }
    Ok(())
}

fn validate_bytes_site_syntax(
    path: &str,
    item: &ParserItem,
    bytes_index: usize,
    text_literal_spans: &[(usize, usize)],
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let token_start = item
        .symbol_spans
        .get(bytes_index)
        .map(|(start, _)| *start)
        .unwrap_or(item.start);
    if containing_source_span(text_literal_spans, token_start, token_start).is_some() {
        return Ok(());
    }
    match item.symbols.get(bytes_index + 1).map(String::as_str) {
        Some("{") => {
            validate_bytes_body_consumption(path, item, bytes_index + 1, work)?;
        }
        Some("[") => {
            let Some(close) = matching_close(&item.symbols, bytes_index + 1) else {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BYTES size is missing closing `]`",
                ));
            };
            let size_tokens = &item.symbols[bytes_index + 2..close];
            let valid_size =
                matches!(size_tokens, [value] if value == "__" || value.parse::<usize>().is_ok());
            if !valid_size {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BYTES size must be `__` or a non-negative decimal integer in v1",
                ));
            }
            if item.symbols.get(close + 1).map(String::as_str) != Some("{") {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BYTES constructor requires a `{ ... }` body",
                ));
            }
            validate_bytes_body_consumption(path, item, close + 1, work)?;
        }
        _ => {
            return Err(error(
                path,
                item.line,
                item.indent + 1,
                "BYTES constructor requires a `{ ... }` body",
            ));
        }
    }
    Ok(())
}

fn validate_bytes_body_consumption(
    path: &str,
    item: &ParserItem,
    body_open: usize,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    work.symbols(item.symbols.len().saturating_sub(body_open));
    let Some(body_close) = matching_close(&item.symbols, body_open) else {
        return Err(error(
            path,
            item.line,
            item.indent + 1,
            "BYTES constructor is missing closing `}`",
        ));
    };
    let Some(next) = item.symbols.get(body_close + 1) else {
        return Ok(());
    };
    if matches!(next.as_str(), "," | ")" | "]" | "}" | "|>") {
        return Ok(());
    }
    Err(error(
        path,
        item.line,
        item.indent + 1,
        format!("BYTES constructor has unexpected trailing token `{next}`").as_str(),
    ))
}

fn text_literal_token_spans_with_work(
    tokens: &[AstToken],
    work: &ParseWorkRecorder,
) -> Vec<(usize, usize)> {
    let mut work_batch = work.batch();
    let mut spans = Vec::new();
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        work_batch.tokens(2);
        work_batch.validation();
        if tokens[index].lexeme == "TEXT" && tokens[index + 1].lexeme == "{" {
            let start = tokens[index].start;
            let mut depth = 0i32;
            let mut cursor = index + 1;
            while cursor < tokens.len() {
                work_batch.tokens(1);
                work_batch.validation();
                match tokens[cursor].lexeme.as_str() {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            spans.push((start, tokens[cursor].end));
                            index = cursor;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        index += 1;
    }
    spans
}

/// Sorts and merges overlapping source intervals once so all later syntax
/// validators can use a logarithmic containment lookup instead of scanning
/// every text literal for every token. Adjacent intervals remain distinct:
/// membership in two neighboring literals must not create one larger literal.
fn normalized_source_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut normalized = Vec::<(usize, usize)>::with_capacity(spans.len());
    for (start, end) in spans {
        if let Some((_, previous_end)) = normalized.last_mut()
            && start < *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            normalized.push((start, end));
        }
    }
    normalized
}

fn containing_source_span(
    spans: &[(usize, usize)],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let insertion = spans.partition_point(|(span_start, _)| *span_start <= start);
    insertion
        .checked_sub(1)
        .and_then(|index| spans.get(index).copied())
        .filter(|(_, span_end)| end <= *span_end)
}

fn text_literal_body_line_ranges_with_work(
    tokens: &[AstToken],
    work: &ParseWorkRecorder,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        work.tokens(2);
        if tokens[index].lexeme == "TEXT" && tokens[index + 1].lexeme == "{" {
            let start_line = tokens[index].line;
            let mut depth = 0i32;
            let mut cursor = index + 1;
            while cursor < tokens.len() {
                work.tokens(1);
                match tokens[cursor].lexeme.as_str() {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            let end_line = tokens[cursor].line;
                            if end_line > start_line {
                                ranges.push((start_line + 1, end_line));
                            }
                            index = cursor;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        index += 1;
    }
    ranges
}

fn statement_has_field(statement: &AstStatement, needle: &str, work: &ParseWorkRecorder) -> bool {
    fn contains(statement: &AstStatement, needle: &str, work: &mut ParseWorkBatch<'_>) -> bool {
        work.statement();
        work.validation();
        matches!(&statement.kind, AstStatementKind::Field { name } if name == needle)
            || statement
                .children
                .iter()
                .any(|child| contains(child, needle, work))
    }

    contains(statement, needle, &mut work.batch())
}

fn validate_source_unit_boundary_with_work(
    path: &str,
    source: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    validate_balanced_brackets_with_work(path, ast, work)?;
    let mut work_batch = work.batch();
    if let Some(item) = validation_index.semantic_items(ast).next() {
        work_batch.symbols(item.symbols.len());
        work_batch.validation();
        let indentation = source
            .lines()
            .nth(item.line.saturating_sub(1))
            .unwrap_or_default()
            .chars()
            .take_while(|character| matches!(character, ' ' | '\t'))
            .count();
        if indentation > 0 {
            return Err(error(
                path,
                item.line,
                indentation.saturating_add(1),
                "a source unit cannot begin with an indented statement; indentation cannot cross source-unit boundaries",
            ));
        }
    }

    for token in &ast.tokens {
        work_batch.tokens(1);
        work_batch.validation();
        if token.kind != AstTokenKind::Unknown
            || token.lexeme != "\""
            || containing_source_span(&validation_index.text_literal_spans, token.start, token.end)
                .is_some()
        {
            continue;
        }
        let quote_column = source
            .get(token.start..token.end)
            .and_then(|lexeme| {
                lexeme
                    .find('"')
                    .map(|offset| token.column + lexeme[..offset].chars().count())
            })
            .unwrap_or(token.column);
        return Err(error(
            path,
            token.line,
            quote_column,
            "unclosed string literal at end of source unit",
        ));
    }
    Ok(())
}

fn validate_balanced_brackets_with_work(
    path: &str,
    ast: &AstProgram,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    let mut stack = Vec::new();
    for token in &ast.tokens {
        work_batch.tokens(1);
        work_batch.validation();
        if matches!(
            token.kind,
            AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
        ) {
            continue;
        }
        match token.lexeme.as_str() {
            "[" | "{" | "(" => stack.push((token.lexeme.as_str(), token.line, token.column)),
            "]" if stack.pop().map(|(ch, _, _)| ch) != Some("[") => {
                return Err(error(path, token.line, token.column, "unbalanced `]`"));
            }
            "}" if stack.pop().map(|(ch, _, _)| ch) != Some("{") => {
                return Err(error(path, token.line, token.column, "unbalanced `}`"));
            }
            ")" if stack.pop().map(|(ch, _, _)| ch) != Some("(") => {
                return Err(error(path, token.line, token.column, "unbalanced `)`"));
            }
            "]" | "}" | ")" => {}
            _ => {}
        }
    }
    if stack.is_empty() {
        Ok(())
    } else {
        let (ch, line, column) = stack
            .last()
            .copied()
            .expect("stack is known to be nonempty");
        Err(ParseError {
            path: path.to_owned(),
            line: Some(line),
            column: Some(column),
            message: format!("unclosed `{ch}` at line {line}, column {column}"),
        })
    }
}

fn validate_list_capacities_with_work(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for (line_id, list_index) in &validation_index.list_sites {
        let Some(line) = ast.lines.get(*line_id) else {
            continue;
        };
        work_batch.validation();
        if line.symbols.get(*list_index + 1).map(String::as_str) != Some("[") {
            continue;
        }
        let capacity_column =
            ast_token_for_parser_line_symbol(ast, validation_index, line, *list_index + 2)
                .map(|token| token.column)
                .unwrap_or(line.indent + 1);
        let Some(close_offset) = line.symbols[*list_index + 2..]
            .iter()
            .position(|lexeme| lexeme == "]")
        else {
            return Err(error(
                path,
                line.line,
                capacity_column,
                "LIST capacity is missing closing `]`",
            ));
        };
        let capacity_parts = &line.symbols[*list_index + 2..*list_index + 2 + close_offset];
        if capacity_parts.len() != 1
            || capacity_parts
                .first()
                .is_none_or(|capacity| capacity.is_empty())
        {
            return Err(error(
                path,
                line.line,
                capacity_column,
                "LIST capacity must be a positive integer",
            ));
        }
        match capacity_parts[0].parse::<usize>() {
            Ok(value) if value > 0 => {}
            _ => {
                return Err(error(
                    path,
                    line.line,
                    capacity_column,
                    "LIST capacity must be a positive integer",
                ));
            }
        }
    }
    Ok(())
}

fn validate_no_reducer_style_update_with_work(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    if validation_index.semantic_items(ast).any(|item| {
        work_batch.symbols(item.symbols.len());
        work_batch.validation();
        reducer_update_signature(item)
    }) {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: "central reducer `FUNCTION update(state, event)` is not allowed; define local HOLD equations for each value".to_owned(),
        });
    }
    let has_event_source_when = validation_index.semantic_items(ast).any(|item| {
        work_batch.symbols(item.symbols.len());
        work_batch.validation();
        item.contains_sequence(&["event", ".", "source", "|>", "WHEN"])
    });
    let has_state_pipe = validation_index.semantic_items(ast).any(|item| {
        work_batch.symbols(item.symbols.len());
        work_batch.validation();
        item.contains_sequence(&["state", "|>"])
    });
    if has_event_source_when && has_state_pipe {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: "global event-source reducer over `state` is not allowed; each value must declare its own sources".to_owned(),
        });
    }
    Ok(())
}

fn reducer_update_signature(item: &ParserItem) -> bool {
    item.function.as_deref() == Some("update")
        && item.has_lexeme("state")
        && item.has_lexeme("event")
}

fn validate_no_hidden_identity_leak_with_work(
    path: &str,
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Result<(), ParseError> {
    let mut work_batch = work.batch();
    for token in validation_index.semantic_tokens(ast) {
        work_batch.tokens(1);
        work_batch.validation();
        if let Some(needle) = hidden_runtime_identity_token(&token.lexeme) {
            return Err(ParseError {
                path: path.to_owned(),
                line: Some(token.line),
                column: Some(token.column),
                message: format!("Boon source exposes hidden runtime identity `{needle}`"),
            });
        }
    }
    for item in validation_index.semantic_items(ast) {
        work_batch.symbols(item.symbols.len());
        work_batch.validation();
        if item.field.as_deref() == Some("alive") {
            return Err(ParseError {
                path: path.to_owned(),
                line: Some(item.line),
                column: None,
                message: format!(
                    "Boon source exposes app-visible liveness field `alive` at line {}",
                    item.line
                ),
            });
        }
    }
    Ok(())
}

fn hidden_runtime_identity_token(value: &str) -> Option<&'static str> {
    if value
        .as_bytes()
        .windows(b"$boon".len())
        .any(|window| window.eq_ignore_ascii_case(b"$boon"))
    {
        return Some("$boon");
    }
    let tokens = value
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty());
    tokens.into_iter().find_map(|token| match token.len() {
        4 if token.eq_ignore_ascii_case("slot") => Some("slot"),
        7 if token.eq_ignore_ascii_case("row_key") => Some("row_key"),
        7 if token.eq_ignore_ascii_case("listkey") => Some("listkey"),
        8 if token.eq_ignore_ascii_case("item_key") => Some("item_key"),
        9 if token.eq_ignore_ascii_case("source_id") => Some("source_id"),
        10 if token.eq_ignore_ascii_case("hidden_key") => Some("hidden_key"),
        10 if token.eq_ignore_ascii_case("target_key") => Some("target_key"),
        10 if token.eq_ignore_ascii_case("bind_epoch") => Some("bind_epoch"),
        11 if token.eq_ignore_ascii_case("runtime_key") => Some("runtime_key"),
        11 if token.eq_ignore_ascii_case("hidden_keys") => Some("hidden_keys"),
        17 if token.eq_ignore_ascii_case("hidden_generation") => Some("hidden_generation"),
        17 if token.eq_ignore_ascii_case("target_generation") => Some("target_generation"),
        _ => None,
    })
}

fn expression_operators<'a>(symbols: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut operators = Vec::new();
    for lexeme in symbols {
        if is_operator_lexeme(lexeme) && !operators.iter().any(|operator| operator == lexeme) {
            operators.push(lexeme.to_owned());
        }
    }
    operators
}

fn is_operator_lexeme(lexeme: &str) -> bool {
    matches!(
        lexeme,
        "SOURCE"
            | "DRAIN"
            | "DRAINING"
            | "HOLD"
            | "THEN"
            | "WHEN"
            | "WHILE"
            | "LATEST"
            | "LIST"
            | "BLOCK"
            | "List/map"
            | "List/append"
            | "List/range"
            | "List/get"
            | "List/find"
            | "List/filter"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/chunk"
            | "List/remove"
            | "List/retain"
            | "List/count"
            | "List/length"
            | "List/sum"
            | "List/every"
            | "List/any"
            | "List/is_not_empty"
            | "List/latest"
            | "Dependency/catch_cycle"
            | "Text/empty"
            | "Text/join"
            | "Text/concat"
            | "Text/time_range_label"
            | "Text/trim"
            | "Text/to_lowercase"
            | "Text/to_uppercase"
            | "Text/slice"
            | "Text/length"
            | "Text/find"
            | "Text/contains"
            | "Text/starts_with"
            | "Text/all_chars_in"
            | "Text/to_number"
            | "Text/to_bytes"
            | "Text/is_empty"
            | "Text/is_not_empty"
            | "Bytes/length"
            | "Bytes/is_empty"
            | "Bytes/get"
            | "Bytes/set"
            | "Bytes/slice"
            | "Bytes/take"
            | "Bytes/drop"
            | "Bytes/concat"
            | "Bytes/equal"
            | "Bytes/find"
            | "Bytes/starts_with"
            | "Bytes/ends_with"
            | "Bytes/zeros"
            | "Bytes/to_text"
            | "Bytes/from_hex"
            | "Bytes/to_hex"
            | "Bytes/from_base64"
            | "Bytes/to_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "File/read_bytes"
            | "File/read_text"
            | "File/write_bytes"
            | "Number/bit_width"
            | "Number/to_text"
            | "Number/to_codepoint_text"
            | "Number/to_ascii_text"
            | "Number/interpolate"
            | "Number/project_width"
            | "Number/project_offset"
            | "Number/project_time"
            | "Bool/not"
            | "Bool/and"
            | "Bool/toggle"
            | "Timer/interval"
            | "Router/route"
            | "Router/go_to"
            | "Ulid/generate"
            | "Light/directional"
            | "Light/ambient"
            | "Light/spot"
    )
}

fn ast_token_for_parser_line_symbol<'a>(
    ast: &'a AstProgram,
    validation_index: &'a ValidationIndex,
    line: &ParserLine,
    lexeme_index: usize,
) -> Option<&'a AstToken> {
    validation_index
        .semantic_tokens_on_line(ast, line.line)
        .nth(lexeme_index)
}

fn collect_functions_with_work(
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Vec<String> {
    validation_index
        .semantic_items(ast)
        .filter_map(|item| {
            work.symbols(item.symbols.len());
            item.function.clone()
        })
        .collect()
}

fn build_unit_item_index(statements: &[AstStatement]) -> UnitItemIndex {
    fn header(
        statement: &AstStatement,
    ) -> Option<(UnitItemKind, Vec<String>, Vec<UnitItemParameter>)> {
        let (kind, names, parameters) = match &statement.kind {
            AstStatementKind::Function { name, parameters } => (
                UnitItemKind::Function,
                vec![name.clone()],
                parameters
                    .iter()
                    .map(|parameter| UnitItemParameter {
                        name: parameter.name.clone(),
                        kind: parameter.kind,
                        ordinal: parameter.ordinal,
                    })
                    .collect(),
            ),
            AstStatementKind::Field { name } => {
                (UnitItemKind::Field, vec![name.clone()], Vec::new())
            }
            AstStatementKind::Source { field, event } => (
                UnitItemKind::Source,
                field.iter().chain(event).cloned().collect(),
                Vec::new(),
            ),
            AstStatementKind::Hold { field, name } => (
                UnitItemKind::Hold,
                field.iter().chain(name).cloned().collect(),
                Vec::new(),
            ),
            AstStatementKind::List { field, .. } => (
                UnitItemKind::List,
                field.iter().cloned().collect(),
                Vec::new(),
            ),
            AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => {
                return None;
            }
        };
        Some((kind, names, parameters))
    }

    fn visit(
        statements: &[AstStatement],
        parent: &[StableItemRouteSegment],
        inside_definition: bool,
        matching_sibling_counts: &mut BTreeMap<(UnitItemKind, Vec<String>), usize>,
        entries: &mut Vec<UnitItemIndexEntry>,
    ) {
        for statement in statements {
            let Some((kind, names, parameters)) = header(statement) else {
                // Anonymous expression/block structure is intentionally not a
                // route segment. Declarations below it remain in the nearest
                // authored item scope, so body-only edits cannot rekey them.
                visit(
                    &statement.children,
                    parent,
                    inside_definition,
                    matching_sibling_counts,
                    entries,
                );
                continue;
            };
            if inside_definition && kind != UnitItemKind::Function {
                // Ordinary fields/sources/holds/lists inside a function are
                // implementation body statements, not item-tree interface
                // entries. Nested functions remain authored definitions.
                visit(
                    &statement.children,
                    parent,
                    true,
                    matching_sibling_counts,
                    entries,
                );
                continue;
            }
            let matching_sibling_ordinal = matching_sibling_counts
                .entry((kind, names.clone()))
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(0);
            let mut route = parent.to_vec();
            route.push(StableItemRouteSegment {
                kind,
                names: names.clone(),
                matching_sibling_ordinal: *matching_sibling_ordinal,
            });
            entries.push(UnitItemIndexEntry {
                route: StableItemRoute::__parser_from_segments(route.clone()),
                local_statement_id: statement.id,
                kind,
                names,
                parameters,
            });
            let mut child_counts = BTreeMap::new();
            visit(
                &statement.children,
                &route,
                inside_definition || kind == UnitItemKind::Function,
                &mut child_counts,
                entries,
            );
        }
    }

    let mut entries = Vec::new();
    let mut root_counts = BTreeMap::new();
    visit(statements, &[], false, &mut root_counts, &mut entries);
    UnitItemIndex { entries }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StableExpressionParent {
    expression: usize,
    segment: StableExpressionRouteSegment,
}

fn stable_statement_identity(statement: &AstStatement) -> (StableStatementKind, Vec<String>) {
    match &statement.kind {
        AstStatementKind::Function { name, .. } => {
            (StableStatementKind::Function, vec![name.clone()])
        }
        AstStatementKind::Field { name } => (StableStatementKind::Field, vec![name.clone()]),
        AstStatementKind::Source { field, event } => (
            StableStatementKind::Source,
            field.iter().chain(event).cloned().collect(),
        ),
        AstStatementKind::Hold { field, name } => (
            StableStatementKind::Hold,
            field.iter().chain(name).cloned().collect(),
        ),
        AstStatementKind::List { field, .. } => {
            (StableStatementKind::List, field.iter().cloned().collect())
        }
        AstStatementKind::Block => (StableStatementKind::Block, Vec::new()),
        AstStatementKind::Spread => (StableStatementKind::Spread, Vec::new()),
        AstStatementKind::Expression => (StableStatementKind::Expression, Vec::new()),
    }
}

/// Canonical stable child roles for one normalized expression kind.
///
/// Parser indexing and downstream artifact validation share this projection so
/// role labels and reverse ordinals cannot drift into two structural truths.
pub fn stable_expression_child_edges(
    kind: &AstExprKind,
    linked_input: Option<usize>,
) -> Vec<(usize, StableExpressionRouteSegment)> {
    fn fixed(
        edges: &mut Vec<(usize, StableExpressionRouteSegment)>,
        expression: usize,
        role: StableExpressionChildRole,
    ) {
        edges.push((
            expression,
            StableExpressionRouteSegment {
                role,
                label: None,
                matching_sibling_reverse_ordinal: 0,
            },
        ));
    }

    fn indexed(
        edges: &mut Vec<(usize, StableExpressionRouteSegment)>,
        role: StableExpressionChildRole,
        expressions: impl IntoIterator<Item = usize>,
    ) {
        let expressions = expressions.into_iter().collect::<Vec<_>>();
        let count = expressions.len();
        edges.extend(
            expressions
                .into_iter()
                .enumerate()
                .map(|(index, expression)| {
                    (
                        expression,
                        StableExpressionRouteSegment {
                            role,
                            label: None,
                            matching_sibling_reverse_ordinal: count - index - 1,
                        },
                    )
                }),
        );
    }

    fn labeled(
        edges: &mut Vec<(usize, StableExpressionRouteSegment)>,
        role: StableExpressionChildRole,
        expressions: impl IntoIterator<Item = (usize, String)>,
    ) {
        let expressions = expressions.into_iter().collect::<Vec<_>>();
        let mut remaining = BTreeMap::<String, usize>::new();
        for (_, label) in &expressions {
            *remaining.entry(label.clone()).or_default() += 1;
        }
        for (expression, label) in expressions {
            let count = remaining
                .get_mut(&label)
                .expect("labeled expression was counted");
            *count -= 1;
            edges.push((
                expression,
                StableExpressionRouteSegment {
                    role,
                    label: Some(label),
                    matching_sibling_reverse_ordinal: *count,
                },
            ));
        }
    }

    let mut edges = Vec::new();
    match kind {
        AstExprKind::TextTemplate { segments } => indexed(
            &mut edges,
            StableExpressionChildRole::TextDynamic,
            segments.iter().filter_map(|segment| match segment {
                AstTextSegment::Dynamic { value } => Some(*value),
                AstTextSegment::Static { .. } => None,
            }),
        ),
        AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => labeled(
            &mut edges,
            StableExpressionChildRole::RecordField,
            fields.iter().map(|field| (field.value, field.name.clone())),
        ),
        AstExprKind::Flush { payload } => {
            if let Some(payload) = payload {
                fixed(
                    &mut edges,
                    *payload,
                    StableExpressionChildRole::FlushPayload,
                );
            }
        }
        AstExprKind::Call { args, pass, .. } => {
            labeled(
                &mut edges,
                StableExpressionChildRole::CallArgument,
                args.iter().map(|argument| {
                    let kind = match argument.kind {
                        AstCallArgKind::BareBinding => "bare",
                        AstCallArgKind::Named => "named",
                    };
                    (argument.value, format!("{kind}:{}", argument.name))
                }),
            );
            if let Some(pass) = pass {
                fixed(&mut edges, pass.value, StableExpressionChildRole::CallPass);
            }
        }
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => {
            fixed(
                &mut edges,
                linked_input.unwrap_or(*input),
                StableExpressionChildRole::PipeInput,
            );
            labeled(
                &mut edges,
                StableExpressionChildRole::PipeArgument,
                args.iter().map(|argument| {
                    let kind = match argument.kind {
                        AstCallArgKind::BareBinding => "bare",
                        AstCallArgKind::Named => "named",
                    };
                    (argument.value, format!("{kind}:{}", argument.name))
                }),
            );
            if let Some(pass) = pass {
                fixed(&mut edges, pass.value, StableExpressionChildRole::PipePass);
            }
            indexed(
                &mut edges,
                StableExpressionChildRole::PipeArm,
                arms.iter().copied(),
            );
        }
        AstExprKind::Draining { input } => fixed(
            &mut edges,
            linked_input.unwrap_or(*input),
            StableExpressionChildRole::DrainingInput,
        ),
        AstExprKind::Hold { initial, .. } => fixed(
            &mut edges,
            linked_input.unwrap_or(*initial),
            StableExpressionChildRole::HoldInitial,
        ),
        AstExprKind::Latest { branches } => indexed(
            &mut edges,
            StableExpressionChildRole::LatestBranch,
            branches.iter().copied(),
        ),
        AstExprKind::When { input, arms } => {
            fixed(
                &mut edges,
                linked_input.unwrap_or(*input),
                StableExpressionChildRole::WhenInput,
            );
            indexed(
                &mut edges,
                StableExpressionChildRole::WhenArm,
                arms.iter().copied(),
            );
        }
        AstExprKind::Then { input, output } => {
            fixed(
                &mut edges,
                linked_input.unwrap_or(*input),
                StableExpressionChildRole::ThenInput,
            );
            if let Some(output) = output {
                fixed(&mut edges, *output, StableExpressionChildRole::ThenOutput);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            fixed(
                &mut edges,
                linked_input.unwrap_or(*left),
                StableExpressionChildRole::InfixLeft,
            );
            fixed(&mut edges, *right, StableExpressionChildRole::InfixRight);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                fixed(&mut edges, *output, StableExpressionChildRole::MatchOutput);
            }
        }
        AstExprKind::Block { bindings, result } => {
            labeled(
                &mut edges,
                StableExpressionChildRole::BlockBinding,
                bindings
                    .iter()
                    .map(|binding| (binding.value, binding.name.clone())),
            );
            if let Some(result) = result {
                fixed(&mut edges, *result, StableExpressionChildRole::BlockResult);
            }
        }
        AstExprKind::ListLiteral { items, .. }
        | AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => indexed(
            &mut edges,
            StableExpressionChildRole::CollectionItem,
            items.iter().copied(),
        ),
        AstExprKind::Arrow { left, output, .. } => {
            fixed(&mut edges, *left, StableExpressionChildRole::ArrowLeft);
            if let Some(output) = output {
                fixed(&mut edges, *output, StableExpressionChildRole::ArrowOutput);
            }
        }
        AstExprKind::MapEntry { key, value } => {
            fixed(&mut edges, *key, StableExpressionChildRole::MapKey);
            fixed(&mut edges, *value, StableExpressionChildRole::MapValue);
        }
        AstExprKind::MapLiteral { entries } => indexed(
            &mut edges,
            StableExpressionChildRole::MapEntry,
            entries.iter().copied(),
        ),
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
    edges
}

const STABLE_EXPRESSION_ROOT_DOMAIN_V1: &[u8] = b"boon.stable-expression-root.v1\0";
const STABLE_EXPRESSION_CHILD_DOMAIN_V1: &[u8] = b"boon.stable-expression-child.v1\0";

struct UnitExpressionIdentities {
    owner_index: UnitOwnerIndex,
    occurrence_routes: Vec<Option<StableOccurrenceRoute>>,
    expression_route_digests_v1: Vec<Option<[u8; 32]>>,
}

fn stable_identity_count(hasher: &mut Sha256, value: usize) {
    hasher.update((value as u64).to_le_bytes());
}

fn stable_identity_text(hasher: &mut Sha256, value: &str) {
    stable_identity_count(hasher, value.len());
    hasher.update(value.as_bytes());
}

fn stable_identity_names(hasher: &mut Sha256, names: &[String]) {
    stable_identity_count(hasher, names.len());
    for name in names {
        stable_identity_text(hasher, name);
    }
}

fn stable_item_kind_tag(kind: UnitItemKind) -> u8 {
    match kind {
        UnitItemKind::Function => 0,
        UnitItemKind::Field => 1,
        UnitItemKind::Source => 2,
        UnitItemKind::Hold => 3,
        UnitItemKind::List => 4,
    }
}

fn stable_statement_kind_tag(kind: StableStatementKind) -> u8 {
    match kind {
        StableStatementKind::Function => 0,
        StableStatementKind::Field => 1,
        StableStatementKind::Source => 2,
        StableStatementKind::Hold => 3,
        StableStatementKind::List => 4,
        StableStatementKind::Block => 5,
        StableStatementKind::Spread => 6,
        StableStatementKind::Expression => 7,
    }
}

fn stable_expression_role_tag(role: StableExpressionChildRole) -> u8 {
    match role {
        StableExpressionChildRole::TextDynamic => 0,
        StableExpressionChildRole::RecordField => 1,
        StableExpressionChildRole::FlushPayload => 2,
        StableExpressionChildRole::CallArgument => 3,
        StableExpressionChildRole::CallPass => 4,
        StableExpressionChildRole::PipeInput => 5,
        StableExpressionChildRole::PipeArgument => 6,
        StableExpressionChildRole::PipePass => 7,
        StableExpressionChildRole::PipeArm => 8,
        StableExpressionChildRole::DrainingInput => 9,
        StableExpressionChildRole::HoldInitial => 10,
        StableExpressionChildRole::LatestBranch => 11,
        StableExpressionChildRole::WhenInput => 12,
        StableExpressionChildRole::WhenArm => 13,
        StableExpressionChildRole::ThenInput => 14,
        StableExpressionChildRole::ThenOutput => 15,
        StableExpressionChildRole::InfixLeft => 16,
        StableExpressionChildRole::InfixRight => 17,
        StableExpressionChildRole::MatchOutput => 18,
        StableExpressionChildRole::BlockBinding => 19,
        StableExpressionChildRole::BlockResult => 20,
        StableExpressionChildRole::CollectionItem => 21,
        StableExpressionChildRole::ArrowLeft => 22,
        StableExpressionChildRole::ArrowOutput => 23,
        StableExpressionChildRole::MapKey => 24,
        StableExpressionChildRole::MapValue => 25,
        StableExpressionChildRole::MapEntry => 26,
    }
}

fn stable_expression_root_digest(route: &StableOccurrenceRoute) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(STABLE_EXPRESSION_ROOT_DOMAIN_V1);
    if let Some(owner) = &route.owner {
        hasher.update([1]);
        stable_identity_count(&mut hasher, owner.segments().len());
        for segment in owner.segments() {
            hasher.update([stable_item_kind_tag(segment.kind)]);
            stable_identity_names(&mut hasher, &segment.names);
            stable_identity_count(&mut hasher, segment.matching_sibling_ordinal);
        }
    } else {
        hasher.update([0]);
    }
    stable_identity_count(&mut hasher, route.statement_route.len());
    for segment in &route.statement_route {
        hasher.update([stable_statement_kind_tag(segment.kind)]);
        stable_identity_names(&mut hasher, &segment.names);
        stable_identity_count(&mut hasher, segment.matching_sibling_reverse_ordinal);
    }
    hasher.finalize().into()
}

fn stable_expression_child_digest(
    parent: [u8; 32],
    segment: &StableExpressionRouteSegment,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(STABLE_EXPRESSION_CHILD_DOMAIN_V1);
    hasher.update(parent);
    hasher.update([stable_expression_role_tag(segment.role)]);
    if let Some(label) = segment.label.as_deref() {
        hasher.update([1]);
        stable_identity_text(&mut hasher, label);
    } else {
        hasher.update([0]);
    }
    stable_identity_count(&mut hasher, segment.matching_sibling_reverse_ordinal);
    hasher.finalize().into()
}

fn stable_expression_route_digests(
    path: &str,
    parents: &[Option<StableExpressionParent>],
    roots: &[Option<StableOccurrenceRoute>],
) -> Result<Vec<Option<[u8; 32]>>, ParseError> {
    let expression_count = parents.len();
    let mut digests = vec![None; expression_count];
    let mut rootless = vec![false; expression_count];
    for expression in 0..expression_count {
        if digests[expression].is_some() || rootless[expression] {
            continue;
        }
        let mut cursor = expression;
        let mut chain = Vec::new();
        let mut has_root = true;
        loop {
            if digests[cursor].is_some() {
                break;
            }
            if rootless[cursor] {
                has_root = false;
                break;
            }
            if let Some(root) = roots.get(cursor).and_then(Option::as_ref) {
                digests[cursor] = Some(stable_expression_root_digest(root));
                break;
            }
            let Some(parent) = parents.get(cursor).and_then(Option::as_ref) else {
                rootless[cursor] = true;
                has_root = false;
                break;
            };
            chain.push((cursor, parent.expression, parent.segment.clone()));
            cursor = parent.expression;
            if chain.len() > expression_count {
                return Err(parsed_source_unit_invariant_error(
                    path,
                    format!("expression {expression} has a cyclic structural route"),
                ));
            }
        }
        if !has_root {
            for (child, _, _) in chain {
                rootless[child] = true;
            }
            continue;
        }
        for (child, parent, segment) in chain.into_iter().rev() {
            let parent = digests
                .get(parent)
                .and_then(|digest| *digest)
                .expect("stable expression parent is hashed before its child");
            digests[child] = Some(stable_expression_child_digest(parent, &segment));
        }
    }

    let mut identities = BTreeMap::new();
    for (expression, digest) in digests.iter().enumerate() {
        let Some(digest) = digest else {
            continue;
        };
        if let Some(previous) = identities.insert(*digest, expression) {
            return Err(parsed_source_unit_invariant_error(
                path,
                format!(
                    "stable expression route digest collision between expressions {previous} and {expression}"
                ),
            ));
        }
    }
    Ok(digests)
}

fn stable_expression_owner_slots(
    path: &str,
    parents: &[Option<StableExpressionParent>],
    root_owners: &[UnitCheckOwnerSlot],
) -> Result<Vec<UnitCheckOwnerSlot>, ParseError> {
    if parents.len() != root_owners.len() {
        return Err(parsed_source_unit_invariant_error(
            path,
            "expression parent and root-owner columns have different lengths",
        ));
    }
    let expression_count = parents.len();
    let mut owners = vec![UnitCheckOwnerSlot::__parser_unrouted(); expression_count];
    let mut chain = Vec::new();
    for expression in 0..expression_count {
        if owners[expression].is_routed() {
            continue;
        }
        chain.clear();
        let mut cursor = expression;
        let owner = loop {
            if let Some(owner) = owners
                .get(cursor)
                .copied()
                .filter(|owner| owner.is_routed())
            {
                break Some(owner);
            }
            if let Some(owner) = root_owners
                .get(cursor)
                .copied()
                .filter(|owner| owner.is_routed())
            {
                break Some(owner);
            }
            let Some(parent) = parents.get(cursor).and_then(Option::as_ref) else {
                break None;
            };
            chain.push(cursor);
            cursor = parent.expression;
            if chain.len() > expression_count {
                return Err(parsed_source_unit_invariant_error(
                    path,
                    format!("expression {expression} has a cyclic owner route"),
                ));
            }
        };
        let Some(owner) = owner else {
            continue;
        };
        owners[cursor] = owner;
        owners[expression] = owner;
        for child in chain.drain(..) {
            owners[child] = owner;
        }
    }
    Ok(owners)
}

fn build_unit_expression_identities(
    path: &str,
    ast: &AstProgram,
    item_index: &UnitItemIndex,
) -> Result<UnitExpressionIdentities, ParseError> {
    let expression_count = ast.expressions.len();
    let mut item_routes = BTreeMap::new();
    for (item, entry) in item_index.entries.iter().enumerate() {
        let slot = UnitCheckOwnerSlot::__parser_item(item).ok_or_else(|| {
            parsed_source_unit_invariant_error(path, "unit owner count exceeds u32")
        })?;
        if item_routes
            .insert(entry.local_statement_id, (entry.route.clone(), slot))
            .is_some()
        {
            return Err(parsed_source_unit_invariant_error(
                path,
                format!(
                    "statement {} appears more than once in the unit item index",
                    entry.local_statement_id
                ),
            ));
        }
    }
    let owner_routes = std::iter::once(UnitOwnerRoute::UnitRoot)
        .chain(
            item_index
                .entries
                .iter()
                .map(|entry| UnitOwnerRoute::Item(entry.route.clone())),
        )
        .collect::<Vec<_>>();
    let mut owner_statements = vec![Vec::new(); owner_routes.len()];
    let mut owner_child_owners = vec![Vec::new(); owner_routes.len()];
    let mut statement_locators = Vec::<Option<UnitStatementLocator>>::new();
    let mut statement_routes = Vec::<Option<StableStatementRoute>>::new();
    let mut statement_owners = Vec::<UnitCheckOwnerSlot>::new();
    let mut parents = vec![None::<StableExpressionParent>; expression_count];
    let mut roots = vec![None::<StableOccurrenceRoute>; expression_count];
    let mut root_owners = vec![UnitCheckOwnerSlot::__parser_unrouted(); expression_count];
    let mut visited = vec![false; expression_count];

    fn register_expression_tree(
        path: &str,
        root: usize,
        base: StableOccurrenceRoute,
        owner: UnitCheckOwnerSlot,
        ast: &AstProgram,
        parents: &mut [Option<StableExpressionParent>],
        roots: &mut [Option<StableOccurrenceRoute>],
        root_owners: &mut [UnitCheckOwnerSlot],
        visited: &mut [bool],
    ) -> Result<(), ParseError> {
        if !owner.is_routed() {
            return Err(parsed_source_unit_invariant_error(
                path,
                "statement expression has no check owner",
            ));
        }
        let root_slot = roots.get_mut(root).ok_or_else(|| {
            parsed_source_unit_invariant_error(
                path,
                format!("statement expression root {root} is out of range"),
            )
        })?;
        if root_slot.is_none() {
            *root_slot = Some(base);
            *root_owners.get_mut(root).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    path,
                    format!("statement expression root owner {root} is out of range"),
                )
            })? = owner;
        }
        let mut pending = vec![root];
        while let Some(parent) = pending.pop() {
            let was_visited = visited.get_mut(parent).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    path,
                    format!("expression parent {parent} is out of range"),
                )
            })?;
            if *was_visited {
                continue;
            }
            *was_visited = true;
            let expression = ast.expressions.get(parent).ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    path,
                    format!("expression parent {parent} is absent"),
                )
            })?;
            let children = stable_expression_child_edges(&expression.kind, expression.linked_input);
            for (child, segment) in children.into_iter().rev() {
                let child_parent = parents.get_mut(child).ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        path,
                        format!("expression child {child} is out of range"),
                    )
                })?;
                let candidate = StableExpressionParent {
                    expression: parent,
                    segment,
                };
                if child_parent.is_none() {
                    *child_parent = Some(candidate);
                    pending.push(child);
                }
            }
        }
        Ok(())
    }

    struct StatementOccurrenceBuilder<'a> {
        path: &'a str,
        ast: &'a AstProgram,
        item_routes: &'a BTreeMap<usize, (StableItemRoute, UnitCheckOwnerSlot)>,
        owner_statements: &'a mut [Vec<UnitLocalStatementId>],
        owner_child_owners: &'a mut [Vec<UnitChildOwnerBoundary>],
        statement_locators: &'a mut Vec<Option<UnitStatementLocator>>,
        statement_routes: &'a mut Vec<Option<StableStatementRoute>>,
        statement_owners: &'a mut Vec<UnitCheckOwnerSlot>,
        parents: &'a mut [Option<StableExpressionParent>],
        roots: &'a mut [Option<StableOccurrenceRoute>],
        root_owners: &'a mut [UnitCheckOwnerSlot],
        visited: &'a mut [bool],
    }

    impl StatementOccurrenceBuilder<'_> {
        fn visit(
            &mut self,
            statements: &[AstStatement],
            owner: Option<StableItemRoute>,
            owner_slot: UnitCheckOwnerSlot,
            parent_route: &[StableStatementRouteSegment],
            parent_statement: Option<UnitLocalStatementId>,
        ) -> Result<(), ParseError> {
            let identities = statements
                .iter()
                .map(stable_statement_identity)
                .collect::<Vec<_>>();
            let mut remaining = BTreeMap::<(StableStatementKind, Vec<String>), usize>::new();
            for identity in &identities {
                *remaining.entry(identity.clone()).or_default() += 1;
            }
            for (child_index, (statement, (kind, names))) in
                statements.iter().zip(identities).enumerate()
            {
                let local_statement =
                    UnitLocalStatementId::__parser_new(statement.id).ok_or_else(|| {
                        parsed_source_unit_invariant_error(
                            self.path,
                            format!("statement {} exceeds the unit-local id bound", statement.id),
                        )
                    })?;
                if self.statement_locators.len() <= statement.id {
                    self.statement_locators.resize(statement.id + 1, None);
                    self.statement_routes.resize(statement.id + 1, None);
                    self.statement_owners
                        .resize(statement.id + 1, UnitCheckOwnerSlot::__parser_unrouted());
                }
                let locator = UnitStatementLocator::__parser_new(parent_statement, child_index)
                    .ok_or_else(|| {
                        parsed_source_unit_invariant_error(
                            self.path,
                            format!(
                                "statement {} child index exceeds the unit-local bound",
                                statement.id
                            ),
                        )
                    })?;
                if self.statement_locators[statement.id]
                    .replace(locator)
                    .is_some()
                {
                    return Err(parsed_source_unit_invariant_error(
                        self.path,
                        format!("statement {} has more than one AST location", statement.id),
                    ));
                }
                let count = remaining
                    .get_mut(&(kind, names.clone()))
                    .expect("statement identity was counted");
                *count -= 1;
                let (statement_owner, statement_owner_slot, statement_route) = if let Some((
                    item_route,
                    item_slot,
                )) =
                    self.item_routes.get(&statement.id)
                {
                    let parent_entry = owner_slot.owner_entry_index().ok_or_else(|| {
                        parsed_source_unit_invariant_error(
                            self.path,
                            "nested item has no parent owner entry",
                        )
                    })?;
                    let boundary_expression = statement
                            .expr
                            .map(|expression| {
                                UnitLocalExpressionId::__parser_new(expression).ok_or_else(|| {
                                    parsed_source_unit_invariant_error(
                                        self.path,
                                        format!(
                                            "child owner boundary expression {expression} exceeds the unit-local bound"
                                        ),
                                    )
                                })
                            })
                            .transpose()?;
                    let result_expression = statement_value_expression(
                            statement,
                            &self.ast.expressions,
                        )
                        .map(|expression| {
                            UnitLocalExpressionId::__parser_new(expression).ok_or_else(|| {
                                parsed_source_unit_invariant_error(
                                    self.path,
                                    format!(
                                        "child owner result expression {expression} exceeds the unit-local bound"
                                    ),
                                )
                            })
                        })
                        .transpose()?;
                    let boundary = UnitChildOwnerBoundary::__parser_new(
                        item_route.clone(),
                        local_statement,
                        parent_statement,
                        child_index,
                        boundary_expression,
                        result_expression,
                    )
                    .ok_or_else(|| {
                        parsed_source_unit_invariant_error(
                            self.path,
                            format!(
                                "child owner at statement {} exceeds the unit-local bound",
                                statement.id
                            ),
                        )
                    })?;
                    self.owner_child_owners[parent_entry].push(boundary);
                    (Some(item_route.clone()), *item_slot, Vec::new())
                } else {
                    let mut route = parent_route.to_vec();
                    route.push(StableStatementRouteSegment {
                        kind,
                        names,
                        matching_sibling_reverse_ordinal: *count,
                    });
                    (owner.clone(), owner_slot, route)
                };
                let stable_statement_route = StableStatementRoute {
                    owner: statement_owner.clone(),
                    statement_route: statement_route.clone(),
                };
                if self.statement_routes[statement.id]
                    .replace(stable_statement_route)
                    .is_some()
                {
                    return Err(parsed_source_unit_invariant_error(
                        self.path,
                        format!("statement {} has more than one stable route", statement.id),
                    ));
                }
                let owner_entry = statement_owner_slot.owner_entry_index().ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        self.path,
                        format!("statement {} has no owner entry", statement.id),
                    )
                })?;
                self.statement_owners[statement.id] = statement_owner_slot;
                self.owner_statements[owner_entry].push(local_statement);
                if let Some(expression) = statement.expr {
                    register_expression_tree(
                        self.path,
                        expression,
                        StableOccurrenceRoute {
                            owner: statement_owner.clone(),
                            statement_route: statement_route.clone(),
                            expression_route: Vec::new(),
                        },
                        statement_owner_slot,
                        self.ast,
                        self.parents,
                        self.roots,
                        self.root_owners,
                        self.visited,
                    )?;
                }
                self.visit(
                    &statement.children,
                    statement_owner,
                    statement_owner_slot,
                    &statement_route,
                    Some(local_statement),
                )?;
            }
            Ok(())
        }
    }

    StatementOccurrenceBuilder {
        path,
        ast,
        item_routes: &item_routes,
        owner_statements: &mut owner_statements,
        owner_child_owners: &mut owner_child_owners,
        statement_locators: &mut statement_locators,
        statement_routes: &mut statement_routes,
        statement_owners: &mut statement_owners,
        parents: &mut parents,
        roots: &mut roots,
        root_owners: &mut root_owners,
        visited: &mut visited,
    }
    .visit(
        &ast.statements,
        None,
        UnitCheckOwnerSlot::__parser_unit_root(),
        &[],
        None,
    )?;

    let statement_locators = statement_locators
        .into_iter()
        .enumerate()
        .map(|(statement, locator)| {
            locator.ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    path,
                    format!("statement {statement} has no direct AST locator"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let statement_routes = statement_routes
        .into_iter()
        .enumerate()
        .map(|(statement, route)| {
            route.ok_or_else(|| {
                parsed_source_unit_invariant_error(
                    path,
                    format!("statement {statement} has no stable route"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if statement_owners.iter().any(|owner| !owner.is_routed()) {
        return Err(parsed_source_unit_invariant_error(
            path,
            "one or more statements have no check owner",
        ));
    }

    let expression_route_digests_v1 = stable_expression_route_digests(path, &parents, &roots)?;
    let expression_owners = stable_expression_owner_slots(path, &parents, &root_owners)?;
    let mut owner_expressions = vec![Vec::new(); owner_routes.len()];
    for (expression, owner) in expression_owners.iter().copied().enumerate() {
        let Some(owner_entry) = owner.owner_entry_index() else {
            continue;
        };
        let expression = UnitLocalExpressionId::__parser_new(expression).ok_or_else(|| {
            parsed_source_unit_invariant_error(
                path,
                "expression count exceeds the unit-local id bound",
            )
        })?;
        owner_expressions[owner_entry].push(expression);
    }
    let owner_entries = owner_routes
        .into_iter()
        .zip(owner_statements)
        .zip(owner_expressions)
        .zip(owner_child_owners)
        .map(
            |(((route, statements), expressions), child_owners)| UnitOwnerIndexEntry {
                route,
                statements: statements.into_boxed_slice(),
                expressions: expressions.into_boxed_slice(),
                child_owners: child_owners.into_boxed_slice(),
            },
        )
        .collect::<Vec<_>>();
    let expression_parents = parents
        .iter()
        .map(|parent| {
            parent
                .as_ref()
                .map(|parent| {
                    let expression = UnitLocalExpressionId::__parser_new(parent.expression)
                        .ok_or_else(|| {
                            parsed_source_unit_invariant_error(
                                path,
                                "expression parent exceeds the unit-local id bound",
                            )
                        })?;
                    Ok(UnitExpressionParentEdge::__parser_new(
                        expression,
                        parent.segment.clone(),
                    ))
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let owner_index = UnitOwnerIndex::__parser_new(
        owner_entries,
        statement_locators,
        statement_routes,
        statement_owners,
        expression_owners,
        expression_parents,
    );
    let mut occurrence_routes = vec![None; expression_count];
    for expression in &ast.expressions {
        if !matches!(
            expression.kind,
            AstExprKind::Call { .. } | AstExprKind::Pipe { .. }
        ) {
            continue;
        }
        let mut cursor = expression.id;
        let mut expression_route = Vec::new();
        let mut traversed = 0usize;
        let mut base = loop {
            if let Some(base) = roots.get(cursor).and_then(Option::clone) {
                break base;
            }
            let parent = parents
                .get(cursor)
                .and_then(Option::as_ref)
                .ok_or_else(|| {
                    parsed_source_unit_invariant_error(
                        path,
                        format!(
                            "call/pipe expression {} has no structural root",
                            expression.id
                        ),
                    )
                })?;
            expression_route.push(parent.segment.clone());
            cursor = parent.expression;
            traversed = traversed.saturating_add(1);
            if traversed > expression_count {
                return Err(parsed_source_unit_invariant_error(
                    path,
                    format!("call/pipe expression {} route is cyclic", expression.id),
                ));
            }
        };
        expression_route.reverse();
        base.expression_route = expression_route;
        occurrence_routes[expression.id] = Some(base);
    }
    Ok(UnitExpressionIdentities {
        owner_index,
        occurrence_routes,
        expression_route_digests_v1,
    })
}

#[cfg(test)]
fn collect_raw_declared_functions(statements: &[AstStatement]) -> Vec<String> {
    collect_raw_declared_functions_with_work(statements, &ParseWorkRecorder::disabled())
}

fn collect_raw_declared_functions_with_work(
    statements: &[AstStatement],
    work: &ParseWorkRecorder,
) -> Vec<String> {
    fn visit(statements: &[AstStatement], functions: &mut Vec<String>, work: &ParseWorkRecorder) {
        for statement in statements {
            work.statement();
            if let AstStatementKind::Function { name, .. } = &statement.kind {
                functions.push(name.clone());
            }
            visit(&statement.children, functions, work);
        }
    }

    let mut functions = Vec::new();
    visit(statements, &mut functions, work);
    functions
}

fn collect_operators_with_work(
    ast: &AstProgram,
    validation_index: &ValidationIndex,
    work: &ParseWorkRecorder,
) -> Vec<String> {
    let mut operators = Vec::new();
    for token in validation_index.semantic_tokens(ast) {
        work.tokens(1);
        if is_operator_lexeme(&token.lexeme)
            && !operators.iter().any(|operator| operator == &token.lexeme)
        {
            operators.push(token.lexeme.clone());
        }
    }
    operators
}

fn error(path: &str, line: usize, column: usize, message: &str) -> ParseError {
    ParseError {
        path: path.to_owned(),
        line: Some(line),
        column: Some(column),
        message: format!("{message} at line {line}, column {column}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser_line_fixture(
        line: usize,
        indent: usize,
        symbols: &[&str],
        start: usize,
    ) -> ParserLine {
        let symbol_spans = (0..symbols.len())
            .map(|offset| (start + offset, start + offset + 1))
            .collect::<Vec<_>>();
        ParserLine {
            line,
            indent,
            symbols: symbols.iter().map(|symbol| (*symbol).to_owned()).collect(),
            symbol_spans,
            start,
            end: start + symbols.len(),
        }
    }

    #[test]
    fn unchanged_logical_line_and_parser_item_share_symbols() {
        let ast = parse_ast("shared-symbols.bn", "value: Text/new(value: \"hello\")\n")
            .expect("fixture parses");
        let line = ast
            .lines
            .iter()
            .find(|line| !line.symbols.is_empty())
            .expect("fixture has one logical line");
        let item = ast
            .items
            .iter()
            .find(|item| item.line == line.line)
            .expect("fixture has one parser item");
        assert!(boon_syntax::SharedParserSymbols::ptr_eq(
            &line.symbols,
            &item.symbols,
        ));
    }

    fn assert_assembled_references_are_dense_and_in_range(ast: &AstProgram) {
        fn statement_count(statements: &[AstStatement]) -> usize {
            statements
                .iter()
                .map(|statement| 1 + statement_count(&statement.children))
                .sum()
        }

        fn expression_id(id: usize, count: usize) {
            assert!(id < count, "expression id {id} is outside 0..{count}");
        }

        fn optional_expression_id(id: Option<usize>, count: usize) {
            if let Some(id) = id {
                expression_id(id, count);
            }
        }

        fn call_arg(arg: &AstCallArg, expression_count: usize) {
            expression_id(arg.value, expression_count);
        }

        fn pass_context(pass: &AstPassContext, expression_count: usize) {
            expression_id(pass.value, expression_count);
        }

        fn record_field(field: &AstRecordField, expression_count: usize) {
            expression_id(field.value, expression_count);
        }

        fn assert_statements(nodes: &[AstStatement], next_id: &mut usize, expression_count: usize) {
            for statement in nodes {
                assert_eq!(statement.id, *next_id);
                *next_id += 1;
                optional_expression_id(statement.expr, expression_count);
                assert_statements(&statement.children, next_id, expression_count);
            }
        }

        let expression_count = ast.expressions.len();
        let statement_count = statement_count(&ast.statements);
        let mut next_statement_id = 0usize;
        assert_statements(&ast.statements, &mut next_statement_id, expression_count);
        assert_eq!(next_statement_id, statement_count);

        for (expected_id, expression) in ast.expressions.iter().enumerate() {
            assert_eq!(expression.id, expected_id);
            optional_expression_id(expression.linked_input, expression_count);
            match &expression.kind {
                AstExprKind::TextTemplate { segments } => {
                    for segment in segments {
                        if let AstTextSegment::Dynamic { value } = segment {
                            expression_id(*value, expression_count);
                        }
                    }
                }
                AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
                    for field in fields {
                        record_field(field, expression_count);
                    }
                }
                AstExprKind::Flush { payload } => {
                    optional_expression_id(*payload, expression_count);
                }
                AstExprKind::Call { args, pass, .. } => {
                    for arg in args {
                        call_arg(arg, expression_count);
                    }
                    if let Some(pass) = pass {
                        pass_context(pass, expression_count);
                    }
                }
                AstExprKind::Pipe {
                    input,
                    args,
                    pass,
                    arms,
                    ..
                } => {
                    expression_id(*input, expression_count);
                    for arg in args {
                        call_arg(arg, expression_count);
                    }
                    if let Some(pass) = pass {
                        pass_context(pass, expression_count);
                    }
                    for arm in arms {
                        expression_id(*arm, expression_count);
                    }
                }
                AstExprKind::Draining { input } => expression_id(*input, expression_count),
                AstExprKind::Hold { initial, .. } => expression_id(*initial, expression_count),
                AstExprKind::Latest { branches } => {
                    for branch in branches {
                        expression_id(*branch, expression_count);
                    }
                }
                AstExprKind::When { input, arms } => {
                    expression_id(*input, expression_count);
                    for arm in arms {
                        expression_id(*arm, expression_count);
                    }
                }
                AstExprKind::Then { input, output } => {
                    expression_id(*input, expression_count);
                    optional_expression_id(*output, expression_count);
                }
                AstExprKind::Infix { left, right, .. } => {
                    expression_id(*left, expression_count);
                    expression_id(*right, expression_count);
                }
                AstExprKind::MatchArm { output, .. } => {
                    optional_expression_id(*output, expression_count);
                }
                AstExprKind::Block { bindings, result } => {
                    for binding in bindings {
                        let AstBlockBindingDeclaration::Local { statement } = binding.declaration
                        else {
                            panic!("raw parsed BLOCK binding must remain local");
                        };
                        assert!(statement < statement_count);
                        expression_id(binding.value, expression_count);
                    }
                    optional_expression_id(*result, expression_count);
                }
                AstExprKind::ListLiteral { items, .. }
                | AstExprKind::BytesLiteral { items, .. }
                | AstExprKind::SetLiteral { items } => {
                    for item in items {
                        expression_id(*item, expression_count);
                    }
                }
                AstExprKind::Arrow { left, output, .. } => {
                    expression_id(*left, expression_count);
                    optional_expression_id(*output, expression_count);
                }
                AstExprKind::MapEntry { key, value } => {
                    expression_id(*key, expression_count);
                    expression_id(*value, expression_count);
                }
                AstExprKind::MapLiteral { entries } => {
                    for entry in entries {
                        expression_id(*entry, expression_count);
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
        }
    }

    #[test]
    fn validation_index_preserves_semantic_views_and_exact_source_order() {
        let ast = parse_ast(
            "validation/index.bn",
            r#"
rows: LIST[4] {}
opcode: BITS[7] { 2u011_0011 }
payload: BYTES[2] { 16u01, 16u02 }
label: TEXT { DRAIN LIST[99] BITS[1] { 2u0 } }
document:
    hidden_rows: LIST[8] {}
"#,
        )
        .unwrap();
        let work = ParseWorkRecorder::enabled();
        let index = ValidationIndex::build(&ast, &work);

        let canonical_tokens = ast
            .semantic_tokens()
            .map(|token| (token.start, token.end, token.lexeme.as_str()))
            .collect::<Vec<_>>();
        let indexed_tokens = index
            .semantic_tokens(&ast)
            .map(|token| (token.start, token.end, token.lexeme.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(indexed_tokens, canonical_tokens);

        let canonical_items = ast
            .semantic_parser_items()
            .map(|item| (item.line, item.start, item.end))
            .collect::<Vec<_>>();
        let indexed_items = index
            .semantic_items(&ast)
            .map(|item| (item.line, item.start, item.end))
            .collect::<Vec<_>>();
        assert_eq!(indexed_items, canonical_items);

        for line in ast.semantic_parser_lines() {
            let canonical = ast
                .semantic_tokens()
                .filter(|token| token.line == line.line)
                .map(|token| token.start)
                .collect::<Vec<_>>();
            let indexed = index
                .semantic_tokens_on_line(&ast, line.line)
                .map(|token| token.start)
                .collect::<Vec<_>>();
            assert_eq!(indexed, canonical);
        }
        for token in &ast.tokens {
            assert_eq!(
                index
                    .token_at_start(&ast, token.start, &work)
                    .map(|found| found.start),
                Some(token.start)
            );
        }

        let canonical_list_sites = ast
            .semantic_parser_lines()
            .enumerate()
            .filter_map(|(_, line)| {
                line.symbols
                    .iter()
                    .position(|symbol| symbol == "LIST")
                    .map(|symbol| (line.line, symbol))
            })
            .collect::<Vec<_>>();
        let indexed_list_sites = index
            .list_sites
            .iter()
            .map(|(line_id, symbol)| (ast.lines[*line_id].line, *symbol))
            .collect::<Vec<_>>();
        assert_eq!(indexed_list_sites, canonical_list_sites);

        let boundary = ValidationIndex::build_boundary(&ast, &work);
        assert_eq!(boundary.semantic_items(&ast).count(), 1);
        assert!(boundary.semantic_token_ids.is_empty());
        assert!(boundary.list_sites.is_empty());
        assert!(work.finish().work_counters.validation_visits > 0);
    }

    #[test]
    fn profiled_source_unit_work_is_deterministic_and_tracks_failure_progress() {
        let source = "value: input |> Number/abs()\n";
        let (first, first_profile) =
            parse_source_unit_profiled("profile/unit.bn", source.to_owned());
        let (second, second_profile) =
            parse_source_unit_profiled("profile/unit.bn", source.to_owned());

        let first = first.unwrap();
        assert_eq!(first, second.unwrap());
        assert_eq!(first_profile, second_profile);
        let work = first_profile.work_counters;
        assert_eq!(work.source_units_attempted, 1);
        assert_eq!(work.source_units_parsed, 1);
        assert_eq!(work.source_units_reused, 0);
        assert!(work.source_bytes_inspected >= source.len());
        assert!(work.token_inspections > 0);
        assert!(work.symbol_inspections > 0);
        assert!(work.statement_visits > 0);
        assert!(work.expression_visits > 0);
        assert_eq!(work.nodes_rebased, 0);
        assert!(work.validation_visits > 0);
        assert_eq!(first, parse_source_unit("profile/unit.bn", source).unwrap());

        let (failed, failed_profile) =
            parse_source_unit_profiled("profile/broken.bn", "value: (1\n");
        assert!(failed.is_err());
        assert_eq!(failed_profile.work_counters.source_units_attempted, 1);
        assert_eq!(failed_profile.work_counters.source_units_parsed, 0);
        assert!(failed_profile.work_counters.source_bytes_inspected > 0);
    }

    #[test]
    fn validation_work_batch_flushes_exact_visits_on_scope_exit() {
        let work = ParseWorkRecorder::enabled();
        {
            let mut batch = work.batch();
            batch.tokens(7);
            batch.symbols(11);
            batch.statement();
            batch.statement();
            batch.expression();
            batch.validation();
            batch.validation();
            batch.validation();
        }
        let counters = work.finish().work_counters;
        assert_eq!(counters.token_inspections, 7);
        assert_eq!(counters.symbol_inspections, 11);
        assert_eq!(counters.statement_visits, 2);
        assert_eq!(counters.expression_visits, 1);
        assert_eq!(counters.validation_visits, 3);
    }

    #[test]
    fn profiled_project_work_scales_linearly_across_independent_units() {
        fn project(unit_count: usize) -> Vec<(String, String)> {
            let source = "FUNCTION identity(value) {\n    result: value\n}\n";
            (0..unit_count)
                .map(|index| {
                    let path = if index == 0 {
                        "RUN.bn".to_owned()
                    } else {
                        format!("M{index:02}.bn")
                    };
                    (path, source.to_owned())
                })
                .collect()
        }

        fn total(work: ParseWorkCounters) -> usize {
            work.source_units_attempted
                + work.source_units_parsed
                + work.source_bytes_inspected
                + work.token_inspections
                + work.symbol_inspections
                + work.statement_visits
                + work.expression_visits
                + work.nodes_rebased
                + work.validation_visits
        }

        let four_units = project(4);
        let expected_four = parse_project("RUN.bn", four_units.clone()).unwrap();
        let (four, four_profile) = parse_project_profiled("RUN.bn", four_units);
        let (eight, eight_profile) = parse_project_profiled("RUN.bn", project(8));
        assert_eq!(four.unwrap(), expected_four);
        eight.unwrap();

        let four = four_profile.work_counters;
        let eight = eight_profile.work_counters;
        assert_eq!(four.source_units_attempted, 4);
        assert_eq!(four.source_units_parsed, 4);
        assert_eq!(eight.source_units_attempted, 8);
        assert_eq!(eight.source_units_parsed, 8);
        assert_eq!(
            eight.source_bytes_inspected,
            four.source_bytes_inspected * 2
        );
        assert_eq!(eight.nodes_rebased, four.nodes_rebased * 2);
        assert!(four.nodes_rebased > 0);
        assert!(four.validation_visits > 0);

        let four_total = total(four);
        let eight_total = total(eight);
        assert!(eight_total >= four_total * 18 / 10);
        assert!(eight_total <= four_total * 22 / 10);
    }

    #[test]
    fn multiline_call_delimiter_scan_visits_each_nested_symbol_once() {
        let mut stack = vec![")"];
        let opening = ["[", "{"].map(str::to_owned);
        let body = ["value", ","].map(str::to_owned);
        let closing = ["}", "]", ")", "ignored", ")"].map(str::to_owned);
        let mut visits = 0usize;

        let (progress, inspected) = advance_delimiter_stack(&opening, 0, &mut stack);
        visits += inspected;
        assert_eq!(progress, DelimiterProgress::Open);
        for _ in 0..512 {
            let (progress, inspected) = advance_delimiter_stack(&body, 0, &mut stack);
            visits += inspected;
            assert_eq!(progress, DelimiterProgress::Open);
        }
        let (progress, inspected) = advance_delimiter_stack(&closing, 0, &mut stack);
        visits += inspected;

        assert_eq!(progress, DelimiterProgress::Closed);
        assert_eq!(visits, opening.len() + 512 * body.len() + 3);
        assert!(stack.is_empty());
    }

    #[test]
    fn multiline_call_merger_preserves_separators_spans_and_following_lines() {
        let lines = vec![
            parser_line_fixture(1, 0, &["value", ":", "call", "(", "first", ":", "1"], 0),
            parser_line_fixture(2, 4, &["nested", ":", "[", "2", "]"], 20),
            parser_line_fixture(3, 0, &[")", "trailing"], 40),
            parser_line_fixture(4, 0, &["after", ":", "3"], 60),
        ];

        let merged = merge_multiline_call_expression_lines(
            lines.clone(),
            &[],
            &ParseWorkRecorder::disabled(),
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].line, 1);
        assert_eq!(merged[0].indent, 0);
        assert_eq!(merged[0].start, 0);
        assert_eq!(merged[0].end, lines[2].end);
        assert_eq!(
            merged[0].symbols,
            [
                "value", ":", "call", "(", "first", ":", "1", ",", "nested", ":", "[", "2", "]",
                ")", "trailing",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        );
        assert_eq!(merged[0].symbol_spans[7], (7, 7));
        assert_eq!(merged[1], lines[3]);

        let mismatched = vec![
            parser_line_fixture(1, 0, &["value", ":", "call", "("], 0),
            parser_line_fixture(2, 4, &["wrong", "]"], 10),
            parser_line_fixture(3, 0, &["after", ":", "3"], 20),
        ];
        assert_eq!(
            merge_multiline_call_expression_lines(
                mismatched.clone(),
                &[],
                &ParseWorkRecorder::disabled(),
            ),
            mismatched
        );
        assert_eq!(
            merge_multiline_call_expression_lines(
                lines[..3].to_vec(),
                &[(2, 3)],
                &ParseWorkRecorder::disabled(),
            ),
            lines[..3]
        );
    }

    #[test]
    fn normalized_source_spans_preserve_exact_literal_boundaries() {
        let spans = normalized_source_spans(vec![(20, 30), (5, 12), (5, 10), (10, 18), (30, 40)]);
        assert_eq!(spans, vec![(5, 18), (20, 30), (30, 40)]);
        assert_eq!(containing_source_span(&spans, 6, 17), Some((5, 18)));
        assert_eq!(containing_source_span(&spans, 20, 30), Some((20, 30)));
        assert_eq!(containing_source_span(&spans, 30, 40), Some((30, 40)));
        assert_eq!(containing_source_span(&spans, 18, 20), None);
        assert_eq!(containing_source_span(&spans, 29, 31), None);
    }

    #[test]
    fn source_unit_identity_is_normalized_stable_and_not_bundle_positional() {
        let main = SourceUnitId::from_path("app\\main.bn").unwrap();
        let same_main = SourceUnitId::from_path("app/main.bn").unwrap();
        let helper = SourceUnitId::from_path("app/helper.bn").unwrap();
        assert_eq!(main, same_main);
        assert_ne!(main, helper);
        assert_eq!(main.as_str(), "app/main.bn");

        let before = parse_project(
            "app/main.bn",
            [
                ("app/main.bn".to_owned(), "value: 1\n".to_owned()),
                ("app/zebra.bn".to_owned(), "zebra: 2\n".to_owned()),
            ],
        )
        .unwrap();
        let after = parse_project(
            "app/main.bn",
            [
                ("app/alpha.bn".to_owned(), "alpha: 0\n".to_owned()),
                ("app/main.bn".to_owned(), "value: 3\n".to_owned()),
                ("app/zebra.bn".to_owned(), "zebra: 2\n".to_owned()),
            ],
        )
        .unwrap();

        for path in ["app/main.bn", "app/zebra.bn"] {
            let before_id = before
                .files
                .iter()
                .find(|file| file.path == path)
                .unwrap()
                .source_unit_id()
                .unwrap();
            let after_id = after
                .files
                .iter()
                .find(|file| file.path == path)
                .unwrap()
                .source_unit_id()
                .unwrap();
            assert_eq!(before_id, after_id);
        }

        assert!(SourceUnitId::from_path("/absolute/main.bn").is_err());
        assert!(
            !serde_json::to_string(&before)
                .unwrap()
                .contains("source_unit_id")
        );
    }

    #[test]
    fn project_syntax_keeps_unit_arenas_local_when_earlier_source_grows() {
        let files = [
            (
                "app/Math.bn".to_owned(),
                "FUNCTION double(input) {\n    input + input\n}\n".to_owned(),
            ),
            (
                "app/RUN.bn".to_owned(),
                "value: Math/double(input: 2)\n".to_owned(),
            ),
        ];
        let before = parse_project_syntax("app/RUN.bn", files.clone()).unwrap();
        let after = parse_project_syntax(
            "app/RUN.bn",
            [
                (
                    "app/000_notes.bn".to_owned(),
                    "note: Text/new(value: \"a much longer unrelated source unit\")\n".to_owned(),
                ),
                files[0].clone(),
                files[1].clone(),
            ],
        )
        .unwrap();

        let before_math = before
            .units()
            .iter()
            .find(|unit| unit.path == "app/Math.bn")
            .unwrap();
        let after_math = after
            .units()
            .iter()
            .find(|unit| unit.path == "app/Math.bn")
            .unwrap();
        assert_eq!(before_math.namespace(), after_math.namespace());
        assert_eq!(before_math.ast, after_math.ast);
        assert_eq!(before_math.item_index, after_math.item_index);

        let expression = before_math.ast.expressions.first().unwrap();
        let (namespace, local) = __parser_unpack_syntax_node_id(expression.id).unwrap();
        assert_eq!(namespace, before_math.namespace());
        assert_eq!(local, 0);
        assert!(before.expressions().get(expression.id).is_some());
        assert_eq!(
            expression.line,
            after_math.ast.expressions.first().unwrap().line,
            "unit AST locations stay local when an earlier unit is inserted",
        );

        let before_layout = before
            .source_layouts()
            .iter()
            .find(|layout| layout.path == "app/Math.bn")
            .unwrap();
        let after_layout = after
            .source_layouts()
            .iter()
            .find(|layout| layout.path == "app/Math.bn")
            .unwrap();
        assert_ne!(before_layout.start_line, after_layout.start_line);
        assert_ne!(before_layout.start_byte, after_layout.start_byte);
    }

    #[test]
    fn project_link_reuses_the_parser_unit_validation_seal() {
        let (parsed, parse_profile) = parse_project_source_unit_profiled(
            "app/Math.bn",
            "FUNCTION double(input) {\n    input + input\n}\n".to_owned(),
        );
        let parsed = parsed.unwrap();
        assert!(parse_profile.work_counters.validation_visits > 0);

        let link_key = project_unit_link_keys(
            "app/RUN.bn",
            [(
                parsed.source_unit_id.clone(),
                parsed.declared_functions.clone(),
            )],
        )
        .unwrap()
        .remove(&parsed.source_unit_id)
        .unwrap();
        let (linked, link_profile) = link_project_source_unit_profiled(parsed, link_key);
        let linked = linked.unwrap();

        assert_eq!(linked.module(), Some("Math"));
        assert_eq!(link_profile.work_counters.validation_visits, 0);
        assert_eq!(link_profile.work_counters.token_inspections, 0);
        assert!(link_profile.work_counters.statement_visits > 0);
        assert!(link_profile.work_counters.expression_visits > 0);
    }

    #[test]
    fn project_syntax_namespaces_module_headers_without_rekeying_definitions() {
        let project = parse_project_syntax(
            "app/RUN.bn",
            [
                (
                    "app/Math.bn".to_owned(),
                    "FUNCTION double(input) {\n    input + input\n}\nFUNCTION quad(input) {\n    double(input: double(input: input))\n}\n".to_owned(),
                ),
                (
                    "app/RUN.bn".to_owned(),
                    "value: Math/quad(input: 2)\n".to_owned(),
                ),
            ],
        )
        .unwrap();
        let math = project
            .units()
            .iter()
            .find(|unit| unit.path == "app/Math.bn")
            .unwrap();
        assert_eq!(math.module(), Some("Math"));
        let function_names = collect_raw_declared_functions(&math.ast.statements);
        assert_eq!(function_names, ["Math/double", "Math/quad"]);
        assert!(math.ast.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                AstExprKind::Call { function, .. } if function == "Math/double"
            )
        }));
        let definition_names = project
            .item_index()
            .definitions()
            .flat_map(|entry| entry.names.iter().map(String::as_str))
            .collect::<Vec<_>>();
        assert_eq!(definition_names, ["Math/double", "Math/quad"]);
        assert!(
            project
                .item_index()
                .definitions()
                .all(|entry| entry.definition_key.is_some())
        );
    }

    #[test]
    fn unit_item_index_keeps_definition_identity_across_body_and_unrelated_item_edits() {
        let before = parse_project_source_unit(
            "app/Math.bn",
            "FUNCTION helper(value) {\n    result: value\n}\n",
        )
        .unwrap();
        let after = parse_project_source_unit(
            "app/Math.bn",
            "unrelated: 42\nFUNCTION helper(value) {\n    result: value + 1\n}\n",
        )
        .unwrap();

        let before_keys = before.stable_definition_keys().collect::<Vec<_>>();
        let after_keys = after.stable_definition_keys().collect::<Vec<_>>();
        assert_eq!(before_keys, after_keys);
        assert_eq!(before_keys.len(), 1);
        let before_owner_keys = before.stable_owner_keys().collect::<BTreeSet<_>>();
        let after_owner_keys = after.stable_owner_keys().collect::<BTreeSet<_>>();
        assert!(before_owner_keys.is_subset(&after_owner_keys));
        assert_eq!(before_owner_keys.len(), 1);
        assert_eq!(after_owner_keys.len(), 2);
        assert_eq!(before.item_index.entries.len(), 1);
        assert_eq!(after.item_index.entries.len(), 2);

        let before_definition = before.item_index.definitions().next().unwrap();
        let after_definition = after.item_index.definitions().next().unwrap();
        assert_eq!(before_definition.route, after_definition.route);
        assert_ne!(
            before_definition.local_statement_id,
            after_definition.local_statement_id
        );
        assert_eq!(before_definition.parameters.len(), 1);
        assert_eq!(before_definition.parameters[0].name, "value");
    }

    #[test]
    fn nested_document_list_is_a_stable_executable_owner() {
        let parsed = parse_project_source_unit(
            "app/RUN.bn",
            "store: [\n    todos: LIST {\n        [title: TEXT { Read documentation }, completed: False]\n    }\n]\n",
        )
        .unwrap();
        let list = parsed
            .item_index
            .owners()
            .find(|entry| entry.kind == UnitItemKind::List && entry.names == ["todos"])
            .expect("nested authored list is indexed as an executable owner");
        let segments = list.route.segments();

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].kind, UnitItemKind::Field);
        assert_eq!(segments[0].names, ["store"]);
        assert_eq!(segments[1].kind, UnitItemKind::List);
        assert_eq!(segments[1].names, ["todos"]);
        assert!(parsed.stable_definition_keys().next().is_none());
        assert!(
            parsed
                .stable_owner_keys()
                .any(|key| key.item_route == list.route)
        );
    }

    #[test]
    fn unit_owner_index_partitions_root_items_and_function_local_statements() {
        let parsed = parse_project_source_unit(
            "app/RUN.bn",
            "helper(input: 0)\nvalue: 1\nstore: [\n    todos: LIST {\n        [title: TEXT { One }]\n    }\n]\nFUNCTION helper(input) {\n    local: input\n    local\n}\n",
        )
        .unwrap();

        let keys = parsed.stable_check_owner_keys().collect::<Vec<_>>();
        assert_eq!(keys.len(), parsed.item_index.entries.len() + 1);
        assert_eq!(
            keys.first(),
            Some(&StableCheckOwnerKey::UnitRoot(
                parsed.source_unit_id.clone()
            ))
        );

        let mut owned_statements = BTreeSet::new();
        let mut owned_expressions = BTreeSet::new();
        for entry in parsed.owner_index().entries() {
            let view = parsed.owner_view(&entry.route).unwrap();
            let owner = view.stable_key();
            for statement in view.statements() {
                assert_eq!(
                    parsed.stable_check_owner_for_local_statement(statement.id),
                    Some(owner.clone())
                );
                assert!(owned_statements.insert(statement.id));
            }
            for expression in view.expressions() {
                assert_eq!(
                    parsed.stable_check_owner_for_local_expression(expression.id),
                    Some(owner.clone())
                );
                assert!(owned_expressions.insert(expression.id));
            }
            assert_eq!(
                view.stable_expression_keys().collect::<BTreeSet<_>>().len(),
                view.expression_ids().len()
            );
            assert_eq!(
                view.statement_ids()
                    .iter()
                    .filter_map(|statement| view.stable_statement_key_local(*statement))
                    .collect::<BTreeSet<_>>()
                    .len(),
                view.statement_ids().len()
            );
        }

        fn collect_statement_ids(statements: &[AstStatement], ids: &mut BTreeSet<usize>) {
            for statement in statements {
                assert!(ids.insert(statement.id));
                collect_statement_ids(&statement.children, ids);
            }
        }
        let mut all_statements = BTreeSet::new();
        collect_statement_ids(&parsed.ast.statements, &mut all_statements);
        assert_eq!(owned_statements, all_statements);
        assert_eq!(
            owned_expressions,
            parsed
                .ast
                .expressions
                .iter()
                .filter(|expression| parsed.stable_expression_key(expression.id).is_some())
                .map(|expression| expression.id)
                .collect::<BTreeSet<_>>()
        );

        let root = parsed.owner_view(&UnitOwnerRoute::UnitRoot).unwrap();
        let root_child_names = root
            .child_owners()
            .iter()
            .filter_map(|boundary| boundary.route.segments().last()?.names.first().cloned())
            .collect::<Vec<_>>();
        assert_eq!(root_child_names, ["value", "store", "helper"]);

        let store = parsed
            .owner_index()
            .entries()
            .iter()
            .find(|entry| {
                matches!(
                    &entry.route,
                    UnitOwnerRoute::Item(route)
                        if route.segments().last().is_some_and(|segment| segment.names == ["store"])
                )
            })
            .unwrap();
        assert_eq!(store.child_owners.len(), 1);
        assert_eq!(
            store.child_owners[0].route.segments().last().unwrap().names,
            ["todos"]
        );

        let helper = keys
            .iter()
            .find(|key| {
                matches!(
                    key,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["helper"])
                )
            })
            .unwrap();
        let local_statement = parsed
            .ast
            .statements
            .iter()
            .find(|statement| {
                matches!(&statement.kind, AstStatementKind::Function { name, .. } if name == "helper")
            })
            .unwrap()
            .children
            .iter()
            .find(|statement| {
                matches!(&statement.kind, AstStatementKind::Field { name } if name == "local")
            })
            .unwrap();
        assert_eq!(
            parsed.stable_check_owner_for_local_statement(local_statement.id),
            Some(helper.clone())
        );
        assert!(!parsed.item_index.entries.iter().any(|entry| {
            entry
                .route
                .segments()
                .last()
                .is_some_and(|segment| segment.names == ["local"])
        }));
    }

    #[test]
    fn stable_statement_keys_ignore_payloads_spans_and_unrelated_siblings() {
        fn helper_keys(source: &str) -> Vec<StableStatementKey> {
            let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
            let helper = parsed
                .owner_index()
                .entries()
                .iter()
                .find(|entry| {
                    matches!(
                        &entry.route,
                        UnitOwnerRoute::Item(route)
                            if route.segments().last().is_some_and(|segment| segment.names == ["helper"])
                    )
                })
                .unwrap();
            let view = parsed.owner_view(&helper.route).unwrap();
            view.statement_ids()
                .iter()
                .map(|statement| view.stable_statement_key_local(*statement).unwrap())
                .collect()
        }

        let baseline = helper_keys("FUNCTION helper(input) {\n    local: input\n    local\n}\n");
        let edited = helper_keys(
            "earlier: 999\nFUNCTION helper(input) {\n    local : input + 0\n    local\n}\n",
        );
        assert_eq!(edited, baseline);
        assert_eq!(baseline.len(), 3);
    }

    #[test]
    fn structural_occurrence_keys_ignore_authored_text_and_unrelated_earlier_calls() {
        fn helper_keys(source: &str) -> BTreeMap<String, StableOccurrenceKey> {
            let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
            parsed
                .ast
                .expressions
                .iter()
                .filter(|expression| {
                    matches!(&expression.kind, AstExprKind::Call { function, .. } if function == "helper")
                })
                .filter_map(|expression| {
                    let key = parsed.stable_occurrence_key(expression.id)?;
                    let owner = key
                        .route
                        .owner
                        .as_ref()?
                        .segments()
                        .last()?
                        .names
                        .first()?
                        .clone();
                    ["left", "right"].contains(&owner.as_str()).then_some((owner, key))
                })
                .collect()
        }

        let baseline = helper_keys(
            "left: helper(value: 10)\nright: helper(value: 20)\n\nFUNCTION helper(value) {\n    value\n}\n",
        );
        let edited = helper_keys(
            "earlier: helper(value: 999)\nleft : helper(value: 11)\nright: helper(value: 21)\n\nFUNCTION helper(value) {\n    value + 0\n}\n",
        );

        assert_eq!(edited, baseline);
        assert_eq!(baseline.len(), 2);
    }

    #[test]
    fn stable_expression_keys_cover_every_reachable_expression_without_collisions() {
        let parsed = parse_project_source_unit(
            "app/RUN.bn",
            "FUNCTION helper(value) {\n    doubled: value + value\n    [doubled, value]\n}\nresult: helper(value: 10)\n",
        )
        .unwrap();

        fn visit_expression(
            expression: usize,
            parsed: &ParsedSourceUnit,
            seen: &mut BTreeSet<usize>,
        ) {
            if !seen.insert(expression) {
                return;
            }
            let expression = parsed
                .ast
                .expressions
                .get(expression)
                .expect("reachable expression is in the parser arena");
            for (child, _) in
                stable_expression_child_edges(&expression.kind, expression.linked_input)
            {
                visit_expression(child, parsed, seen);
            }
        }

        fn visit_statements(
            statements: &[AstStatement],
            parsed: &ParsedSourceUnit,
            seen: &mut BTreeSet<usize>,
        ) {
            for statement in statements {
                if let Some(expression) = statement.expr {
                    visit_expression(expression, parsed, seen);
                }
                visit_statements(&statement.children, parsed, seen);
            }
        }

        let mut reachable = BTreeSet::new();
        visit_statements(&parsed.ast.statements, &parsed, &mut reachable);

        let keys = parsed
            .ast
            .expressions
            .iter()
            .filter_map(|expression| parsed.stable_expression_key(expression.id))
            .collect::<BTreeSet<_>>();

        assert_eq!(keys.len(), reachable.len());
        for expression in &parsed.ast.expressions {
            assert_eq!(
                parsed.stable_expression_key(expression.id).is_some(),
                reachable.contains(&expression.id),
                "stable identities cover exactly the final semantic expression graph"
            );
        }
        assert!(
            keys.iter()
                .all(|key| key.source_unit_id == parsed.source_unit_id)
        );
    }

    #[test]
    fn stable_expression_keys_ignore_literal_text_and_unrelated_earlier_items() {
        let literal_before = parse_project_source_unit("app/RUN.bn", "value: 1\n").unwrap();
        let literal_after = parse_project_source_unit("app/RUN.bn", "value: 999\n").unwrap();
        assert_eq!(literal_before.ast.expressions.len(), 1);
        assert_eq!(literal_after.ast.expressions.len(), 1);
        assert_ne!(
            literal_before.ast.expressions[0].kind,
            literal_after.ast.expressions[0].kind
        );
        assert_eq!(
            literal_before.stable_expression_key(0),
            literal_after.stable_expression_key(0),
            "authored literal text is not syntax identity"
        );

        let function_before = parse_project_source_unit(
            "app/RUN.bn",
            "FUNCTION helper(value) {\n    result: value + 1\n    result\n}\n",
        )
        .unwrap();
        let function_after = parse_project_source_unit(
            "app/RUN.bn",
            "unrelated: [100, 200]\nFUNCTION helper(value) {\n    result: value + 1\n    result\n}\n",
        )
        .unwrap();
        let before_keys = function_before
            .ast
            .expressions
            .iter()
            .map(|expression| {
                function_before
                    .stable_expression_key(expression.id)
                    .unwrap()
            })
            .collect::<BTreeSet<_>>();
        let after_keys = function_after
            .ast
            .expressions
            .iter()
            .map(|expression| function_after.stable_expression_key(expression.id).unwrap())
            .collect::<BTreeSet<_>>();

        assert!(after_keys.len() > before_keys.len());
        assert!(before_keys.is_subset(&after_keys));
    }

    #[test]
    fn project_link_preserves_parser_expression_identity() {
        let parsed = parse_project_source_unit(
            "app/Math.bn",
            "FUNCTION double(input) {\n    input + input\n}\nFUNCTION quad(input) {\n    double(input: double(input: input))\n}\n",
        )
        .unwrap();
        let parser_keys = parsed
            .ast
            .expressions
            .iter()
            .map(|expression| parsed.stable_expression_key(expression.id).unwrap())
            .collect::<Vec<_>>();
        let parser_owner_partitions = parsed
            .owner_index()
            .entries()
            .iter()
            .map(|entry| {
                let view = parsed.owner_view(&entry.route).unwrap();
                (
                    view.stable_key(),
                    view.statement_ids()
                        .iter()
                        .map(|statement| view.stable_statement_key_local(*statement).unwrap())
                        .collect::<Vec<_>>(),
                    view.stable_expression_keys().collect::<Vec<_>>(),
                    view.child_owners().to_vec(),
                )
            })
            .collect::<Vec<_>>();
        let link_key = project_unit_link_keys(
            "app/RUN.bn",
            [(
                parsed.source_unit_id.clone(),
                parsed.declared_functions.clone(),
            )],
        )
        .unwrap()
        .remove(&parsed.source_unit_id)
        .unwrap();
        let linked = parsed.into_unit_syntax_snapshot(link_key).unwrap();

        assert_eq!(parser_keys.len(), linked.ast.expressions.len());
        for (parser_key, linked_expression) in parser_keys.iter().zip(linked.ast.expressions.iter())
        {
            assert_eq!(
                linked.stable_expression_key(linked_expression.id),
                Some(parser_key.clone())
            );
        }
        let linked_owner_partitions = linked
            .owner_index()
            .entries()
            .iter()
            .map(|entry| {
                let view = linked.owner_view(&entry.route).unwrap();
                (
                    view.stable_key(),
                    view.statement_ids()
                        .iter()
                        .map(|statement| view.stable_statement_key_local(*statement).unwrap())
                        .collect::<Vec<_>>(),
                    view.stable_expression_keys().collect::<Vec<_>>(),
                    view.child_owners().to_vec(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(linked_owner_partitions, parser_owner_partitions);
        for entry in linked.owner_index().entries() {
            let view = linked.owner_view(&entry.route).unwrap();
            let owner = view.stable_key();
            for (local, statement) in view.statement_ids().iter().copied().zip(view.statements()) {
                assert_eq!(
                    linked.stable_check_owner_for_statement(statement.id),
                    Some(owner.clone())
                );
                assert_eq!(
                    linked.stable_statement_key(statement.id),
                    view.stable_statement_key_local(local)
                );
            }
            for expression in view.expressions() {
                assert_eq!(
                    linked.stable_check_owner_for_expression(expression.id),
                    Some(owner.clone())
                );
            }
        }
    }

    #[test]
    fn parsed_source_unit_is_normalized_raw_and_unit_local() {
        fn statement_ids(statements: &[AstStatement], source_len: usize, ids: &mut Vec<usize>) {
            for statement in statements {
                assert!(statement.line >= 1);
                assert!(statement.start <= statement.end);
                assert!(statement.end <= source_len);
                ids.push(statement.id);
                statement_ids(&statement.children, source_len, ids);
            }
        }

        let source = r#"FUNCTION helper(value) {
    value
}

result: helper(value: 1)
"#;
        let parsed = parse_source_unit("app\\Math.bn", source).unwrap();

        assert_eq!(parsed.source_unit_id.as_str(), "app/Math.bn");
        assert_eq!(parsed.path, "app/Math.bn");
        assert_eq!(parsed.source, source);
        assert_eq!(parsed.declared_functions, ["helper"]);
        assert!(
            parsed
                .declared_functions
                .iter()
                .all(|name| !name.contains('/'))
        );

        let mut ids = Vec::new();
        statement_ids(&parsed.ast.statements, source.len(), &mut ids);
        assert_eq!(ids, (0..ids.len()).collect::<Vec<_>>());
        assert!(
            parsed
                .ast
                .expressions
                .iter()
                .enumerate()
                .all(|(id, expression)| expression.id == id)
        );
        assert!(parsed.ast.expressions.iter().all(|expression| {
            expression.line >= 1
                && expression.start <= expression.end
                && expression.end <= source.len()
        }));
        assert!(parsed.ast.tokens.iter().all(|token| {
            token.line >= 1 && token.start <= token.end && token.end <= source.len()
        }));
        assert!(
            parsed.ast.lines.iter().all(|line| {
                line.line >= 1 && line.start <= line.end && line.end <= source.len()
            })
        );
        assert!(
            parsed.ast.items.iter().all(|item| {
                item.line >= 1 && item.start <= item.end && item.end <= source.len()
            })
        );
    }

    #[test]
    fn source_unit_eof_is_a_hard_lexical_and_delimiter_boundary() {
        for (source, expected) in [
            ("value: \"unterminated\n", "unclosed string literal"),
            ("continued\"\n", "unclosed string literal"),
            ("value: call(\n    input: 1\n", "unclosed `(`"),
            ("value: [\n    child: 1\n", "unclosed `[`"),
            ("value: TEXT {\n    unfinished\n", "unclosed `{`"),
            ("value: BYTES {\n    16uFF\n", "unclosed `{`"),
            (")\n", "unbalanced `)`"),
        ] {
            let error = parse_source_unit("units\\boundary.bn", source).unwrap_err();
            assert_eq!(error.path, "units/boundary.bn");
            assert!(error.message.contains(expected), "{source:?}: {error}");
        }

        let error = parse_source_unit("units/tail.bn", "|> Number/abs()\n").unwrap_err();
        assert!(
            error
                .message
                .contains("pipeline continuation has no preceding value"),
            "{error}"
        );

        let error = parse_source_unit("units/indented.bn", "    value: 1\n").unwrap_err();
        assert_eq!(error.path, "units/indented.bn");
        assert_eq!(error.line, Some(1));
        assert_eq!(error.column, Some(5));
        assert!(
            error.message.contains("indentation cannot cross"),
            "{error}"
        );
    }

    #[test]
    fn source_unit_accepts_valid_within_unit_multiline_syntax() {
        let source = r#"FUNCTION helper(value) {
    value
}

label: "first
second"
description: TEXT {
    a raw " remains text
}
result:
    helper(
        value: label
    )
"#;
        let parsed = parse_source_unit("units/multiline.bn", source).unwrap();

        assert_eq!(parsed.declared_functions, ["helper"]);
        assert!(
            parsed
                .ast
                .expressions
                .iter()
                .any(|expression| matches!(expression.kind, AstExprKind::StringLiteral(_)))
        );
        assert!(
            parsed
                .ast
                .expressions
                .iter()
                .any(|expression| matches!(expression.kind, AstExprKind::TextLiteral(_)))
        );
        assert!(parsed.ast.expressions.iter().any(|expression| {
            matches!(&expression.kind, AstExprKind::Call { function, .. } if function == "helper")
        }));
    }

    #[test]
    fn source_unit_assembler_rebases_dense_ids_and_every_nested_reference() {
        let base = parse_source_unit("project/a_base.bn", "seed: 1\n").unwrap();
        let base_expression_count = base.ast.expressions.len();
        let base_statement_count = dense_statement_count(&base.path, &base.ast.statements).unwrap();
        let main = parse_source_unit(
            "project/z_main.bn",
            r#"FUNCTION calculate(input) {
    BLOCK {
        answer: normalized
        normalized:
            input
            |> Number/abs()
        answer
    }
}

selected:
    candidate |> WHEN {
        True => calculate(input: seed)
        False => 0
    }
"#,
        )
        .unwrap();

        let assembled =
            assemble_parsed_source_units("project/z_main.bn", vec![main, base]).unwrap();

        assert_eq!(
            assembled
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["project/a_base.bn", "project/z_main.bn"]
        );
        assert_eq!(assembled.functions, ["calculate"]);
        assert_assembled_references_are_dense_and_in_range(&assembled.ast);
        assert!(assembled.ast.expressions.iter().any(|expression| {
            expression.id >= base_expression_count
                && matches!(expression.kind, AstExprKind::Block { .. })
        }));
        assert!(assembled.ast.expressions.iter().any(|expression| {
            matches!(&expression.kind, AstExprKind::Block { bindings, .. }
            if bindings.iter().any(|binding| matches!(
                binding.declaration,
                AstBlockBindingDeclaration::Local { statement }
                    if statement >= base_statement_count
            )))
        }));
        assert!(assembled.ast.expressions.iter().any(|expression| {
            matches!(&expression.kind, AstExprKind::Call { args, .. }
                if args.iter().any(|arg| arg.value >= base_expression_count))
        }));
    }

    #[test]
    fn cached_project_unit_route_matches_direct_project_parse() {
        let files = vec![
            (
                "project/Math.bn".to_owned(),
                "FUNCTION double(value) {\n    result: value + value\n}\n".to_owned(),
            ),
            (
                "project/RUN.bn".to_owned(),
                "result: Math/double(value: 2)\n".to_owned(),
            ),
        ];
        let direct = parse_project("project/RUN.bn", files.clone()).unwrap();
        let units = files
            .into_iter()
            .map(|(path, source)| parse_project_source_unit(path, source).unwrap())
            .collect();
        let (cached, assembly_profile) =
            assemble_parsed_source_units_profiled("project/RUN.bn", units);

        assert_eq!(cached.unwrap(), direct);
        assert_eq!(assembly_profile.work_counters.source_units_attempted, 0);
        assert_eq!(assembly_profile.work_counters.source_units_parsed, 0);
        assert!(assembly_profile.work_counters.nodes_rebased > 0);
    }

    #[test]
    fn source_unit_assembler_has_exact_placements_newline_isolation_and_order() {
        let alpha = parse_source_unit("project/a.bn", "alpha: 1").unwrap();
        let middle = parse_source_unit("project/middle.bn", "middle: 2\n").unwrap();
        let omega = parse_source_unit("project/z.bn", "omega: 3").unwrap();

        let first = assemble_parsed_source_units(
            "project/z.bn",
            vec![omega.clone(), alpha.clone(), middle.clone()],
        )
        .unwrap();
        let second = assemble_parsed_source_units(
            "project/z.bn",
            vec![middle.clone(), omega.clone(), alpha.clone()],
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.source, "alpha: 1\nmiddle: 2\nomega: 3");
        assert_eq!(
            first
                .files
                .iter()
                .map(|file| (file.path.as_str(), file.start_line))
                .collect::<Vec<_>>(),
            [
                ("project/a.bn", 1),
                ("project/middle.bn", 2),
                ("project/z.bn", 3),
            ]
        );
        assert_eq!(first.ast.statements.len(), 3);
        assert!(
            first
                .ast
                .statements
                .iter()
                .all(|statement| statement.children.is_empty())
        );

        let canonical_units = [&alpha, &middle, &omega];
        let mut byte_offset = 0usize;
        let mut line_offset = 0usize;
        let mut expression_offset = 0usize;
        let mut statement_offset = 0usize;
        let mut token_index = 0usize;
        let mut line_index = 0usize;
        let mut item_index = 0usize;
        for (unit_index, unit) in canonical_units.iter().enumerate() {
            let has_separator =
                unit_index + 1 < canonical_units.len() && !unit.source.ends_with('\n');
            let local_line_count = unit.source.lines().count().max(1);

            for local in &unit.ast.tokens {
                let global = &first.ast.tokens[token_index];
                assert_eq!(global.kind, local.kind);
                assert_eq!(global.lexeme, local.lexeme);
                assert_eq!(global.line, local.line + line_offset);
                assert_eq!(global.column, local.column);
                assert_eq!(global.start, local.start + byte_offset);
                assert_eq!(global.end, local.end + byte_offset);
                token_index += 1;
            }
            if has_separator {
                let separator = &first.ast.tokens[token_index];
                assert_eq!(separator.kind, AstTokenKind::Newline);
                assert_eq!(separator.lexeme, "\n");
                assert_eq!(separator.line, local_line_count + line_offset);
                assert_eq!(separator.start, byte_offset + unit.source.len());
                assert_eq!(separator.end, separator.start + 1);
                token_index += 1;
            }

            for local in &unit.ast.lines {
                let global = &first.ast.lines[line_index];
                let extends_to_separator = has_separator
                    && local.line == local_line_count
                    && local.end == unit.source.len();
                assert_eq!(global.line, local.line + line_offset);
                assert_eq!(global.start, local.start + byte_offset);
                assert_eq!(
                    global.end,
                    local.end + byte_offset + usize::from(extends_to_separator)
                );
                assert_eq!(global.symbols, local.symbols);
                assert_eq!(
                    global.symbol_spans,
                    local
                        .symbol_spans
                        .iter()
                        .map(|(start, end)| (start + byte_offset, end + byte_offset))
                        .collect::<Vec<_>>()
                );
                line_index += 1;
            }

            for local in &unit.ast.items {
                let global = &first.ast.items[item_index];
                assert_eq!(global.line, local.line + line_offset);
                assert_eq!(global.start, local.start + byte_offset);
                assert_eq!(global.end, local.end + byte_offset);
                assert_eq!(
                    global.symbol_spans,
                    local
                        .symbol_spans
                        .iter()
                        .map(|(start, end)| (start + byte_offset, end + byte_offset))
                        .collect::<Vec<_>>()
                );
                item_index += 1;
            }

            for local in &unit.ast.expressions {
                let global = &first.ast.expressions[local.id + expression_offset];
                assert_eq!(global.id, local.id + expression_offset);
                assert_eq!(global.line, local.line + line_offset);
                assert_eq!(global.start, local.start + byte_offset);
                assert_eq!(global.end, local.end + byte_offset);
            }
            let local_statement = unit.ast.statements.first().unwrap();
            let global_statement = &first.ast.statements[unit_index];
            assert_eq!(global_statement.id, local_statement.id + statement_offset);
            assert_eq!(global_statement.line, local_statement.line + line_offset);
            assert_eq!(global_statement.start, local_statement.start + byte_offset);
            assert_eq!(global_statement.end, local_statement.end + byte_offset);

            byte_offset += unit.source.len() + usize::from(has_separator);
            line_offset += local_line_count;
            expression_offset += unit.ast.expressions.len();
            statement_offset += dense_statement_count(&unit.path, &unit.ast.statements).unwrap();
        }
        assert_eq!(token_index, first.ast.tokens.len());
        assert_eq!(line_index, first.ast.lines.len());
        assert_eq!(item_index, first.ast.items.len());
        assert_eq!(byte_offset, first.source.len());
        assert_assembled_references_are_dense_and_in_range(&first.ast);
    }

    #[test]
    fn source_unit_assembler_fails_closed_on_bad_references_and_overflow() {
        let mut corrupt =
            parse_source_unit("project/main.bn", "value: helper(input: 1)\n").unwrap();
        let call = corrupt
            .fields
            .ast
            .expressions
            .__parser_make_mut()
            .iter_mut()
            .find_map(|expression| match &mut expression.kind {
                AstExprKind::Call { args, .. } => Some(args),
                _ => None,
            })
            .expect("call expression");
        call[0].value = usize::MAX;
        let error = assemble_parsed_source_units("project/main.bn", vec![corrupt]).unwrap_err();
        assert_eq!(error.path, "project/main.bn");
        assert!(error.message.contains("call argument value expression id"));

        let rebase = SourceUnitAstRebase {
            path: "project/overflow.bn",
            byte_offset: 1,
            line_offset: 0,
            expression_offset: 0,
            statement_offset: 0,
            local_source_len: usize::MAX,
            local_line_count: 1,
            local_expression_count: 0,
            local_statement_count: 0,
            work: &ParseWorkRecorder::disabled(),
        };
        let mut start = usize::MAX;
        let mut end = usize::MAX;
        let error = rebase
            .span(&mut start, &mut end, "overflow fixture")
            .unwrap_err();
        assert!(error.message.contains("overflows usize while rebasing"));
    }

    #[test]
    fn parse_project_qualifies_every_module_owned_function_surface_once() {
        let parsed = parse_project(
            "RUN.bn",
            [
                (
                    "Math.bn".to_owned(),
                    r#"FUNCTION double(value) {
    value |> helper()
}

FUNCTION helper(value) {
    value
}
"#
                    .to_owned(),
                ),
                (
                    "RUN.bn".to_owned(),
                    "result: Math/double(value: 21)\n".to_owned(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(
            parsed
                .files
                .iter()
                .map(|file| (file.path.as_str(), file.module.as_deref()))
                .collect::<Vec<_>>(),
            [("Math.bn", Some("Math")), ("RUN.bn", None)]
        );
        assert_eq!(parsed.functions, ["Math/double", "Math/helper"]);
        assert_eq!(
            collect_raw_declared_functions(&parsed.ast.statements),
            ["Math/double", "Math/helper"]
        );
        assert_eq!(
            parsed
                .ast
                .items
                .iter()
                .filter_map(|item| item.function.as_deref())
                .collect::<Vec<_>>(),
            ["Math/double", "Math/helper"]
        );
        assert!(parsed.ast.expressions.iter().any(|expression| {
            matches!(&expression.kind, AstExprKind::Call { function, .. }
                if function == "Math/double")
        }));
        assert!(parsed.ast.expressions.iter().any(|expression| {
            matches!(&expression.kind, AstExprKind::Pipe { op, .. }
                if op == "Math/helper")
        }));
    }

    #[test]
    fn parse_project_preserves_shared_uppercase_module_membership() {
        let parsed = parse_project(
            "RUN.bn",
            [
                (
                    "a/Math.bn".to_owned(),
                    "FUNCTION alpha(value) {\n    value\n}\n".to_owned(),
                ),
                (
                    "b/Math.bn".to_owned(),
                    "FUNCTION beta(value) {\n    alpha(value: value)\n}\n".to_owned(),
                ),
                (
                    "RUN.bn".to_owned(),
                    "result: Math/beta(value: 1)\n".to_owned(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(parsed.functions, ["Math/alpha", "Math/beta"]);
        assert!(parsed.ast.expressions.iter().any(|expression| {
            matches!(&expression.kind, AstExprKind::Call { function, .. }
                if function == "Math/alpha")
        }));
    }

    #[test]
    fn parse_project_enforces_hard_boundaries_between_canonical_units() {
        let continuation = parse_project(
            "z.bn",
            [
                ("a.bn".to_owned(), "value: 1\n".to_owned()),
                ("z.bn".to_owned(), "|> Number/abs()\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(continuation.path, "z.bn");
        assert!(
            continuation
                .message
                .contains("pipeline continuation has no preceding value")
        );

        let indentation = parse_project(
            "z.bn",
            [
                ("a.bn".to_owned(), "value: 1\n".to_owned()),
                ("z.bn".to_owned(), "    nested: 2\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(indentation.path, "z.bn");
        assert!(indentation.message.contains("indentation cannot cross"));

        let delimiter = parse_project(
            "z.bn",
            [
                ("a.bn".to_owned(), "value: helper(\n".to_owned()),
                ("z.bn".to_owned(), ")\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(delimiter.path, "a.bn");
        assert!(delimiter.message.contains("unclosed `(`"));
    }

    #[test]
    fn parse_project_runs_project_validation_and_projects_local_errors() {
        let reducer = parse_project(
            "z.bn",
            [
                (
                    "a.bn".to_owned(),
                    "selected: event.source |> WHEN {\n    True => 1\n}\n".to_owned(),
                ),
                (
                    "z.bn".to_owned(),
                    "next: state |> Number/abs()\n".to_owned(),
                ),
            ],
        )
        .unwrap_err();
        assert_eq!(reducer.path, "z.bn");
        assert!(reducer.message.contains("global event-source reducer"));

        let syntax = parse_project(
            "z.bn",
            [
                ("a.bn".to_owned(), "first: 1\nsecond: 2\n".to_owned()),
                (
                    "middle.bn".to_owned(),
                    "valid: 1\n# invalid comment\n".to_owned(),
                ),
                ("z.bn".to_owned(), "entry: 3\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(syntax.path, "middle.bn");
        assert_eq!(syntax.line, Some(2));
        assert!(syntax.message.contains("`#` comments are not supported"));
    }

    #[test]
    fn parse_project_preserves_entrypoint_owned_example_policy() {
        parse_project(
            "RUN.bn",
            [
                ("RUN.bn".to_owned(), "entry: 1\n".to_owned()),
                ("examples/helper.bn".to_owned(), "bg: 2\n".to_owned()),
            ],
        )
        .unwrap();

        let error = parse_project(
            "examples/RUN.bn",
            [
                ("examples/RUN.bn".to_owned(), "entry: 1\n".to_owned()),
                ("lib/helper.bn".to_owned(), "bg: 2\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(error.path, "lib/helper.bn");
        assert!(error.message.contains("canonical names"));
    }

    #[test]
    fn true_and_false_use_the_canonical_tag_ast_and_match_pattern() {
        let parsed = parse_source(
            "truth-tags.bn",
            r#"
truth: True
selected:
    truth |> WHEN {
        True => False
        False => True
    }
"#,
        )
        .unwrap();

        let expression_tags = parsed
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                AstExprKind::Tag(tag) => Some(tag.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(expression_tags.contains(&"True"));
        assert!(expression_tags.contains(&"False"));

        let pattern_tags = parsed
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Tag { name, .. },
                    ..
                } => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(pattern_tags, ["True", "False"]);

        let artifact = serde_json::to_string(&parsed).unwrap();
        assert!(!artifact.contains("\"kind\":\"bool\""));
        assert!(!artifact.contains("\"kind\":\"enum\""));
        assert!(!artifact.contains("\"kind\":\"record\""));
    }

    #[test]
    fn match_patterns_preserve_only_the_supported_typed_surface() {
        let parsed = parse_source(
            "typed-match-patterns.bn",
            r#"
selected:
    candidate |> WHEN {
        __ => 0
        whole => whole
        42 => 1
        TEXT { exact } => 2
        Ready => 3
        Found[value] => value
        InvalidNumber[reason, position] => position
    }
"#,
        )
        .unwrap();

        let patterns = parsed
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                AstExprKind::MatchArm { pattern, .. } => Some(pattern),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            patterns,
            [
                &AstMatchPattern::Wildcard,
                &AstMatchPattern::Binding {
                    name: "whole".to_owned(),
                },
                &AstMatchPattern::Number {
                    value: "42".to_owned(),
                },
                &AstMatchPattern::Text {
                    value: "exact".to_owned(),
                },
                &AstMatchPattern::Tag {
                    name: "Ready".to_owned(),
                    fields: Vec::new(),
                },
                &AstMatchPattern::Tag {
                    name: "Found".to_owned(),
                    fields: vec!["value".to_owned()],
                },
                &AstMatchPattern::Tag {
                    name: "InvalidNumber".to_owned(),
                    fields: vec!["reason".to_owned(), "position".to_owned()],
                },
            ]
        );
        let artifact = serde_json::to_string(&parsed).unwrap();
        assert!(!artifact.contains("\"pattern\":["));
        assert!(!artifact.contains("\"kind\":\"unknown\""));
    }

    #[test]
    fn unsupported_compound_match_patterns_fail_closed_with_targeted_errors() {
        let cases = [
            ("[field: value]", "object patterns are unsupported"),
            ("LIST { value }", "LIST patterns are unsupported"),
            ("MAP { key => value }", "MAP patterns are unsupported"),
            ("SET { value }", "SET patterns are unsupported"),
            ("NUMBER", "runtime type patterns are unsupported"),
            ("SKIP", "private flow-control states cannot be matched"),
            ("NaN", "is not a Number or a valid match pattern"),
            (
                "Found[value: renamed]",
                "do not support renaming, nesting, or comparison",
            ),
            (
                "Outer[Inner[value]]",
                "do not support renaming, nesting, or comparison",
            ),
            ("Found[value, value]", "cannot be bound more than once"),
            ("Found[Value]", "must be a lowercase field name"),
            ("{expected}", "dynamic comparison patterns are unsupported"),
        ];

        for (pattern, expected) in cases {
            let error = parse_source(
                "invalid-match-pattern.bn",
                format!(
                    "selected:\n    candidate |> WHEN {{\n        {pattern} => 1\n        __ => 0\n    }}\n"
                ),
            )
            .unwrap_err();
            assert!(
                error.message.contains(expected),
                "pattern `{pattern}` produced `{}`",
                error.message
            );
        }
    }

    #[test]
    fn parsed_program_digest_is_order_independent_and_uses_normalized_paths() {
        let forward = parse_project(
            "app/main.bn",
            [
                ("app/main.bn".to_owned(), "main_value: 1\n".to_owned()),
                ("app/helper.bn".to_owned(), "helper_value: 2\n".to_owned()),
            ],
        )
        .unwrap();
        let reverse = parse_project(
            "app\\main.bn",
            [
                ("app\\helper.bn".to_owned(), "helper_value: 2\n".to_owned()),
                ("app\\main.bn".to_owned(), "main_value: 1\n".to_owned()),
            ],
        )
        .unwrap();

        assert_eq!(
            forward.source_bundle_digest_v1,
            reverse.source_bundle_digest_v1
        );
        assert_eq!(forward, reverse);
        assert_eq!(forward.path, "app/main.bn");
        assert_eq!(
            forward
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["app/helper.bn", "app/main.bn"]
        );
    }

    #[test]
    fn parsed_program_digest_and_parser_input_preserve_exact_source_text() {
        let without_newline = parse_source("main.bn", "value: 1").unwrap();
        let with_newline = parse_source("main.bn", "value: 1\n").unwrap();

        assert_eq!(without_newline.source, "value: 1");
        assert_eq!(without_newline.files[0].source, "value: 1");
        assert_eq!(with_newline.source, "value: 1\n");
        assert_ne!(
            without_newline.source_bundle_digest_v1,
            with_newline.source_bundle_digest_v1
        );
    }

    #[test]
    fn parse_project_rejects_missing_and_ambiguous_entrypoints() {
        let missing = parse_project(
            "missing.bn",
            [("main.bn".to_owned(), "value: 1\n".to_owned())],
        )
        .unwrap_err();
        assert_eq!(missing.path, "missing.bn");
        assert!(missing.message.contains("entrypoint `missing.bn`"));
        assert!(missing.message.contains("not one of its units"));

        let ambiguous = parse_project(
            "app/main.bn",
            [
                ("app/main.bn".to_owned(), "value: 1\n".to_owned()),
                ("app\\main.bn".to_owned(), "value: 2\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert_eq!(ambiguous.path, "app/main.bn");
        assert!(
            ambiguous
                .message
                .contains("duplicate normalized path `app/main.bn`")
        );
    }

    #[test]
    fn parsed_program_serialization_includes_unforgeable_provenance_fields() {
        let parsed = parse_source("app/main.bn", "value: 1\n").unwrap();
        let cloned = parsed.clone();
        assert!(std::sync::Arc::ptr_eq(&parsed.fields, &cloned.fields));
        assert!(std::ptr::eq(
            parsed.expressions.as_slice().as_ptr(),
            parsed.ast.expressions.as_slice().as_ptr(),
        ));
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            serialized["source_bundle_digest_v1"],
            parsed.source_bundle_digest_v1.to_string()
        );
        assert_eq!(serialized["path"], "app/main.bn");
        assert!(serialized["expressions"].is_array());
        assert_eq!(serialized["expressions"], serialized["ast"]["expressions"]);

        let fields: ParsedProgramFields = serde_json::from_value(serialized).unwrap();
        assert_eq!(
            fields.source_bundle_digest_v1,
            parsed.source_bundle_digest_v1
        );
        assert_eq!(fields.path, parsed.path);
    }

    #[test]
    fn multiline_selectors_retain_owned_match_arms() {
        let parsed = parse_ast(
            "structured-selectors.bn",
            r#"
when_value:
    selector |> WHEN {
        Ready =>
            selector.value
            |> Number/abs()
        fallback => BLOCK {
            copied: fallback
            copied
        }
    }

while_value:
    selector |> WHILE {
        Ready => selector.value
        fallback => fallback
    }
"#,
        )
        .unwrap();

        let when = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::When { .. }))
            .expect("WHEN expression");
        let AstExprKind::When { arms, .. } = &when.kind else {
            unreachable!();
        };
        assert_eq!(arms.len(), 2);
        assert!(arms.iter().all(|arm| matches!(
            parsed.expressions[*arm].kind,
            AstExprKind::MatchArm {
                output: Some(_),
                ..
            }
        )));

        let ready_output = arms
            .iter()
            .find_map(|arm| match &parsed.expressions[*arm].kind {
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Tag { name, .. },
                    output: Some(output),
                } if name == "Ready" => Some(*output),
                _ => None,
            });
        let ready_output = ready_output.expect("Ready output");
        let ready = &parsed.expressions[ready_output];
        assert!(matches!(&ready.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs"));
        let linked_input = ready.linked_input.expect("multiline pipeline input");
        assert!(matches!(
            &parsed.expressions[linked_input].kind,
            AstExprKind::Path(parts) if parts == &["selector", "value"]
        ));

        let fallback_output = arms
            .iter()
            .find_map(|arm| match &parsed.expressions[*arm].kind {
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Binding { name },
                    output: Some(output),
                } if name == "fallback" => Some(*output),
                _ => None,
            })
            .expect("fallback output");
        let AstExprKind::Block { bindings, result } = &parsed.expressions[fallback_output].kind
        else {
            panic!("fallback output must remain a BLOCK");
        };
        assert_eq!(
            bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>(),
            ["copied"]
        );
        assert!(matches!(
            parsed.expressions[result.expect("fallback BLOCK result")].kind,
            AstExprKind::Identifier(ref name) if name == "copied"
        ));

        let while_arms = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::Pipe { op, arms, .. } if op == "WHILE" => Some(arms),
                _ => None,
            });
        assert_eq!(while_arms.expect("WHILE arms").len(), 2);
    }

    #[test]
    fn multiline_selector_structural_arms_replace_partial_inline_parse() {
        let parsed = parse_ast(
            "partial-inline-selector.bn",
            r#"
value:
    selector |> WHEN {
        True => TEXT { immediate }
        False =>
            TEXT { prefix }
            |> Text/concat(with: TEXT { suffix }, separator: " ")
    }
"#,
        )
        .unwrap();

        let when = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::When { .. }))
            .expect("WHEN expression");
        let AstExprKind::When { arms, .. } = &when.kind else {
            unreachable!();
        };
        assert_eq!(arms.len(), 2, "{arms:#?}");
        assert_eq!(
            arms.iter()
                .map(|arm| match &parsed.expressions[*arm].kind {
                    AstExprKind::MatchArm {
                        pattern: AstMatchPattern::Tag { name, .. },
                        ..
                    } => name.as_str(),
                    other => panic!("expected tag match arm, found {other:#?}"),
                })
                .collect::<Vec<_>>(),
            ["True", "False"]
        );
        let false_output = match &parsed.expressions[arms[1]].kind {
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            } => *output,
            other => panic!("False arm lost its output: {other:#?}"),
        };
        assert!(matches!(
            &parsed.expressions[false_output].kind,
            AstExprKind::Pipe { op, .. } if op == "Text/concat"
        ));
    }

    #[test]
    fn expression_block_retains_bindings_result_and_multiline_children() {
        let parsed = parse_ast(
            "structured-block.bn",
            r#"
FUNCTION calculate(input) {
    BLOCK {
        answer: normalized
        normalized:
            input
            |> Number/abs()
        answer
    }
}

FUNCTION row(input) {
    [
        value: input
    ]
}

rows: LIST {
    [value: 1]
    [value: 2]
}
"#,
        )
        .unwrap();

        let block = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::Block { .. }))
            .expect("BLOCK expression");
        let AstExprKind::Block { bindings, result } = &block.kind else {
            unreachable!();
        };
        assert_eq!(
            bindings
                .iter()
                .map(|binding| binding.name.as_str())
                .collect::<Vec<_>>(),
            ["answer", "normalized"]
        );
        assert!(bindings.iter().all(|binding| matches!(
            binding.declaration,
            AstBlockBindingDeclaration::Local { statement } if statement > 0
        )));
        assert!(matches!(
            parsed.expressions[result.expect("BLOCK result")].kind,
            AstExprKind::Identifier(ref name) if name == "answer"
        ));
        let normalized = bindings
            .iter()
            .find(|binding| binding.name == "normalized")
            .expect("normalized binding");
        assert!(matches!(
            parsed.expressions[normalized.value].kind,
            AstExprKind::Pipe { ref op, .. } if op == "Number/abs"
        ));
        assert!(parsed.expressions[normalized.value].linked_input.is_some());

        let record = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::Object(fields) if fields.iter().any(|field| field.name == "value") => {
                    Some(fields)
                }
                _ => None,
            })
            .expect("multiline record fields");
        assert_eq!(record.len(), 1);
        let list_items = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::ListLiteral { items, .. } if items.len() == 2 => Some(items),
                _ => None,
            });
        assert_eq!(list_items.expect("multiline list items").len(), 2);
    }

    #[test]
    fn multiline_list_pipeline_does_not_replace_the_last_item() {
        let parsed = parse_ast(
            "multiline-list-pipeline.bn",
            r#"
rows:
    LIST {
        [value: 1]
        [value: 2]
        [value: 3]
        [value: 4]
    }

    |> List/map(item, new: item)
    |> List/retain(item, if: True)
"#,
        )
        .unwrap();

        let list = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::ListLiteral { items, .. } if items.len() == 4
                )
            })
            .expect("four-item multiline list");
        let AstExprKind::ListLiteral { items, .. } = &list.kind else {
            unreachable!();
        };
        assert!(items.iter().all(|item| {
            parsed
                .expressions
                .get(*item)
                .is_some_and(|item| matches!(item.kind, AstExprKind::Object(_)))
        }));

        let map = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "List/map")
            })
            .expect("map continuation");
        let retain = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "List/retain")
            })
            .expect("retain continuation");
        assert_eq!(map.linked_input, Some(list.id));
        assert_eq!(retain.linked_input, Some(map.id));
        assert!(!items.contains(&map.id));
        assert!(!items.contains(&retain.id));
    }

    #[test]
    fn inline_multiline_list_pipeline_links_from_the_list_not_its_last_item() {
        let parsed = parse_ast(
            "inline-multiline-list-pipeline.bn",
            r#"
rows: LIST {
        [value: 1, retained: True]
        [value: 2, retained: False]
    }
    |> List/append(item: [value: 3])
    |> List/map(item, new: item)
"#,
        )
        .unwrap();

        let list = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::ListLiteral { items, .. } if items.len() == 2
                )
            })
            .expect("two-item inline multiline list");
        let AstExprKind::ListLiteral { items, .. } = &list.kind else {
            unreachable!();
        };
        let append = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "List/append")
            })
            .expect("append continuation");
        let map = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "List/map")
            })
            .expect("map continuation");

        assert_eq!(append.linked_input, Some(list.id));
        assert_eq!(map.linked_input, Some(append.id));
        assert!(!items.contains(&append.id));
        assert!(!items.contains(&map.id));
    }

    #[test]
    fn hanging_indented_pipeline_retains_exact_input_chain() {
        let parsed = parse_ast(
            "hanging-indented-pipeline.bn",
            r#"
value:
    input
        |> Number/abs()
        |> Number/floor()
"#,
        )
        .unwrap();

        let input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "input")
            })
            .expect("pipeline source");
        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("first continuation");
        let floor = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/floor")
            })
            .expect("second continuation");

        assert_eq!(abs.linked_input, Some(input.id));
        assert_eq!(floor.linked_input, Some(abs.id));
    }

    #[test]
    fn named_leading_minus_is_unary_not_a_sibling_continuation() {
        let parsed = parse_ast(
            "named-negative.bn",
            r#"
first: 10
second: -5 / 2 |> Number/round(to: 1, using: NearestEven)
third: 1 |> Number/round(to: -0.1, using: TowardZero)
"#,
        )
        .unwrap();

        let second = parsed
            .statements
            .iter()
            .find(|statement| {
                matches!(&statement.kind, AstStatementKind::Field { name } if name == "second")
            })
            .and_then(|statement| statement.expr)
            .expect("second field expression");
        let AstExprKind::Pipe { input, .. } = &parsed.expressions[second].kind else {
            panic!("second field must retain its rounding call");
        };
        let AstExprKind::Infix { left, op, right: _ } = &parsed.expressions[*input].kind else {
            panic!("second field must retain unary negation");
        };
        assert_eq!(op, "-");
        assert_eq!(parsed.expressions[*input].linked_input, None);
        assert!(matches!(
            &parsed.expressions[*left].kind,
            AstExprKind::Number(value) if value == "0"
        ));

        let third = parsed
            .statements
            .iter()
            .find(|statement| {
                matches!(&statement.kind, AstStatementKind::Field { name } if name == "third")
            })
            .and_then(|statement| statement.expr)
            .expect("third field expression");
        let AstExprKind::Pipe { args, .. } = &parsed.expressions[third].kind else {
            panic!("third field must retain its rounding call");
        };
        let quantum = args
            .iter()
            .find(|argument| argument.name == "to")
            .map(|argument| argument.value)
            .expect("rounding quantum");
        assert_eq!(parsed.expressions[quantum].linked_input, None);
        match &parsed.expressions[quantum].kind {
            AstExprKind::Number(value) => assert_eq!(value, "-0.1"),
            AstExprKind::Infix { left, op, right: _ } => {
                assert_eq!(op, "-");
                assert!(matches!(
                    &parsed.expressions[*left].kind,
                    AstExprKind::Number(value) if value == "0"
                ));
            }
            _ => panic!("negative call argument must retain unary negation"),
        }
    }

    #[test]
    fn one_line_continuation_links_the_innermost_placeholder_operator() {
        let parsed = parse_ast(
            "one-line-continuation.bn",
            r#"
value:
    input
    |> Number/abs() |> Number/floor()
"#,
        )
        .unwrap();

        let input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "input")
            })
            .expect("pipeline source");
        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("placeholder-bearing operator");
        let floor = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/floor")
            })
            .expect("outer operator");

        assert_eq!(abs.linked_input, Some(input.id));
        assert_eq!(floor.linked_input, None);
        assert!(matches!(
            floor.kind,
            AstExprKind::Pipe { input, .. } if input == abs.id
        ));
    }

    #[test]
    fn every_special_pipeline_continuation_receives_its_exact_input() {
        let parsed = parse_ast(
            "special-continuations.bn",
            r#"
when_value:
    when_input
    |> WHEN {
        True => 1
        False => 0
    }

then_value:
    then_input
    |> THEN { 1 }

hold_value:
    hold_input
    |> HOLD held { LATEST {} }

draining_value:
    draining_input
    |> DRAINING

while_value:
    while_input
    |> WHILE {
        True => 1
        False => 0
    }
"#,
        )
        .unwrap();

        let expected = [
            ("when_input", "WHEN"),
            ("then_input", "THEN"),
            ("hold_input", "HOLD"),
            ("draining_input", "DRAINING"),
            ("while_input", "WHILE"),
        ];
        for (input_name, operator) in expected {
            let expression = parsed
                .expressions
                .iter()
                .find(|expression| match (&expression.kind, operator) {
                    (AstExprKind::When { .. }, "WHEN")
                    | (AstExprKind::Then { .. }, "THEN")
                    | (AstExprKind::Hold { .. }, "HOLD")
                    | (AstExprKind::Draining { .. }, "DRAINING") => true,
                    (AstExprKind::Pipe { op, .. }, "WHILE") => op == "WHILE",
                    _ => false,
                })
                .unwrap_or_else(|| panic!("{operator} continuation"));
            let linked = expression
                .linked_input
                .unwrap_or_else(|| panic!("{operator} linked input"));
            assert!(matches!(
                &parsed.expressions[linked].kind,
                AstExprKind::Identifier(name) if name == input_name
            ));
        }
    }

    #[test]
    fn nested_hold_pipeline_links_to_its_local_predecessor() {
        let parsed = parse_ast(
            "nested-hold-pipeline.bn",
            r#"
value:
    selector |> WHEN {
        Ready =>
            [previous: 0, current: 1]
            |> HOLD state {
                count - 1
                |> Stream/pulses()
                |> THEN { state.current }
            }
    }
"#,
        )
        .unwrap();
        let pulses = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::Pipe { op, .. } if op == "Stream/pulses"
                )
            })
            .expect("nested Stream/pulses expression");
        let linked = pulses.linked_input.expect("nested pulse predecessor");
        assert!(
            matches!(
                &parsed.expressions[linked].kind,
                AstExprKind::Infix { op, .. } if op == "-"
            ),
            "nested pulse linked to {:?}",
            parsed.expressions[linked].kind,
        );
        let then = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::Then { .. }))
            .expect("nested THEN expression");
        assert_eq!(then.linked_input, Some(pulses.id));
    }

    #[test]
    fn nested_block_pipeline_links_to_its_local_predecessor() {
        let parsed = parse_ast(
            "nested-block-pipeline.bn",
            r#"
value:
    BLOCK {
        items: LIST { 6 }
        items
        |> List/is_not_empty()
    }
"#,
        )
        .unwrap();
        let is_not_empty = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::Pipe { op, .. } if op == "List/is_not_empty"
                )
            })
            .expect("nested List/is_not_empty expression");
        let linked = is_not_empty
            .linked_input
            .expect("nested block pipeline predecessor");
        assert!(
            matches!(
                &parsed.expressions[linked].kind,
                AstExprKind::Identifier(name) if name == "items"
            ),
            "nested block pipeline linked to {:?}",
            parsed.expressions[linked].kind,
        );
        let block = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::Block { .. }))
            .expect("nested BLOCK expression");
        let AstExprKind::Block { result, .. } = &block.kind else {
            unreachable!();
        };
        assert_eq!(*result, Some(is_not_empty.id));
    }

    #[test]
    fn pipeline_after_a_closed_block_stays_outside_the_block_result() {
        let parsed = parse_ast(
            "outer-block-pipeline.bn",
            r#"
value:
    BLOCK {
        items: LIST { 6 }
        items
    }
    |> List/is_not_empty()
"#,
        )
        .unwrap();
        let block = parsed
            .expressions
            .iter()
            .find(|expression| matches!(expression.kind, AstExprKind::Block { .. }))
            .expect("closed BLOCK expression");
        let AstExprKind::Block { result, .. } = &block.kind else {
            unreachable!();
        };
        let result = result.expect("closed BLOCK result");
        assert!(matches!(
            &parsed.expressions[result].kind,
            AstExprKind::Identifier(name) if name == "items"
        ));
        let outer = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(
                    &expression.kind,
                    AstExprKind::Pipe { op, .. } if op == "List/is_not_empty"
                )
            })
            .expect("outer List/is_not_empty expression");
        assert_eq!(outer.linked_input, Some(block.id));
        assert_ne!(outer.id, result);
    }

    #[test]
    fn multiline_list_item_pipeline_keeps_its_local_predecessor() {
        let parsed = parse_ast(
            "multiline-list-item-pipeline.bn",
            r#"
value:
    LIST {
        input
        |> Number/abs()
    }
"#,
        )
        .unwrap();
        let input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "input")
            })
            .expect("list item input");
        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("list item pipeline");
        assert_eq!(abs.linked_input, Some(input.id));
    }

    #[test]
    fn inline_match_arm_and_then_outputs_extend_to_the_final_continuation() {
        let parsed = parse_ast(
            "inline-output-continuations.bn",
            r#"
matched:
    selector |> WHEN {
        Ready => match_input
            |> Number/abs()
        __ => 0
    }

triggered:
    trigger |> THEN { then_input }
        |> Number/floor()
"#,
        )
        .unwrap();

        let abs = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/abs")
            })
            .expect("match-arm continuation");
        let match_input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "match_input")
            })
            .expect("match-arm input");
        assert_eq!(abs.linked_input, Some(match_input.id));
        assert!(parsed.expressions.iter().any(|expression| matches!(
            expression.kind,
            AstExprKind::MatchArm { output: Some(output), .. } if output == abs.id
        )));

        let floor = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Pipe { op, .. } if op == "Number/floor")
            })
            .expect("THEN output continuation");
        let then_input = parsed
            .expressions
            .iter()
            .find(|expression| {
                matches!(&expression.kind, AstExprKind::Identifier(name) if name == "then_input")
            })
            .expect("THEN output input");
        assert_eq!(floor.linked_input, Some(then_input.id));
        assert!(parsed.expressions.iter().any(|expression| matches!(
            expression.kind,
            AstExprKind::Then { output: Some(output), .. } if output == floor.id
        )));
    }

    #[test]
    fn orphan_pipeline_continuation_is_rejected_by_the_parser() {
        let error = parse_ast(
            "orphan-continuation.bn",
            r#"
value: |> Number/abs()
"#,
        )
        .expect_err("orphan continuation must fail");

        assert!(
            error
                .message
                .contains("pipeline continuation has no preceding value")
        );
    }

    #[test]
    fn pipeline_field_projection_has_exact_syntax_and_keeps_call_parentheses_strict() {
        let parsed =
            parse_source("field-projection.bn", "value: [answer: 42] |> .answer\n").unwrap();
        assert!(parsed.ast.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                AstExprKind::Pipe { op, .. } if op == "Field/answer"
            )
        }));

        let error = parse_source("missing-call-parentheses.bn", "value: 7 |> Number/abs\n")
            .expect_err("ordinary pipeline functions still require a call");
        assert!(
            error
                .message
                .contains("pipeline function `Number/abs` must be called with parentheses")
        );
    }

    #[test]
    fn every_cells_pipeline_placeholder_is_parser_linked() {
        let files = [
            ("cell.bn", include_str!("../../../examples/cells/cell.bn")),
            (
                "columns.bn",
                include_str!("../../../examples/cells/columns.bn"),
            ),
            (
                "defaults.bn",
                include_str!("../../../examples/cells/defaults.bn"),
            ),
            (
                "formula.bn",
                include_str!("../../../examples/cells/formula.bn"),
            ),
            ("model.bn", include_str!("../../../examples/cells/model.bn")),
            ("store.bn", include_str!("../../../examples/cells/store.bn")),
            ("view.bn", include_str!("../../../examples/cells/view.bn")),
        ];
        let mut placeholder_count = 0usize;
        for (path, source) in files {
            let parsed = parse_ast(path, source).unwrap_or_else(|error| panic!("{path}: {error}"));
            for expression in &parsed.expressions {
                let input = match &expression.kind {
                    AstExprKind::Pipe { input, .. }
                    | AstExprKind::Then { input, .. }
                    | AstExprKind::When { input, .. }
                    | AstExprKind::Draining { input }
                    | AstExprKind::Hold { initial: input, .. } => *input,
                    _ => continue,
                };
                if parsed
                    .expressions
                    .get(input)
                    .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
                {
                    placeholder_count += 1;
                    assert!(
                        expression.linked_input.is_some(),
                        "{path}: expression {}",
                        expression.id
                    );
                }
            }
        }
        assert!(placeholder_count > 0);
    }

    #[test]
    fn grouped_infix_expression_preserves_the_inner_operation() {
        let parsed = parse_source(
            "grouped-infix.bn",
            r#"
store: [
    value: (input.value + 1) * 2
]
"#,
        )
        .unwrap();
        let multiply = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Infix { op, .. } if op == "*"))
            .expect("outer multiplication is parsed");
        let AstExprKind::Infix { left, .. } = multiply.kind else {
            unreachable!();
        };
        assert!(matches!(
            parsed.expressions[left].kind,
            AstExprKind::Infix { ref op, .. } if op == "+"
        ));
    }

    #[test]
    fn comparison_pipeline_feeds_when_with_the_comparison_result() {
        let parsed = parse_source(
            "comparison-when.bn",
            r#"
store: [
    left: [value: 1]
    right: [value: 1]
    result:
        left
        == right
        |> WHEN {
            True => TEXT { equal }
            False => TEXT { different }
        }
]
"#,
        )
        .unwrap();
        let when = parsed
            .expressions
            .iter()
            .find(|expr| matches!(expr.kind, AstExprKind::When { .. }))
            .expect("WHEN expression");
        let AstExprKind::When { input, .. } = when.kind else {
            unreachable!();
        };
        let input = when.linked_input.unwrap_or(input);
        assert!(
            matches!(
                parsed.expressions[input].kind,
                AstExprKind::Infix { ref op, .. } if op == "=="
            ),
            "WHEN input: {:#?}",
            parsed.expressions[input]
        );
    }

    #[test]
    fn nested_grouped_expression_preserves_every_operation() {
        let parsed = parse_source(
            "nested-grouped-infix.bn",
            r#"
store: [
    value: ((input.value + 1) * 2)
]
"#,
        )
        .unwrap();
        let multiply = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Infix { op, .. } if op == "*"))
            .expect("outer groups are unwrapped");
        let AstExprKind::Infix { left, .. } = multiply.kind else {
            unreachable!();
        };
        assert!(matches!(
            parsed.expressions[left].kind,
            AstExprKind::Infix { ref op, .. } if op == "+"
        ));
    }

    #[test]
    fn text_interpolation_creates_structured_children_with_exact_spans() {
        let source = r#"
store: [value: 7]
label: TEXT {
    prefix
    {store.value}
    suffix
}
"#;
        let parsed = parse_source("text-interpolation-span.bn", source).unwrap();
        let dynamic = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::TextTemplate { segments } => {
                    segments.iter().find_map(|segment| match segment {
                        AstTextSegment::Static { .. } => None,
                        AstTextSegment::Dynamic { value } => Some(*value),
                    })
                }
                _ => None,
            })
            .expect("structured text interpolation child");
        let dynamic = &parsed.expressions[dynamic];

        assert_eq!(&source[dynamic.start..dynamic.end], "store.value");
        assert_eq!(dynamic.line, 5);
        assert!(matches!(
            &dynamic.kind,
            AstExprKind::Path(path) if path == &["store", "value"]
        ));
    }

    #[test]
    fn multiline_list_map_retains_bare_binding_and_named_call() {
        let parsed = parse_source(
            "multiline-dynamic-map.bn",
            r#"
store: [
    input_rows:
        True |> WHEN {
            True => LIST { [id: TEXT { a }] }
            False => LIST {}
        }
    mapped_rows:
        input_rows
        |> List/map(row, new:
            new_row(row: row)
        )
]

FUNCTION new_row(row) {
    [
        select: SOURCE
        id: row.id
    ]
}
"#,
        )
        .unwrap();
        let args = parsed
            .expressions
            .iter()
            .find_map(|expr| match &expr.kind {
                AstExprKind::Pipe { op, args, .. } if op == "List/map" => Some(args),
                _ => None,
            })
            .expect("multiline List/map arguments");
        let row_arg = args
            .iter()
            .find(|arg| arg.is_bare_binding())
            .expect("bare row binding");
        assert_eq!(row_arg.name, "row");
        assert!(matches!(
            parsed.expressions[row_arg.value].kind,
            AstExprKind::Identifier(ref name) if name == "row"
        ));
        let new_arg = args
            .iter()
            .find(|arg| arg.named_name() == Some("new"))
            .expect("multiline new argument is normalized in the AST");
        assert!(matches!(
            parsed.expressions[new_arg.value].kind,
            AstExprKind::Call { ref function, .. } if function == "new_row"
        ));
    }

    #[test]
    fn multiline_then_uses_the_final_pipeline_expression_as_its_output() {
        let parsed = parse_source(
            "multiline-then-pipeline.bn",
            r#"
store: [
    submit: SOURCE
    normalized:
        submit |> THEN {
            submit.text
                |> Text/trim()
                |> Text/to_uppercase()
        }
]
"#,
        )
        .unwrap();
        let output = parsed
            .expressions
            .iter()
            .find_map(|expr| match &expr.kind {
                AstExprKind::Then {
                    output: Some(output),
                    ..
                } => Some(*output),
                _ => None,
            })
            .expect("multiline THEN has an output");
        assert!(matches!(
            parsed.expressions[output].kind,
            AstExprKind::Pipe { ref op, .. } if op == "Text/to_uppercase"
        ));
    }

    #[test]
    fn multiline_when_arm_uses_the_final_pipeline_expression_as_its_output() {
        let parsed = parse_source(
            "multiline-when-arm-pipeline.bn",
            r#"
store: [
    status:
        result |> WHEN {
            Ready =>
                result.name
                |> Text/concat(with: TEXT { ready }, separator: " ")
            __ => TEXT { pending }
        }
]
"#,
        )
        .unwrap();
        let concat = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "Text/concat"))
            .expect("concat continuation exists");
        assert!(parsed.expressions.iter().any(|expr| {
            matches!(expr.kind, AstExprKind::MatchArm { output: Some(output), .. } if output == concat.id)
        }));
    }

    #[test]
    fn multiline_when_arm_call_pipeline_uses_the_final_operator_as_its_output() {
        let parsed = parse_source(
            "multiline-when-arm-call-pipeline.bn",
            r#"
store: [
    rows: LIST { [id: TEXT { one }, values: LIST {}] }
    selected:
        result |> WHEN {
            Ready =>
                rows
                |> List/find(item, if: item.id == TEXT { one })
                |> WHEN {
                    Found[value] => value.values
                    NotFound => LIST {}
                }
                |> List/map(item, new: item)
            __ => LIST {}
        }
]
"#,
        )
        .unwrap();
        let map = parsed
            .expressions
            .iter()
            .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "List/map"))
            .expect("map continuation exists");
        let ready_output = parsed.expressions.iter().find_map(|expr| match &expr.kind {
            AstExprKind::MatchArm {
                pattern: AstMatchPattern::Tag { name, .. },
                output: Some(output),
            } if name == "Ready" => Some(*output),
            _ => None,
        });
        assert_eq!(ready_output, Some(map.id));
    }

    #[test]
    fn list_literal_ast_preserves_scalar_record_empty_and_capacity_forms() {
        let parsed = parse_source(
            "scalar-list-value.bn",
            r#"
store: [
    selected: TEXT { alpha }
    selected_ids: LIST { selected }
    optional_selected_ids:
        True |> WHEN {
            True => LIST {}
            False => LIST { selected }
        }
    rows: LIST { [id: selected] }
    optional_rows:
        True |> WHEN {
            True => LIST {}
            False => LIST { [id: selected] }
    }
    empty_rows: LIST {}
    bounded_rows: LIST[4] {}
]
"#,
        )
        .unwrap();
        let lists = parsed
            .expressions
            .iter()
            .filter_map(|expr| match &expr.kind {
                AstExprKind::ListLiteral { capacity, items } => Some((*capacity, items.as_slice())),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert!(lists.iter().any(|(_, items)| {
            items.len() == 1
                && matches!(
                    parsed.expressions[items[0]].kind,
                    AstExprKind::Identifier(ref name) if name == "selected"
                )
        }));
        assert!(lists.iter().any(|(_, items)| {
            items.len() == 1
                && matches!(
                    &parsed.expressions[items[0]].kind,
                    AstExprKind::Object(fields)
                        if fields.iter().any(|field| field.name == "id")
                )
        }));
        assert!(
            lists
                .iter()
                .any(|(capacity, items)| capacity.is_none() && items.is_empty())
        );
        fn contains_bounded_rows(statements: &[AstStatement]) -> bool {
            statements.iter().any(|statement| {
                matches!(
                    &statement.kind,
                    AstStatementKind::List {
                        field: Some(field),
                        capacity: Some(4),
                    } if field == "bounded_rows"
                ) || contains_bounded_rows(&statement.children)
            })
        }
        assert!(contains_bounded_rows(&parsed.ast.statements));
    }

    #[test]
    fn map_and_set_ast_keep_dynamic_keys_separate_from_match_arms() {
        let parsed = parse_ast(
            "map-set-structure.bn",
            r#"
settings: MAP {
    TEXT { theme } => [enabled: True]
    selected.id => selected.value
}
tags: SET {
    TEXT { alpha }
    Ready
}
result:
    status |> WHEN {
        Ready => settings
        __ => MAP {}
    }
"#,
        )
        .unwrap();

        let populated_map = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::MapLiteral { entries } if entries.len() == 2 => Some(entries),
                _ => None,
            })
            .expect("populated MAP literal");
        assert!(populated_map.iter().all(|entry| {
            matches!(
                parsed.expressions[*entry].kind,
                AstExprKind::MapEntry { .. }
            )
        }));
        assert!(parsed.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                AstExprKind::MapEntry { key, .. }
                    if matches!(
                        &parsed.expressions[*key].kind,
                        AstExprKind::Path(parts) if parts == &["selected", "id"]
                    )
            )
        }));

        let set_items = parsed
            .expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                AstExprKind::SetLiteral { items } if items.len() == 2 => Some(items),
                _ => None,
            })
            .expect("SET literal");
        assert_eq!(set_items.len(), 2);
        assert!(
            parsed
                .expressions
                .iter()
                .any(|expression| matches!(expression.kind, AstExprKind::MatchArm { .. }))
        );
        assert!(
            !parsed
                .expressions
                .iter()
                .any(|expression| matches!(expression.kind, AstExprKind::Arrow { .. }))
        );
    }

    #[test]
    fn map_and_when_arrows_survive_canonical_formatting_idempotently() {
        fn semantic_ast(value: &[AstExpr]) -> serde_json::Value {
            fn remove_source_locations(value: &mut serde_json::Value) {
                match value {
                    serde_json::Value::Array(values) => {
                        for value in values {
                            remove_source_locations(value);
                        }
                    }
                    serde_json::Value::Object(fields) => {
                        fields.remove("line");
                        fields.remove("start");
                        fields.remove("end");
                        for value in fields.values_mut() {
                            remove_source_locations(value);
                        }
                    }
                    _ => {}
                }
            }

            let mut value = serde_json::to_value(value).unwrap();
            remove_source_locations(&mut value);
            value
        }

        let source = "settings: MAP {   \n    TEXT { theme } => [enabled: True]\n    selected.id => selected.value\n}\n\n\nresult:\n    status |> WHEN {\n        Ready => settings\n        __ => MAP {}\n    }   \n";
        let parsed = parse_source("map-when-format.bn", source).unwrap();
        let formatted = format_source("map-when-format.bn", source).unwrap();
        let reparsed = parse_source("map-when-format.bn", &formatted).unwrap();
        let reformatted = format_source("map-when-format.bn", &formatted).unwrap();

        assert_eq!(formatted, reformatted);
        assert_eq!(
            semantic_ast(&parsed.expressions),
            semantic_ast(&reparsed.expressions)
        );
        assert_eq!(formatted.matches("=>").count(), 4);
        assert_eq!(formatted.matches("\n\n").count(), 1);
    }

    #[test]
    fn bits_literals_and_exact_patterns_preserve_static_width_and_encoding() {
        let parsed = parse_source(
            "bits-literals.bn",
            r#"
opcode: BITS[7] { 2u011_0011 }
kind:
    opcode |> WHEN {
        BITS[7] { 2u011_0011 } => Register
        __ => Unknown
    }
"#,
        )
        .unwrap();
        assert!(parsed.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                AstExprKind::BitsLiteral {
                    width: 7,
                    radix: 2,
                    digits,
                } if digits == "011_0011"
            )
        }));
        assert!(parsed.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Bits {
                        width: 7,
                        radix: 2,
                        digits,
                    },
                    ..
                } if digits == "011_0011"
            )
        }));
    }

    #[test]
    fn bits_literal_syntax_rejects_zero_width_fragments_signs_and_bad_digits() {
        for (source, expected) in [
            ("value: BITS[0] { 2u0 }", "positive"),
            ("value: BITS[__] { 2u0 }", "compile-time integer"),
            ("value: BITS[8] { 2 u1010 }", "adjacent"),
            ("value: BITS[8] { 2u10 2u10 }", "exactly one token"),
            ("value: BITS[8] { - 2u1 }", "exactly one token"),
            ("value: BITS[8] { 2u012 }", "outside radix"),
        ] {
            let error = parse_source("invalid-bits.bn", source).unwrap_err();
            assert!(error.message.contains(expected), "{source}: {error}");
        }
    }

    #[test]
    fn function_parameters_and_call_entries_preserve_out_and_pass_structure() {
        let parsed = parse_source(
            "out-structure.bn",
            r#"
FUNCTION map(list, item: OUT, new) {
    new
}

items: LIST { [value: 1] }
mapped:
    map(
        list: items
        item
        new: item.value
        PASS: [theme: theme]
    )
"#,
        )
        .unwrap();

        let parameters = parsed
            .ast
            .statements
            .iter()
            .find_map(|statement| match &statement.kind {
                AstStatementKind::Function { name, parameters } if name == "map" => {
                    Some(parameters)
                }
                _ => None,
            })
            .expect("map parameters");
        assert_eq!(
            parameters
                .iter()
                .map(|parameter| (parameter.name.as_str(), parameter.kind))
                .collect::<Vec<_>>(),
            vec![
                ("list", AstParameterKind::Value),
                ("item", AstParameterKind::Out),
                ("new", AstParameterKind::Value),
            ]
        );

        let (args, pass) = parsed
            .expressions
            .iter()
            .find_map(|expr| match &expr.kind {
                AstExprKind::Call {
                    function,
                    args,
                    pass,
                } if function == "map" => Some((args, pass.as_ref())),
                _ => None,
            })
            .expect("map call");
        assert_eq!(args.len(), 3);
        assert_eq!(args[0].named_name(), Some("list"));
        assert!(args[1].is_bare_binding());
        assert_eq!(args[1].name, "item");
        assert_eq!(args[2].named_name(), Some("new"));
        assert!(pass.is_some());
    }

    #[test]
    fn pass_must_be_unique_and_final() {
        for source in [
            "value: render(PASS: [theme: theme], value: 1)",
            "value: render(value: 1, PASS: [theme: theme], PASS: [theme: other])",
        ] {
            let error = parse_source("invalid-pass.bn", source).unwrap_err();
            assert!(error.message.contains("`PASS`"), "{error}");
        }
        parse_source(
            "valid-pass.bn",
            "value: render(value: 1, PASS: [theme: theme])",
        )
        .unwrap();
        parse_source(
            "valid-multiline-pass.bn",
            "value:\n    render(\n        value: 1\n        PASS: [theme: theme]\n    )\n",
        )
        .unwrap();
        let error = parse_source(
            "invalid-multiline-pass.bn",
            "value:\n    render(\n        PASS: [theme: theme]\n        value: 1\n    )\n",
        )
        .unwrap_err();
        assert!(error.message.contains("final call clause"), "{error}");
    }

    #[test]
    fn ordinary_positional_call_entries_are_rejected() {
        let error = parse_source("positional.bn", "value: render(1)").unwrap_err();
        assert!(error.message.contains("ordinary arguments use"), "{error}");
    }

    #[test]
    fn function_parameter_kinds_are_exact() {
        let error =
            parse_source("invalid-parameter.bn", "FUNCTION map(item: EACH) { item }").unwrap_err();
        assert!(error.message.contains("`name` or `name: OUT`"), "{error}");
    }

    #[test]
    fn application_code_cannot_shadow_canonical_standard_roots() {
        let field_error = parse_source("app.bn", "SessionInfo: 1\n").unwrap_err();
        assert!(field_error.message.contains("reserved Boon standard root"));

        let function_error =
            parse_source("app.bn", "FUNCTION List/custom(value) { value }\n").unwrap_err();
        assert!(
            function_error
                .message
                .contains("reserved Boon standard namespace")
        );

        let projection_error =
            parse_source("app.bn", "FUNCTION Field/custom(value) { value }\n").unwrap_err();
        assert!(
            projection_error
                .message
                .contains("reserved Boon standard namespace")
        );

        let module_error = parse_project(
            "RUN.bn",
            [
                ("RUN.bn".to_owned(), "store: [ready: True]\n".to_owned()),
                ("File.bn".to_owned(), "value: 1\n".to_owned()),
            ],
        )
        .unwrap_err();
        assert!(
            module_error
                .message
                .contains("reserved Boon standard namespace")
        );

        for role in ["Client", "Session", "Server"] {
            let source = format!("{role}: 1\n");
            let error = parse_source("app.bn", source).unwrap_err();
            assert!(error.message.contains("reserved Boon standard root"));
        }
    }

    #[test]
    fn planned_unimplemented_spellings_fail_closed_outside_comments_and_text() {
        for feature in LANGUAGE_FEATURE_REGISTRY.iter().filter(|feature| {
            feature.stage == LanguageFeatureStage::Planned
                && feature.parse_expectation == LanguageFeatureParseExpectation::Reject
        }) {
            for spelling in feature.spellings {
                let source = format!("value: {spelling} {{}}\n");
                let error = parse_source("planned-language-surface.bn", source).unwrap_err();
                assert!(
                    error.message.contains(feature.id),
                    "unexpected `{spelling}` error for {}: {error}",
                    feature.id
                );
            }
        }

        parse_source(
            "planned-words-in-text.bn",
            "-- BITS FLUSH MAP SET WHERE remain ordinary comment text\n\
             note: TEXT { BITS FLUSH MAP SET WHERE }\n",
        )
        .unwrap();
    }

    #[test]
    fn obsolete_dotted_role_qualification_is_rejected_for_every_role_root() {
        for role in ProgramRoleRoot::ALL {
            let namespace = role.namespace();
            let source = format!("value: {namespace}.foo\n");
            let error = parse_source("obsolete-role-path.bn", source).unwrap_err();
            assert!(
                error
                    .message
                    .contains(&format!("use `{namespace}/value.field`")),
                "unexpected {namespace} diagnostic: {error}"
            );
        }
    }

    #[test]
    fn byte_lexer_preserves_padded_spans_columns_and_fail_closed_edges() {
        let tokens = lex_source("probe.bn", "  alpha  \n\twide <= \"h\\\"i\" -- note\n").unwrap();
        assert_eq!(
            tokens
                .iter()
                .map(|token| {
                    (
                        token.kind,
                        token.lexeme.as_str(),
                        token.line,
                        token.column,
                        token.start,
                        token.end,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (AstTokenKind::Identifier, "alpha", 1, 1, 0, 9),
                (AstTokenKind::Newline, "\n\t", 1, 10, 9, 11),
                (AstTokenKind::Identifier, "wide", 2, 2, 11, 16),
                (AstTokenKind::Operator, "<=", 2, 7, 16, 19),
                (AstTokenKind::String, "\"h\\\"i\" ", 2, 10, 19, 26),
                (AstTokenKind::Comment, "-- note", 2, 17, 26, 33),
                (AstTokenKind::Newline, "\n", 2, 24, 33, 34),
            ]
        );
        assert_eq!(
            lex_source("probe.bn", "   ").unwrap_err().message,
            "syntax error near 3..3"
        );
        assert_eq!(lex_source("probe.bn", "foo   ").unwrap()[0].end, 6);
        let unterminated = lex_source("probe.bn", "\"oops").unwrap();
        assert_eq!(unterminated[0].kind, AstTokenKind::Unknown);
        assert_eq!(unterminated[0].lexeme, "\"");
        assert_eq!(unterminated[1].lexeme, "oops");

        let unicode = lex_source("unicode.bn", "é x\n\"ž\"").unwrap();
        assert_eq!(unicode[0].kind, AstTokenKind::Unknown);
        assert_eq!(unicode[0].lexeme, "é");
        assert_eq!((unicode[0].start, unicode[0].end), (0, 3));
        assert_eq!((unicode[1].column, unicode[1].start), (3, 3));
        assert_eq!((unicode[3].line, unicode[3].column), (2, 1));
    }
}
