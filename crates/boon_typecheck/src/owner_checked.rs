use crate::{OwnerSyntaxInput, OwnerSyntaxProjectionError};
use boon_checked::{
    OwnerCheckedConstructionReceipt, OwnerCheckedDomainCount, OwnerCheckedReceiptSet,
    OwnerCheckedRelocation, OwnerCheckedRowDomain, OwnerCheckedRowReceipt, OwnerExpressionId,
    OwnerExpressionRef, OwnerRelocationSpan, OwnerRelocationTarget, OwnerStatementChild,
    OwnerStatementId,
};
use boon_syntax::{AstExprKind, AstStatementKind, StableCheckOwnerKey, StableExpressionKey};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const OWNER_CHECKED_STABLE_ROW_DOMAIN_V1: &[u8] = b"boon.owner-checked-stable-row.v1\0";
const OWNER_CHECKED_ROW_PAYLOAD_DOMAIN_V1: &[u8] = b"boon.owner-checked-row-payload.v1\0";
const OWNER_CHECKED_LOCAL_CONTENT_DOMAIN_V1: &[u8] = b"boon.owner-checked-local-content.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerCheckedReceiptError {
    message: String,
}

impl OwnerCheckedReceiptError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerCheckedReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerCheckedReceiptError {}

#[derive(Clone, Debug)]
struct PendingOwnerCheckedRowReceipt {
    domain: OwnerCheckedRowDomain,
    row: u32,
    stable_key_digest_v1: [u8; 32],
    payload_digest_v1: [u8; 32],
    relocations: Vec<OwnerRelocationTarget>,
}

/// Construction-time receipt sink for one checked owner.
///
/// Callers pass stable identity and an already-normalized semantic payload at
/// the point where they append the authoritative row. Revision-local dense IDs,
/// source spans, timings, allocation counts, and other telemetry are absent
/// from this API by design. `finish` orders rows by typed domain/local ordinal,
/// closes each relocation span once, and hashes the compact proof material; it
/// never scans the completed rich row tables.
#[derive(Default)]
pub struct OwnerCheckedReceiptSink {
    row_counts: BTreeMap<OwnerCheckedRowDomain, u32>,
    stable_key_bytes: BTreeMap<[u8; 32], Vec<u8>>,
    pending: Vec<PendingOwnerCheckedRowReceipt>,
    hash_scratch: Vec<u8>,
}

impl OwnerCheckedReceiptSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record<StableKey: Serialize, NormalizedPayload: Serialize>(
        &mut self,
        domain: OwnerCheckedRowDomain,
        stable_key: &StableKey,
        normalized_payload: &NormalizedPayload,
        relocations: impl IntoIterator<Item = OwnerRelocationTarget>,
    ) -> Result<u32, OwnerCheckedReceiptError> {
        let stable_key_digest_v1 = boon_contract::canonical_serde_hash_v1_with_buffer(
            OWNER_CHECKED_STABLE_ROW_DOMAIN_V1,
            &(domain, stable_key),
            &mut self.hash_scratch,
        )
        .map_err(|error| {
            OwnerCheckedReceiptError::new(format!(
                "failed to hash {domain:?} stable row identity: {error}"
            ))
        })?;
        let canonical_stable_key = self.hash_scratch.clone();
        let payload_digest_v1 = boon_contract::canonical_serde_hash_v1_with_buffer(
            OWNER_CHECKED_ROW_PAYLOAD_DOMAIN_V1,
            &(domain, normalized_payload),
            &mut self.hash_scratch,
        )
        .map_err(|error| {
            OwnerCheckedReceiptError::new(format!(
                "failed to hash {domain:?} normalized row payload: {error}"
            ))
        })?;
        let row = *self.row_counts.get(&domain).unwrap_or(&0);
        let next = row.checked_add(1).ok_or_else(|| {
            OwnerCheckedReceiptError::new(format!("{domain:?} owner row count exceeds u32"))
        })?;

        match self.stable_key_bytes.entry(stable_key_digest_v1) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(canonical_stable_key);
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if entry.get() == &canonical_stable_key =>
            {
                return Err(OwnerCheckedReceiptError::new(format!(
                    "duplicate {domain:?} stable row identity"
                )));
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                return Err(OwnerCheckedReceiptError::new(format!(
                    "owner checked stable-row digest collision in {domain:?}"
                )));
            }
        }
        self.row_counts.insert(domain, next);

        let mut relocations = relocations.into_iter().collect::<Vec<_>>();
        relocations.sort();
        relocations.dedup();
        self.pending.push(PendingOwnerCheckedRowReceipt {
            domain,
            row,
            stable_key_digest_v1,
            payload_digest_v1,
            relocations,
        });
        Ok(row)
    }

    pub fn finish(mut self) -> Result<OwnerCheckedReceiptSet, OwnerCheckedReceiptError> {
        self.pending
            .sort_unstable_by_key(|receipt| (receipt.domain, receipt.row));

        let mut row_receipts = Vec::with_capacity(self.pending.len());
        let mut relocations = Vec::new();
        for pending in self.pending {
            let start = checked_receipt_u32(relocations.len(), "owner relocation start")?;
            let len = checked_receipt_u32(pending.relocations.len(), "owner row relocation count")?;
            start.checked_add(len).ok_or_else(|| {
                OwnerCheckedReceiptError::new("owner relocation span exceeds u32")
            })?;
            relocations.extend(pending.relocations.into_iter().map(|target| {
                OwnerCheckedRelocation {
                    source_domain: pending.domain,
                    source_row: pending.row,
                    target,
                }
            }));
            row_receipts.push(OwnerCheckedRowReceipt {
                domain: pending.domain,
                row: pending.row,
                stable_key_digest_v1: pending.stable_key_digest_v1,
                payload_digest_v1: pending.payload_digest_v1,
                relocations: OwnerRelocationSpan { start, len },
            });
        }

        let domain_counts = self
            .row_counts
            .into_iter()
            .map(|(domain, rows)| OwnerCheckedDomainCount { domain, rows })
            .collect::<Vec<_>>();
        let row_receipt_count =
            checked_receipt_u32(row_receipts.len(), "owner checked row receipt count")?;
        let relocation_count =
            checked_receipt_u32(relocations.len(), "owner checked relocation count")?;
        let local_content_digest_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_CHECKED_LOCAL_CONTENT_DOMAIN_V1,
            &(&domain_counts, &row_receipts, &relocations),
        )
        .map_err(|error| {
            OwnerCheckedReceiptError::new(format!(
                "failed to hash owner checked construction receipt: {error}"
            ))
        })?;

        Ok(OwnerCheckedReceiptSet {
            construction: OwnerCheckedConstructionReceipt {
                domain_counts: domain_counts.into_boxed_slice(),
                row_receipt_count,
                relocation_count,
                local_content_digest_v1,
            },
            row_receipts: row_receipts.into_boxed_slice(),
            relocations: relocations.into_boxed_slice(),
        })
    }
}

fn checked_receipt_u32(value: usize, context: &str) -> Result<u32, OwnerCheckedReceiptError> {
    u32::try_from(value)
        .map_err(|_| OwnerCheckedReceiptError::new(format!("{context} exceeds u32")))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSyntaxGraphError {
    message: String,
}

impl OwnerSyntaxGraphError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerSyntaxGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerSyntaxGraphError {}

impl From<OwnerSyntaxProjectionError> for OwnerSyntaxGraphError {
    fn from(error: OwnerSyntaxProjectionError) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSyntaxStatementNode {
    pub id: OwnerStatementId,
    pub parent: Option<OwnerStatementId>,
    pub child_index: u32,
    pub children: Box<[OwnerStatementChild]>,
    pub direct_value: Option<OwnerExpressionRef>,
    pub canonical_value: Option<OwnerExpressionRef>,
}

/// Validated structural index over one span-free owner syntax artifact.
///
/// The graph keeps authored child-owner boundaries in the same ordered lane as
/// local statements and derives the exact canonical statement value used by
/// checked lowering. It owns no types, declarations, diagnostics, or source
/// positions and therefore cannot be mistaken for a checked body result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSyntaxGraph {
    owner: StableCheckOwnerKey,
    roots: Box<[OwnerStatementChild]>,
    statements: Box<[OwnerSyntaxStatementNode]>,
    expression_inputs: Box<[Box<[OwnerExpressionRef]>]>,
}

impl OwnerSyntaxGraph {
    pub fn build(syntax: &OwnerSyntaxInput) -> Result<Self, OwnerSyntaxGraphError> {
        validate_expression_table(syntax)?;

        let mut attachments =
            BTreeMap::<Option<OwnerStatementId>, Vec<(u32, OwnerStatementChild)>>::new();
        let mut stable_statements = BTreeSet::new();
        for (index, statement) in syntax.statements.iter().enumerate() {
            let expected = checked_u32(index, "owner statement id")?;
            if statement.id != expected {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {:?} statement table is not dense at row {index}: found {}",
                    syntax.owner, statement.id
                )));
            }
            if statement.stable_key.source_unit_id != *syntax.owner.source_unit_id() {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {:?} statement {index} has a foreign stable source unit",
                    syntax.owner
                )));
            }
            if !stable_statements.insert(statement.stable_key.clone()) {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {:?} has duplicate stable statement identity {:?}",
                    syntax.owner, statement.stable_key
                )));
            }
            let id = OwnerStatementId(statement.id);
            let parent = statement.parent.map(OwnerStatementId);
            if let Some(parent) = parent
                && parent.0 >= statement.id
            {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {:?} statement {} has a non-preorder parent {}",
                    syntax.owner, statement.id, parent.0
                )));
            }
            attachments.entry(parent).or_default().push((
                statement.child_index,
                OwnerStatementChild::Local { statement: id },
            ));
        }
        for child in &syntax.child_owners {
            if child.owner.source_unit_id() != syntax.owner.source_unit_id() {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {:?} has a child boundary in another source unit: {:?}",
                    syntax.owner, child.owner
                )));
            }
            let parent = child.parent.map(OwnerStatementId);
            if let Some(parent) = parent
                && parent.0 as usize >= syntax.statements.len()
            {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {:?} child {:?} has missing parent {}",
                    syntax.owner, child.owner, parent.0
                )));
            }
            attachments.entry(parent).or_default().push((
                child.child_index,
                OwnerStatementChild::Owner {
                    owner: child.owner.clone(),
                },
            ));
        }
        for (parent, children) in &mut attachments {
            children.sort_by_key(|(child_index, _)| *child_index);
            for (expected, (actual, _)) in children.iter().enumerate() {
                let expected = checked_u32(expected, "owner child index")?;
                if *actual != expected {
                    return Err(OwnerSyntaxGraphError::new(format!(
                        "owner {:?} parent {:?} has missing or duplicate child slot: expected {expected}, found {actual}",
                        syntax.owner, parent
                    )));
                }
            }
        }

        let expression_inputs = syntax
            .expressions
            .iter()
            .map(|expression| {
                expression_references(&expression.kind)
                    .into_iter()
                    .chain(expression.linked_input)
                    .map(|reference| expression_reference(syntax, reference))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Vec::into_boxed_slice)
            })
            .collect::<Result<Vec<_>, OwnerSyntaxGraphError>>()?;
        validate_expression_acyclic(syntax, &expression_inputs)?;

        let roots = attachments
            .remove(&None)
            .unwrap_or_default()
            .into_iter()
            .map(|(_, child)| child)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let statements = syntax
            .statements
            .iter()
            .map(|statement| {
                let id = OwnerStatementId(statement.id);
                Ok(OwnerSyntaxStatementNode {
                    id,
                    parent: statement.parent.map(OwnerStatementId),
                    child_index: statement.child_index,
                    children: attachments
                        .remove(&Some(id))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(_, child)| child)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                    direct_value: statement
                        .expression
                        .map(|expression| expression_reference(syntax, expression))
                        .transpose()?,
                    canonical_value: None,
                })
            })
            .collect::<Result<Vec<_>, OwnerSyntaxGraphError>>()?;
        if !attachments.is_empty() {
            return Err(OwnerSyntaxGraphError::new(format!(
                "owner {:?} has statement attachments for missing parents",
                syntax.owner
            )));
        }

        let mut graph = Self {
            owner: syntax.owner.clone(),
            roots,
            statements: statements.into_boxed_slice(),
            expression_inputs: expression_inputs.into_boxed_slice(),
        };
        for index in 0..graph.statements.len() {
            let id = OwnerStatementId(checked_u32(index, "canonical statement id")?);
            graph.statements[index].canonical_value =
                graph.canonical_checked_statement_value(syntax, id);
        }
        Ok(graph)
    }

    pub fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn roots(&self) -> &[OwnerStatementChild] {
        &self.roots
    }

    pub fn statements(&self) -> &[OwnerSyntaxStatementNode] {
        &self.statements
    }

    pub fn statement(&self, id: OwnerStatementId) -> Option<&OwnerSyntaxStatementNode> {
        self.statements
            .get(id.0 as usize)
            .filter(|statement| statement.id == id)
    }

    pub fn expression_inputs(&self, id: OwnerExpressionId) -> Option<&[OwnerExpressionRef]> {
        self.expression_inputs.get(id.0 as usize).map(Box::as_ref)
    }

    pub fn stable_expression_key<'a>(
        &self,
        syntax: &'a OwnerSyntaxInput,
        expression: &'a OwnerExpressionRef,
    ) -> Option<&'a StableExpressionKey> {
        match expression {
            OwnerExpressionRef::Local { expression } => syntax
                .expressions
                .get(expression.0 as usize)
                .map(|expression| &expression.stable_key),
            OwnerExpressionRef::Child { expression, .. } => Some(expression),
        }
    }

    fn local_statement(&self, child: &OwnerStatementChild) -> Option<OwnerStatementId> {
        match child {
            OwnerStatementChild::Local { statement } => Some(*statement),
            OwnerStatementChild::Owner { .. } => None,
        }
    }

    fn child_owner_value(
        &self,
        syntax: &OwnerSyntaxInput,
        child: &OwnerStatementChild,
    ) -> Option<OwnerExpressionRef> {
        let OwnerStatementChild::Owner { owner } = child else {
            return None;
        };
        syntax
            .child_owners
            .iter()
            .find(|child| &child.owner == owner)
            .and_then(|child| child.result_expression.clone())
            .map(|expression| OwnerExpressionRef::Child {
                owner: owner.clone(),
                expression,
            })
    }

    fn local_expression<'a>(
        &self,
        syntax: &'a OwnerSyntaxInput,
        expression: &OwnerExpressionRef,
    ) -> Option<(OwnerExpressionId, &'a crate::OwnerExpressionInput)> {
        let OwnerExpressionRef::Local { expression } = expression else {
            return None;
        };
        syntax
            .expressions
            .get(expression.0 as usize)
            .map(|value| (*expression, value))
    }

    fn expression_is_pipeline_continuation(
        &self,
        syntax: &OwnerSyntaxInput,
        expression: &OwnerExpressionRef,
    ) -> bool {
        self.local_expression(syntax, expression)
            .is_some_and(|(_, expression)| expression.linked_input.is_some())
    }

    fn expression_contains(
        &self,
        root: &OwnerExpressionRef,
        needle: &OwnerExpressionRef,
        visited: &mut BTreeSet<OwnerExpressionId>,
    ) -> bool {
        if root == needle {
            return true;
        }
        let OwnerExpressionRef::Local { expression } = root else {
            return false;
        };
        if !visited.insert(*expression) {
            return false;
        }
        self.expression_inputs(*expression).is_some_and(|inputs| {
            inputs
                .iter()
                .any(|input| self.expression_contains(input, needle, visited))
        })
    }

    fn collect_pipe_continuations(
        &self,
        syntax: &OwnerSyntaxInput,
        statement: OwnerStatementId,
        expressions: &mut Vec<OwnerExpressionRef>,
    ) {
        let Some(statement) = self.statement(statement) else {
            return;
        };
        for child in &statement.children {
            let Some(child_id) = self.local_statement(child) else {
                continue;
            };
            let Some(child_syntax) = syntax.statements.get(child_id.0 as usize) else {
                continue;
            };
            let Some(value) = self
                .statement(child_id)
                .and_then(|child| child.direct_value.clone())
            else {
                continue;
            };
            if !matches!(child_syntax.kind, AstStatementKind::Expression)
                || !self.expression_is_pipeline_continuation(syntax, &value)
            {
                continue;
            }
            expressions.push(value);
            self.collect_pipe_continuations(syntax, child_id, expressions);
        }
    }

    fn collect_following_sibling_pipe_continuations(
        &self,
        syntax: &OwnerSyntaxInput,
        siblings: &[OwnerStatementChild],
        start: usize,
        expressions: &mut Vec<OwnerExpressionRef>,
    ) {
        for child in siblings.iter().skip(start) {
            let Some(child_id) = self.local_statement(child) else {
                break;
            };
            let Some(child_syntax) = syntax.statements.get(child_id.0 as usize) else {
                break;
            };
            let Some(value) = self
                .statement(child_id)
                .and_then(|child| child.direct_value.clone())
            else {
                break;
            };
            if !matches!(child_syntax.kind, AstStatementKind::Expression)
                || !self.expression_is_pipeline_continuation(syntax, &value)
            {
                break;
            }
            expressions.push(value);
            self.collect_pipe_continuations(syntax, child_id, expressions);
        }
    }

    fn expression_sequence_is_pipeline(
        &self,
        syntax: &OwnerSyntaxInput,
        expressions: &[OwnerExpressionRef],
    ) -> bool {
        expressions.len() > 1
            && !self.expression_is_pipeline_continuation(syntax, &expressions[0])
            && expressions
                .iter()
                .skip(1)
                .all(|expression| self.expression_is_pipeline_continuation(syntax, expression))
    }

    fn statement_pipeline_final(
        &self,
        syntax: &OwnerSyntaxInput,
        statement: OwnerStatementId,
    ) -> Option<OwnerExpressionRef> {
        let statement = self.statement(statement)?;
        let first = statement.direct_value.clone()?;
        if self.expression_is_pipeline_continuation(syntax, &first)
            || !statement.children.iter().any(|child| {
                self.local_statement(child).is_some_and(|child| {
                    syntax
                        .statements
                        .get(child.0 as usize)
                        .is_some_and(|child| matches!(child.kind, AstStatementKind::Expression))
                        && self
                            .statement(child)
                            .and_then(|child| child.direct_value.as_ref())
                            .is_some_and(|value| {
                                self.expression_is_pipeline_continuation(syntax, value)
                            })
                })
            })
        {
            return None;
        }
        let mut expressions = vec![first];
        self.collect_pipe_continuations(syntax, statement.id, &mut expressions);
        self.expression_sequence_is_pipeline(syntax, &expressions)
            .then(|| {
                expressions
                    .pop()
                    .expect("pipeline has at least two expressions")
            })
    }

    fn first_child_expression(&self, statement: OwnerStatementId) -> Option<OwnerExpressionRef> {
        let statement = self.statement(statement)?;
        for child in &statement.children {
            // This helper is called only with a validated graph; the child
            // value itself is resolved by callers that also retain syntax.
            let Some(child) = self.local_statement(child) else {
                continue;
            };
            if let Some(value) = self
                .statement(child)
                .and_then(|child| child.direct_value.clone())
            {
                return Some(value);
            }
            if let Some(value) = self.first_child_expression(child) {
                return Some(value);
            }
        }
        None
    }

    fn direct_statement_value(
        &self,
        syntax: &OwnerSyntaxInput,
        statement: OwnerStatementId,
    ) -> Option<OwnerExpressionRef> {
        if let Some(value) = self.statement_pipeline_final(syntax, statement) {
            return Some(value);
        }
        let statement_node = self.statement(statement)?;
        if statement_node.direct_value.is_some() {
            return statement_node.direct_value.clone();
        }
        let mut expression_children = Vec::new();
        for child in &statement_node.children {
            if let Some(value) = self.child_owner_value(syntax, child) {
                expression_children.push(value);
                continue;
            }
            let Some(child) = self.local_statement(child) else {
                continue;
            };
            let Some(child_syntax) = syntax.statements.get(child.0 as usize) else {
                continue;
            };
            let include = matches!(
                child_syntax.kind,
                AstStatementKind::Expression | AstStatementKind::Hold { .. }
            ) || matches!(
                &child_syntax.kind,
                AstStatementKind::List { field: None, .. }
            );
            if !include {
                continue;
            }
            if let Some(value) = self
                .statement(child)
                .and_then(|child| child.direct_value.clone())
                .or_else(|| self.first_child_expression(child))
            {
                expression_children.push(value);
            }
        }
        match expression_children.as_slice() {
            [] => None,
            [single] => Some(single.clone()),
            many if self.expression_sequence_is_pipeline(syntax, many) => many.last().cloned(),
            _ => None,
        }
    }

    fn statement_pipeline_final_containing(
        &self,
        syntax: &OwnerSyntaxInput,
        statements: &[OwnerStatementChild],
        needle: &OwnerExpressionRef,
    ) -> Option<OwnerExpressionRef> {
        for (index, child) in statements.iter().enumerate() {
            let Some(statement_id) = self.local_statement(child) else {
                continue;
            };
            let statement = self.statement(statement_id)?;
            let contains = statement.direct_value.as_ref().is_some_and(|root| {
                root == needle || self.expression_contains(root, needle, &mut BTreeSet::new())
            });
            if contains {
                if let Some(value) = self.statement_pipeline_final(syntax, statement_id) {
                    return Some(value);
                }
                let mut expressions = statement.direct_value.iter().cloned().collect::<Vec<_>>();
                self.collect_pipe_continuations(syntax, statement_id, &mut expressions);
                self.collect_following_sibling_pipe_continuations(
                    syntax,
                    statements,
                    index + 1,
                    &mut expressions,
                );
                if self.expression_sequence_is_pipeline(syntax, &expressions) {
                    return expressions.last().cloned();
                }
                let mut continuations = Vec::new();
                self.collect_pipe_continuations(syntax, statement_id, &mut continuations);
                self.collect_following_sibling_pipe_continuations(
                    syntax,
                    statements,
                    index + 1,
                    &mut continuations,
                );
                if let Some(value) = continuations.last() {
                    return Some(value.clone());
                }
            }
            if let Some(value) =
                self.statement_pipeline_final_containing(syntax, &statement.children, needle)
            {
                return Some(value);
            }
        }
        None
    }

    fn canonical_statement_value_in(
        &self,
        syntax: &OwnerSyntaxInput,
        statements: &[OwnerStatementChild],
        statement: OwnerStatementId,
    ) -> Option<OwnerExpressionRef> {
        let statement_node = self.statement(statement)?;
        statement_node
            .direct_value
            .as_ref()
            .and_then(|value| self.statement_pipeline_final_containing(syntax, statements, value))
            .or_else(|| self.direct_statement_value(syntax, statement))
    }

    fn statement_is_source_pipe_continuation(
        &self,
        syntax: &OwnerSyntaxInput,
        statement: OwnerStatementId,
    ) -> bool {
        let Some((_, expression)) = self
            .statement(statement)
            .and_then(|statement| statement.direct_value.as_ref())
            .and_then(|value| self.local_expression(syntax, value))
        else {
            return false;
        };
        matches!(
            &expression.kind,
            AstExprKind::Pipe { op, .. }
                if op == "SOURCE" && expression.linked_input.is_some()
        )
    }

    fn canonical_statement_value_at(
        &self,
        syntax: &OwnerSyntaxInput,
        statements: &[OwnerStatementChild],
        index: usize,
    ) -> Option<OwnerExpressionRef> {
        let child = statements.get(index)?;
        if let Some(value) = self.child_owner_value(syntax, child) {
            let mut expressions = vec![value.clone()];
            self.collect_following_sibling_pipe_continuations(
                syntax,
                statements,
                index + 1,
                &mut expressions,
            );
            return self
                .expression_sequence_is_pipeline(syntax, &expressions)
                .then(|| expressions.last().expect("pipeline has a result").clone())
                .or(Some(value));
        }
        let statement_id = self.local_statement(child)?;
        let statement = self.statement(statement_id)?;
        let pipeline_value = statement.direct_value.as_ref().and_then(|direct| {
            self.statement_pipeline_final(syntax, statement_id)
                .or_else(|| {
                    let mut continuations = Vec::new();
                    self.collect_pipe_continuations(syntax, statement_id, &mut continuations);
                    self.collect_following_sibling_pipe_continuations(
                        syntax,
                        statements,
                        index + 1,
                        &mut continuations,
                    );
                    let continuation_value = continuations.last().cloned();
                    let mut expressions = Vec::with_capacity(continuations.len() + 1);
                    expressions.push(direct.clone());
                    expressions.extend(continuations);
                    self.expression_sequence_is_pipeline(syntax, &expressions)
                        .then(|| {
                            expressions
                                .last()
                                .expect("pipeline has at least two expressions")
                                .clone()
                        })
                        .or(continuation_value)
                })
        });
        pipeline_value.or_else(|| self.direct_statement_value(syntax, statement_id))
    }

    fn canonical_block_value(
        &self,
        syntax: &OwnerSyntaxInput,
        statements: &[OwnerStatementChild],
    ) -> Option<OwnerExpressionRef> {
        let mut result = None;
        for (index, child) in statements.iter().enumerate() {
            if self
                .local_statement(child)
                .is_some_and(|statement| {
                    self.statement_is_source_pipe_continuation(syntax, statement)
                })
                && result.is_some()
            {
                continue;
            }
            if let Some(value) = self.canonical_statement_value_at(syntax, statements, index) {
                result = Some(value);
            }
        }
        result
    }

    fn canonical_checked_statement_value(
        &self,
        syntax: &OwnerSyntaxInput,
        statement: OwnerStatementId,
    ) -> Option<OwnerExpressionRef> {
        let statement_node = self.statement(statement)?;
        let statement_syntax = syntax.statements.get(statement.0 as usize)?;
        if statement_node.direct_value.as_ref().is_some_and(|value| {
            self.local_expression(syntax, value)
                .is_some_and(|(_, expression)| matches!(expression.kind, AstExprKind::Then { .. }))
        }) {
            return statement_node.direct_value.clone();
        }
        if matches!(statement_syntax.kind, AstStatementKind::Function { .. }) {
            self.canonical_block_value(syntax, &statement_node.children)
                .or_else(|| {
                    self.canonical_statement_value_in(syntax, &statement_node.children, statement)
                })
        } else {
            self.canonical_statement_value_in(syntax, &statement_node.children, statement)
                .or_else(|| self.canonical_block_value(syntax, &statement_node.children))
        }
    }
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerSyntaxGraphError> {
    u32::try_from(value)
        .map_err(|_| OwnerSyntaxGraphError::new(format!("{context} exceeds the u32 bound")))
}

fn expression_reference(
    syntax: &OwnerSyntaxInput,
    reference: u32,
) -> Result<OwnerExpressionRef, OwnerSyntaxGraphError> {
    let reference = reference as usize;
    if reference < syntax.expressions.len() {
        return Ok(OwnerExpressionRef::Local {
            expression: OwnerExpressionId(reference as u32),
        });
    }
    let external = syntax.external_expression(reference).ok_or_else(|| {
        OwnerSyntaxGraphError::new(format!(
            "owner {:?} expression reference {reference} is out of bounds",
            syntax.owner
        ))
    })?;
    Ok(OwnerExpressionRef::Child {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn validate_expression_table(syntax: &OwnerSyntaxInput) -> Result<(), OwnerSyntaxGraphError> {
    let mut stable_expressions = BTreeSet::new();
    for (index, expression) in syntax.expressions.iter().enumerate() {
        if expression.stable_key.source_unit_id != *syntax.owner.source_unit_id() {
            return Err(OwnerSyntaxGraphError::new(format!(
                "owner {:?} expression {index} has a foreign stable source unit",
                syntax.owner
            )));
        }
        if !stable_expressions.insert(expression.stable_key.clone()) {
            return Err(OwnerSyntaxGraphError::new(format!(
                "owner {:?} has duplicate stable expression identity {:?}",
                syntax.owner, expression.stable_key
            )));
        }
        for reference in expression_references(&expression.kind)
            .into_iter()
            .chain(expression.linked_input)
        {
            let _ = expression_reference(syntax, reference)?;
        }
        if let Some(selector) = expression.pattern_selector {
            let _ = expression_reference(syntax, selector)?;
        }
        if let AstExprKind::Block { bindings, .. } = &expression.kind {
            for binding in bindings {
                if binding.statement >= syntax.statements.len() {
                    return Err(OwnerSyntaxGraphError::new(format!(
                        "owner {:?} expression {index} block binding references missing statement {}",
                        syntax.owner, binding.statement
                    )));
                }
            }
        }
    }
    let mut external = BTreeSet::new();
    for expression in &syntax.external_expressions {
        if expression.owner.source_unit_id() != syntax.owner.source_unit_id() {
            return Err(OwnerSyntaxGraphError::new(format!(
                "owner {:?} external expression belongs to another source unit: {:?}",
                syntax.owner, expression.owner
            )));
        }
        if !external.insert((expression.owner.clone(), expression.expression.clone())) {
            return Err(OwnerSyntaxGraphError::new(format!(
                "owner {:?} has duplicate external expression {:?}",
                syntax.owner, expression.expression
            )));
        }
    }
    Ok(())
}

fn validate_expression_acyclic(
    syntax: &OwnerSyntaxInput,
    inputs: &[Box<[OwnerExpressionRef]>],
) -> Result<(), OwnerSyntaxGraphError> {
    fn visit(
        owner: &StableCheckOwnerKey,
        expression: OwnerExpressionId,
        inputs: &[Box<[OwnerExpressionRef]>],
        states: &mut [u8],
    ) -> Result<(), OwnerSyntaxGraphError> {
        let state = states.get_mut(expression.0 as usize).ok_or_else(|| {
            OwnerSyntaxGraphError::new(format!(
                "owner {owner:?} expression {} is out of bounds",
                expression.0
            ))
        })?;
        match *state {
            2 => return Ok(()),
            1 => {
                return Err(OwnerSyntaxGraphError::new(format!(
                    "owner {owner:?} expression graph contains a cycle at {}",
                    expression.0
                )));
            }
            _ => *state = 1,
        }
        for input in inputs.get(expression.0 as usize).into_iter().flatten() {
            if let OwnerExpressionRef::Local { expression } = input {
                visit(owner, *expression, inputs, states)?;
            }
        }
        states[expression.0 as usize] = 2;
        Ok(())
    }

    let mut states = vec![0u8; syntax.expressions.len()];
    for expression in 0..syntax.expressions.len() {
        visit(
            &syntax.owner,
            OwnerExpressionId(checked_u32(expression, "owner expression id")?),
            inputs,
            &mut states,
        )?;
    }
    Ok(())
}

fn expression_references(kind: &AstExprKind) -> Vec<u32> {
    let mut references = Vec::new();
    let mut push = |reference: usize| {
        if let Ok(reference) = u32::try_from(reference) {
            references.push(reference);
        } else {
            references.push(u32::MAX);
        }
    };
    match kind {
        AstExprKind::TextTemplate { segments } => {
            for segment in segments {
                if let boon_syntax::AstTextSegment::Dynamic { value } = segment {
                    push(*value);
                }
            }
        }
        AstExprKind::TaggedObject { fields, .. } | AstExprKind::Object(fields) => {
            for field in fields {
                push(field.value);
            }
        }
        AstExprKind::Flush { payload } => payload.iter().for_each(|value| push(*value)),
        AstExprKind::Call { args, pass, .. } => {
            for argument in args {
                push(argument.value);
            }
            if let Some(pass) = pass {
                push(pass.value);
            }
        }
        AstExprKind::Pipe {
            input,
            args,
            pass,
            arms,
            ..
        } => {
            push(*input);
            for argument in args {
                push(argument.value);
            }
            if let Some(pass) = pass {
                push(pass.value);
            }
            arms.iter().for_each(|arm| push(*arm));
        }
        AstExprKind::Draining { input } => push(*input),
        AstExprKind::Hold { initial, .. } => push(*initial),
        AstExprKind::Latest { branches } => branches.iter().for_each(|branch| push(*branch)),
        AstExprKind::When { input, arms } => {
            push(*input);
            arms.iter().for_each(|arm| push(*arm));
        }
        AstExprKind::Then { input, output } => {
            push(*input);
            output.iter().for_each(|output| push(*output));
        }
        AstExprKind::Infix { left, right, .. } => {
            push(*left);
            push(*right);
        }
        AstExprKind::MatchArm { output, .. } => {
            output.iter().for_each(|output| push(*output));
        }
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                push(binding.value);
            }
            result.iter().for_each(|result| push(*result));
        }
        AstExprKind::ListLiteral { items, .. }
        | AstExprKind::BytesLiteral { items, .. }
        | AstExprKind::SetLiteral { items } => items.iter().for_each(|item| push(*item)),
        AstExprKind::Arrow { left, output, .. } => {
            push(*left);
            output.iter().for_each(|output| push(*output));
        }
        AstExprKind::MapEntry { key, value } => {
            push(*key);
            push(*value);
        }
        AstExprKind::MapLiteral { entries } => entries.iter().for_each(|entry| push(*entry)),
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
    references
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check_project_diagnostics_program_profiled_with_external_types, project_owner_syntax_input,
    };
    use boon_checked::{ExternalTypeEnvironment, ProgramRole};
    use boon_parser::{ProjectSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys};
    use std::sync::Arc;

    #[test]
    fn receipt_sink_closes_canonical_relocation_spans() {
        let project = project("app/RUN.bn", "value: 1\n");
        let owner = project.stable_check_owner_keys().next().unwrap();
        let context = OwnerRelocationTarget::ContextFormal {
            owner: owner.clone(),
        };
        let root = OwnerRelocationTarget::ProjectRootScope;
        let mut sink = OwnerCheckedReceiptSink::new();
        assert_eq!(
            sink.record(
                OwnerCheckedRowDomain::Expression,
                &"expression-key",
                &("number", 1u8),
                [root.clone(), context.clone(), root.clone()],
            )
            .unwrap(),
            0
        );
        assert_eq!(
            sink.record(OwnerCheckedRowDomain::Scope, &"scope-key", &"root", [],)
                .unwrap(),
            0
        );

        let sealed = sink.finish().unwrap();
        assert_eq!(
            sealed.construction.domain_counts.as_ref(),
            [
                OwnerCheckedDomainCount {
                    domain: OwnerCheckedRowDomain::Scope,
                    rows: 1,
                },
                OwnerCheckedDomainCount {
                    domain: OwnerCheckedRowDomain::Expression,
                    rows: 1,
                },
            ]
        );
        assert_eq!(sealed.construction.row_receipt_count, 2);
        assert_eq!(sealed.construction.relocation_count, 2);
        assert_eq!(
            sealed.row_receipts[0].relocations,
            OwnerRelocationSpan { start: 0, len: 0 }
        );
        assert_eq!(
            sealed.row_receipts[1].relocations,
            OwnerRelocationSpan { start: 0, len: 2 }
        );
        let mut expected_targets = vec![context, root];
        expected_targets.sort();
        assert_eq!(
            sealed
                .relocations
                .iter()
                .map(|relocation| relocation.target.clone())
                .collect::<Vec<_>>(),
            expected_targets
        );
        assert!(sealed.relocations.iter().all(|relocation| {
            relocation.source_domain == OwnerCheckedRowDomain::Expression
                && relocation.source_row == 0
        }));
        assert_eq!(
            sealed.row_receipts[1].relocations.checked_range().unwrap(),
            0..2
        );
    }

    #[test]
    fn receipt_sink_rejects_duplicate_stable_rows() {
        let mut sink = OwnerCheckedReceiptSink::new();
        sink.record(OwnerCheckedRowDomain::Statement, &"same-key", &"first", [])
            .unwrap();
        let error = sink
            .record(OwnerCheckedRowDomain::Statement, &"same-key", &"second", [])
            .unwrap_err();
        assert!(error.to_string().contains("duplicate Statement stable row"));
    }

    #[test]
    fn receipt_digest_excludes_dense_ids_and_telemetry_by_construction() {
        #[derive(Serialize)]
        struct NormalizedPayload<'a> {
            kind: &'a str,
            value: u32,
        }

        fn seal(
            ignored_dense_id: u32,
            ignored_work_counter: u64,
            value: u32,
        ) -> OwnerCheckedReceiptSet {
            let _telemetry = (ignored_dense_id, ignored_work_counter);
            let mut sink = OwnerCheckedReceiptSink::new();
            sink.record(
                OwnerCheckedRowDomain::Expression,
                &"stable-expression",
                &NormalizedPayload {
                    kind: "number",
                    value,
                },
                [],
            )
            .unwrap();
            sink.finish().unwrap()
        }

        let first = seal(1, 10, 7);
        let renumbered_and_remeasured = seal(99, 999_999, 7);
        assert_eq!(first, renumbered_and_remeasured);
        assert_ne!(
            first.construction.local_content_digest_v1,
            seal(1, 10, 8).construction.local_content_digest_v1
        );
    }

    fn project(path: &str, source: &str) -> ProjectSyntaxSnapshot {
        let parsed = parse_project_source_unit(path, source).unwrap();
        let source_unit_id = parsed.source_unit_id.clone();
        let link_key = project_unit_link_keys(
            path,
            [(source_unit_id.clone(), parsed.declared_functions.clone())],
        )
        .unwrap()
        .remove(&source_unit_id)
        .unwrap();
        let unit = parsed.into_unit_syntax_snapshot(link_key).unwrap();
        ProjectSyntaxSnapshot::from_unit_snapshots(path, vec![Arc::new(unit)]).unwrap()
    }

    fn owner_graphs(
        project: &ProjectSyntaxSnapshot,
    ) -> BTreeMap<StableCheckOwnerKey, (OwnerSyntaxInput, OwnerSyntaxGraph)> {
        project
            .stable_check_owner_keys()
            .map(|owner| {
                let syntax =
                    project_owner_syntax_input(project.owner_view(&owner).unwrap()).unwrap();
                let graph = OwnerSyntaxGraph::build(&syntax).unwrap();
                (owner, (syntax, graph))
            })
            .collect()
    }

    fn local_statement_for_key(
        syntax: &OwnerSyntaxInput,
        stable_key: &boon_syntax::StableStatementKey,
    ) -> OwnerStatementId {
        syntax
            .statements
            .iter()
            .find(|statement| &statement.stable_key == stable_key)
            .map(|statement| OwnerStatementId(statement.id))
            .unwrap()
    }

    #[test]
    fn graph_matches_monolithic_statement_values_and_child_weaving() {
        let project = project(
            "app/RUN.bn",
            r#"
store: [
    title:
        TEXT { hello }
        |> Text/trim()
    nested: [
        count: 1
    ]
]

FUNCTION helper(input) {
    input
    |> Text/trim()
}

result: helper(input: store.title)
"#,
        );
        let graphs = owner_graphs(&project);
        let (output, _) = check_project_diagnostics_program_profiled_with_external_types(
            &project,
            &ExternalTypeEnvironment::empty(ProgramRole::Client),
        );
        let checked = output.checked_program_fields().unwrap();

        assert_eq!(checked.statements.len(), project.statement_count());
        for checked_statement in &checked.statements {
            let syntax_id = project
                .statement_id_for_slot(checked_statement.id.0 as usize)
                .unwrap();
            let owner = project.stable_check_owner_for_statement(syntax_id).unwrap();
            let stable_key = project.stable_statement_key(syntax_id).unwrap();
            let (syntax, graph) = &graphs[&owner];
            let local = local_statement_for_key(syntax, &stable_key);
            let statement = graph.statement(local).unwrap();

            let expected_value = checked_statement.value.map(|expression| {
                let syntax_id = project
                    .expression_id_for_slot(expression.0 as usize)
                    .unwrap();
                project.stable_expression_key(syntax_id).unwrap()
            });
            let actual_value = statement
                .canonical_value
                .as_ref()
                .and_then(|expression| graph.stable_expression_key(syntax, expression))
                .cloned();
            assert_eq!(
                actual_value, expected_value,
                "canonical value differs for {stable_key:?}"
            );

            let expected_children = checked_statement
                .children
                .iter()
                .map(|child| {
                    let syntax_id = project.statement_id_for_slot(child.0 as usize).unwrap();
                    let child_owner = project.stable_check_owner_for_statement(syntax_id).unwrap();
                    if child_owner == owner {
                        let key = project.stable_statement_key(syntax_id).unwrap();
                        OwnerStatementChild::Local {
                            statement: local_statement_for_key(syntax, &key),
                        }
                    } else {
                        OwnerStatementChild::Owner { owner: child_owner }
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                statement.children.as_ref(),
                expected_children,
                "child weaving differs for {stable_key:?}"
            );
        }
    }

    #[test]
    fn graph_identity_ignores_payload_spans_and_unrelated_siblings() {
        fn helper(source: &str) -> (OwnerSyntaxInput, OwnerSyntaxGraph) {
            let project = project("app/RUN.bn", source);
            let owner = project
                .stable_check_owner_keys()
                .find(|owner| {
                    matches!(
                        owner,
                        StableCheckOwnerKey::Item(owner)
                            if owner.item_route.segments().last().is_some_and(|segment| segment.names == ["helper"])
                    )
                })
                .unwrap();
            let syntax = project_owner_syntax_input(project.owner_view(&owner).unwrap()).unwrap();
            let graph = OwnerSyntaxGraph::build(&syntax).unwrap();
            (syntax, graph)
        }

        let (before_syntax, before) =
            helper("FUNCTION helper(input) {\n    value: TEXT { before }\n    value\n}\n");
        let (after_syntax, after) = helper(
            "unrelated: 99\nFUNCTION helper(input) {\n    value : TEXT { after }\n    value\n}\n",
        );
        assert_eq!(before, after);
        assert_ne!(
            before_syntax.fingerprint_v1(),
            after_syntax.fingerprint_v1()
        );
    }

    #[test]
    fn graph_rejects_corrupt_statement_and_expression_topology() {
        let project = project("app/RUN.bn", "value: 1\n");
        let syntax = project
            .stable_check_owner_keys()
            .find_map(|owner| {
                let syntax =
                    project_owner_syntax_input(project.owner_view(&owner).unwrap()).unwrap();
                (!syntax.statements.is_empty() && !syntax.expressions.is_empty()).then_some(syntax)
            })
            .unwrap();

        let mut corrupt_statement = syntax.clone();
        corrupt_statement.statements[0].id = 4;
        assert!(OwnerSyntaxGraph::build(&corrupt_statement).is_err());

        let mut corrupt_expression = syntax;
        corrupt_expression.expressions[0].linked_input = Some(0);
        assert!(OwnerSyntaxGraph::build(&corrupt_expression).is_err());
    }
}
