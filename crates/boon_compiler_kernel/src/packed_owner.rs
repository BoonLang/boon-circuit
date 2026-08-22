use crate::definition_code::Span32;
use crate::{
    KernelCollectionKind, KernelExternalExpression, KernelInheritedFormal, KernelOwnerBuildError,
    KernelOwnerEdgeRole, KernelOwnerInputEdge, KernelOwnerNodeKind, KernelPattern,
    KernelProjectProgramInput, KernelPureBuiltinKind, KernelRenderConstructorKind, KernelTypeRef,
    TypeTermArena,
};
use boon_checked::FlowMode;
use boon_contract::{PathId, ProjectTextSnapshot, SymbolId};
use std::hash::{Hash, Hasher};

/// Allocation-free pattern carried by the packed owner program.
///
/// Literal-bearing pattern data remains in the explicit presentation payload;
/// type solving needs only the structural tag/binding identity and field path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackedKernelPattern {
    Wildcard,
    Number,
    Text,
    Bits { width: u32 },
    Tag { name: SymbolId, fields: PathId },
    Binding { name: SymbolId },
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackedKernelRenderConstructorKind {
    Fixed(SymbolId),
    StripeDirection,
}

/// Fixed-width operation kind in the project-wide owner node column.
///
/// Every text coordinate is issued by the `ProjectTextSnapshot` stored beside
/// this program. Closed types are branded references into the one project type
/// arena. No variant owns a string, path vector, recursive type, or heap box.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackedKernelOwnerNodeKind {
    Known(KernelTypeRef),
    Source(KernelTypeRef),
    Absent,
    Text,
    TextTemplate,
    Number,
    Byte,
    Bits(u32),
    Tag(SymbolId),
    Record {
        tag: Option<SymbolId>,
    },
    Block,
    Collection {
        kind: KernelCollectionKind,
        capacity: Option<u32>,
    },
    MapEntry,
    FormalRead {
        formal: u32,
        fields: PathId,
    },
    ContextRead {
        formal: u32,
        fields: PathId,
    },
    LexicalRead {
        fields: PathId,
    },
    ValueRead {
        fields: PathId,
        mode_narrowing: Option<crate::KernelExpressionId>,
    },
    DerivedRead {
        fields: PathId,
    },
    PatternRead {
        pattern: PackedKernelPattern,
        fields: PathId,
    },
    CollectionItemRead,
    FreshOut,
    UserCall {
        target: crate::KernelOwnerId,
        inherited_formal: Option<KernelInheritedFormal>,
    },
    RenderConstructor {
        kind: PackedKernelRenderConstructorKind,
    },
    PureBuiltin {
        kind: KernelPureBuiltinKind,
    },
    FixedAbiCall {
        result: KernelTypeRef,
    },
    HostEffect {
        operation: SymbolId,
    },
    Latest,
    When,
    Then,
    Infix {
        operation: SymbolId,
    },
    Draining,
    Hold,
    MatchArm {
        pattern: PackedKernelPattern,
    },
    Arrow,
    Delimiter,
    Unknown,
    Flush,
    FieldProjection {
        field: SymbolId,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PackedKernelOwnerEdgeRole {
    RecordField { name: SymbolId, spread: bool },
    TextDynamic,
    BlockResult,
    CollectionItem,
    MapEntry,
    MapKey,
    MapValue,
    ReadProvider,
    CallArgument { ordinal: u32 },
    CallOutArgument { ordinal: u32 },
    AbiArgument { name: SymbolId },
    LatestBranch,
    WhenInput,
    WhenArm,
    ThenInput,
    ThenOutput,
    InfixLeft,
    InfixRight,
    DrainingInput,
    HoldInitial,
    HoldUpdate,
    MatchOutput,
    ArrowOutput,
    FlushPayload,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PackedKernelOwnerInputEdge {
    pub role: PackedKernelOwnerEdgeRole,
    pub expression: crate::KernelExpressionId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackedKernelOwnerNode {
    pub kind: PackedKernelOwnerNodeKind,
    inputs: Span32,
    pub mode: FlowMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PackedKernelOwnerProgram {
    nodes: Span32,
    pub formal_count: u32,
    external_expressions: Span32,
    pub result: crate::KernelExpressionId,
}

/// One immutable project-wide owner program store.
///
/// Owners, nodes, input edges, and external references each occupy one flat
/// allocation. Per-owner and per-node slices are represented by `Span32`.
#[cfg_attr(test, derive(Clone))]
#[derive(Debug, Eq, PartialEq)]
pub struct PackedKernelProjectProgram {
    text: ProjectTextSnapshot,
    owners: Box<[PackedKernelOwnerProgram]>,
    nodes: Box<[PackedKernelOwnerNode]>,
    edges: Box<[PackedKernelOwnerInputEdge]>,
    external_expressions: Box<[KernelExternalExpression]>,
}

#[derive(Clone, Copy, Debug)]
pub struct PackedKernelOwnerProgramRef<'a> {
    store: &'a PackedKernelProjectProgram,
    owner: &'a PackedKernelOwnerProgram,
}

#[derive(Clone, Copy, Debug)]
pub struct PackedKernelPathRef<'a> {
    text: &'a ProjectTextSnapshot,
    path: PathId,
}

pub struct PackedKernelPathIter<'a> {
    path: PackedKernelPathRef<'a>,
    next: u32,
}

impl PackedKernelProjectProgram {
    pub fn definition_count(&self) -> usize {
        self.owners.len()
    }

    pub fn owner(&self, owner: crate::KernelOwnerId) -> Option<PackedKernelOwnerProgramRef<'_>> {
        self.owner_at(owner.0 as usize)
    }

    pub fn owner_at(&self, owner: usize) -> Option<PackedKernelOwnerProgramRef<'_>> {
        Some(PackedKernelOwnerProgramRef {
            store: self,
            owner: self.owners.get(owner)?,
        })
    }

    pub fn owners(&self) -> impl ExactSizeIterator<Item = PackedKernelOwnerProgramRef<'_>> + '_ {
        self.owners
            .iter()
            .map(|owner| PackedKernelOwnerProgramRef { store: self, owner })
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn call_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| matches!(node.kind, PackedKernelOwnerNodeKind::UserCall { .. }))
            .count()
    }

    pub fn definition_result(
        &self,
        owner: crate::KernelOwnerId,
    ) -> Option<crate::KernelExpressionId> {
        self.owner(owner).map(PackedKernelOwnerProgramRef::result)
    }

    pub fn text(&self) -> &ProjectTextSnapshot {
        &self.text
    }

    pub fn symbol(&self, symbol: SymbolId) -> Option<&str> {
        self.text.symbol(symbol)
    }

    pub fn path(&self, path: PathId) -> Option<PackedKernelPathRef<'_>> {
        self.text.path_depth(path).map(|_| PackedKernelPathRef {
            text: &self.text,
            path,
        })
    }

    #[cfg(test)]
    pub(crate) fn corrupt_external_owner_for_test(
        &mut self,
        owner: usize,
        external: usize,
        target: crate::KernelOwnerId,
    ) -> Option<()> {
        let span = self.owners.get(owner)?.external_expressions;
        span.get_mut(&mut self.external_expressions)
            .and_then(|externals| externals.get_mut(external))?
            .owner = target;
        Some(())
    }

    #[cfg(test)]
    pub(crate) fn corrupt_result_for_test(
        &mut self,
        owner: usize,
        result: crate::KernelExpressionId,
    ) -> Option<()> {
        self.owners.get_mut(owner)?.result = result;
        Some(())
    }
}

impl<'a> PackedKernelPathRef<'a> {
    pub fn len(self) -> usize {
        self.text
            .path_depth(self.path)
            .expect("packed path belongs to its project text authority") as usize
    }

    pub fn is_empty(self) -> bool {
        self.path == PathId::ROOT
    }

    pub fn symbol_at(self, ordinal: usize) -> Option<SymbolId> {
        self.text
            .path_symbol_at(self.path, u32::try_from(ordinal).ok()?)
    }

    pub fn name_at(self, ordinal: usize) -> Option<&'a str> {
        self.text.symbol(self.symbol_at(ordinal)?)
    }

    pub fn iter(self) -> PackedKernelPathIter<'a> {
        PackedKernelPathIter {
            path: self,
            next: 0,
        }
    }

    pub const fn id(self) -> PathId {
        self.path
    }
}

impl<'a> Iterator for PackedKernelPathIter<'a> {
    type Item = SymbolId;

    fn next(&mut self) -> Option<Self::Item> {
        let symbol = self.path.symbol_at(self.next as usize)?;
        self.next += 1;
        Some(symbol)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.path.len().saturating_sub(self.next as usize);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for PackedKernelPathIter<'_> {}

impl PartialEq for PackedKernelPathRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .zip(other.iter())
                .all(|(left, right)| self.text.symbol(left) == other.text.symbol(right))
    }
}

impl Eq for PackedKernelPathRef<'_> {}

impl Hash for PackedKernelPathRef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for symbol in self.iter() {
            self.text
                .symbol(symbol)
                .expect("packed path symbol belongs to its text authority")
                .hash(state);
        }
    }
}

impl<'a> PackedKernelOwnerProgramRef<'a> {
    pub fn node_count(self) -> usize {
        self.owner.nodes.len as usize
    }

    pub fn nodes(self) -> &'a [PackedKernelOwnerNode] {
        self.owner
            .nodes
            .get(&self.store.nodes)
            .expect("validated packed owner node span remains in range")
    }

    pub const fn formal_count(self) -> u32 {
        self.owner.formal_count
    }

    pub fn external_expressions(self) -> &'a [KernelExternalExpression] {
        self.owner
            .external_expressions
            .get(&self.store.external_expressions)
            .expect("validated packed owner external-expression span remains in range")
    }

    pub const fn result(self) -> crate::KernelExpressionId {
        self.owner.result
    }

    pub fn node(self, expression: crate::KernelExpressionId) -> Option<&'a PackedKernelOwnerNode> {
        self.nodes().get(expression.0 as usize)
    }

    pub fn symbol(self, symbol: SymbolId) -> Option<&'a str> {
        self.store.symbol(symbol)
    }

    pub fn path(self, path: PathId) -> Option<PackedKernelPathRef<'a>> {
        self.store.path(path)
    }

    /// Materialize one edge role only at a compatibility/output boundary.
    /// Solver and indexing passes should keep using the packed role directly.
    pub fn materialize_edge_role(
        self,
        role: PackedKernelOwnerEdgeRole,
    ) -> Option<KernelOwnerEdgeRole> {
        Some(match role {
            PackedKernelOwnerEdgeRole::RecordField { name, spread } => {
                KernelOwnerEdgeRole::RecordField {
                    name: self.symbol(name)?.into(),
                    spread,
                }
            }
            PackedKernelOwnerEdgeRole::TextDynamic => KernelOwnerEdgeRole::TextDynamic,
            PackedKernelOwnerEdgeRole::BlockResult => KernelOwnerEdgeRole::BlockResult,
            PackedKernelOwnerEdgeRole::CollectionItem => KernelOwnerEdgeRole::CollectionItem,
            PackedKernelOwnerEdgeRole::MapEntry => KernelOwnerEdgeRole::MapEntry,
            PackedKernelOwnerEdgeRole::MapKey => KernelOwnerEdgeRole::MapKey,
            PackedKernelOwnerEdgeRole::MapValue => KernelOwnerEdgeRole::MapValue,
            PackedKernelOwnerEdgeRole::ReadProvider => KernelOwnerEdgeRole::ReadProvider,
            PackedKernelOwnerEdgeRole::CallArgument { ordinal } => {
                KernelOwnerEdgeRole::CallArgument { ordinal }
            }
            PackedKernelOwnerEdgeRole::CallOutArgument { ordinal } => {
                KernelOwnerEdgeRole::CallOutArgument { ordinal }
            }
            PackedKernelOwnerEdgeRole::AbiArgument { name } => KernelOwnerEdgeRole::AbiArgument {
                name: self.symbol(name)?.into(),
            },
            PackedKernelOwnerEdgeRole::LatestBranch => KernelOwnerEdgeRole::LatestBranch,
            PackedKernelOwnerEdgeRole::WhenInput => KernelOwnerEdgeRole::WhenInput,
            PackedKernelOwnerEdgeRole::WhenArm => KernelOwnerEdgeRole::WhenArm,
            PackedKernelOwnerEdgeRole::ThenInput => KernelOwnerEdgeRole::ThenInput,
            PackedKernelOwnerEdgeRole::ThenOutput => KernelOwnerEdgeRole::ThenOutput,
            PackedKernelOwnerEdgeRole::InfixLeft => KernelOwnerEdgeRole::InfixLeft,
            PackedKernelOwnerEdgeRole::InfixRight => KernelOwnerEdgeRole::InfixRight,
            PackedKernelOwnerEdgeRole::DrainingInput => KernelOwnerEdgeRole::DrainingInput,
            PackedKernelOwnerEdgeRole::HoldInitial => KernelOwnerEdgeRole::HoldInitial,
            PackedKernelOwnerEdgeRole::HoldUpdate => KernelOwnerEdgeRole::HoldUpdate,
            PackedKernelOwnerEdgeRole::MatchOutput => KernelOwnerEdgeRole::MatchOutput,
            PackedKernelOwnerEdgeRole::ArrowOutput => KernelOwnerEdgeRole::ArrowOutput,
            PackedKernelOwnerEdgeRole::FlushPayload => KernelOwnerEdgeRole::FlushPayload,
        })
    }
}

#[derive(Debug)]
struct PackedKernelProjectProgramBuilder {
    text: ProjectTextSnapshot,
    owners: Vec<PackedKernelOwnerProgram>,
    nodes: Vec<PackedKernelOwnerNode>,
    edges: Vec<PackedKernelOwnerInputEdge>,
    external_expressions: Vec<KernelExternalExpression>,
}

impl PackedKernelProjectProgramBuilder {
    fn with_capacity(
        text: ProjectTextSnapshot,
        owners: usize,
        nodes: usize,
        edges: usize,
        external_expressions: usize,
    ) -> Self {
        Self {
            text,
            owners: Vec::with_capacity(owners),
            nodes: Vec::with_capacity(nodes),
            edges: Vec::with_capacity(edges),
            external_expressions: Vec::with_capacity(external_expressions),
        }
    }

    fn push_compatibility_owner(
        &mut self,
        terms: &TypeTermArena,
        owner: crate::KernelOwnerProgramInput,
        owner_index: usize,
    ) -> Result<(), KernelOwnerBuildError> {
        let node_start = self.nodes.len();
        for (expression, node) in owner.nodes.into_vec().into_iter().enumerate() {
            let edge_start = self.edges.len();
            for edge in node.inputs.into_vec() {
                self.edges
                    .push(pack_edge(&self.text, edge, owner_index, expression)?);
            }
            let input_span = packed_span(edge_start, self.edges.len(), "owner input edges")?;
            self.nodes.push(PackedKernelOwnerNode {
                kind: pack_kind(terms, node.kind, owner_index, expression)?,
                inputs: input_span,
                mode: node.mode,
            });
        }
        let node_span = packed_span(node_start, self.nodes.len(), "owner nodes")?;
        let external_start = self.external_expressions.len();
        self.external_expressions.extend(owner.external_expressions);
        let external_span = packed_span(
            external_start,
            self.external_expressions.len(),
            "owner external expressions",
        )?;
        self.owners.push(PackedKernelOwnerProgram {
            nodes: node_span,
            formal_count: owner.formal_count,
            external_expressions: external_span,
            result: owner.result,
        });
        Ok(())
    }

    fn finish(self) -> Result<PackedKernelProjectProgram, KernelOwnerBuildError> {
        let program = PackedKernelProjectProgram {
            text: self.text,
            owners: self.owners.into_boxed_slice(),
            nodes: self.nodes.into_boxed_slice(),
            edges: self.edges.into_boxed_slice(),
            external_expressions: self.external_expressions.into_boxed_slice(),
        };
        program.validate()?;
        Ok(program)
    }
}

impl PackedKernelProjectProgram {
    fn validate(&self) -> Result<(), KernelOwnerBuildError> {
        for (owner_index, owner) in self.owners().enumerate() {
            let node_count = owner.node_count();
            let input_namespace_count = node_count
                .checked_add(owner.external_expressions().len())
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel definition {owner_index} input namespace overflows usize"
                    ))
                })?;
            if owner.result().0 as usize >= node_count {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel definition {owner_index} result {} is outside its {node_count} packed nodes",
                    owner.result().0
                )));
            }
            for (expression, node) in owner.nodes().iter().enumerate() {
                for edge in node.inputs(owner) {
                    if edge.expression.0 as usize >= input_namespace_count {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel definition {owner_index} expression {expression} input {} is outside its {input_namespace_count}-value packed input namespace",
                            edge.expression.0
                        )));
                    }
                }
                match node.kind {
                    PackedKernelOwnerNodeKind::FormalRead { formal, .. }
                    | PackedKernelOwnerNodeKind::ContextRead { formal, .. }
                        if formal >= owner.formal_count() =>
                    {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel definition {owner_index} expression {expression} reads formal {formal} outside its {}-formal frame",
                            owner.formal_count()
                        )));
                    }
                    PackedKernelOwnerNodeKind::ValueRead {
                        mode_narrowing: Some(selector),
                        ..
                    } if selector.0 as usize >= node_count => {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel definition {owner_index} expression {expression} mode selector {} is outside its {node_count} packed nodes",
                            selector.0
                        )));
                    }
                    PackedKernelOwnerNodeKind::UserCall {
                        target,
                        inherited_formal,
                    } => {
                        let target_owner = self.owner(target).ok_or_else(|| {
                            KernelOwnerBuildError::new(format!(
                                "kernel definition {owner_index} expression {expression} calls missing definition {}",
                                target.0
                            ))
                        })?;
                        if let Some(inherited) = inherited_formal {
                            if inherited.caller_ordinal >= owner.formal_count()
                                || inherited.target_ordinal >= target_owner.formal_count()
                            {
                                return Err(KernelOwnerBuildError::new(format!(
                                    "kernel definition {owner_index} expression {expression} has inherited formal ({}, {}) outside caller/target frames ({}, {})",
                                    inherited.caller_ordinal,
                                    inherited.target_ordinal,
                                    owner.formal_count(),
                                    target_owner.formal_count()
                                )));
                            }
                        }
                    }
                    _ => {}
                }
            }
            for external in owner.external_expressions() {
                let target_owner = self.owner(external.owner).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel definition {owner_index} references missing external definition {}",
                        external.owner.0
                    ))
                })?;
                if let crate::KernelExternalTarget::Expression(expression) = external.target
                    && expression.0 as usize >= target_owner.node_count()
                {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel definition {owner_index} references external definition {} expression {} outside its {} packed nodes",
                        external.owner.0,
                        expression.0,
                        target_owner.node_count()
                    )));
                }
            }
        }
        Ok(())
    }
}

impl PackedKernelOwnerNode {
    pub fn inputs<'a>(
        &'a self,
        owner: PackedKernelOwnerProgramRef<'a>,
    ) -> &'a [PackedKernelOwnerInputEdge] {
        self.inputs
            .get(&owner.store.edges)
            .expect("validated packed owner input-edge span remains in range")
    }
}

pub(crate) fn pack_kernel_project_program(
    input: KernelProjectProgramInput,
    terms: &TypeTermArena,
) -> Result<PackedKernelProjectProgram, KernelOwnerBuildError> {
    let text = terms.text_snapshot();
    let owner_count = input.owners.len();
    let node_count = input.owners.iter().map(|owner| owner.nodes.len()).sum();
    let edge_count = input
        .owners
        .iter()
        .flat_map(|owner| &owner.nodes)
        .map(|node| node.inputs.len())
        .sum();
    let external_count = input
        .owners
        .iter()
        .map(|owner| owner.external_expressions.len())
        .sum();
    let mut builder = PackedKernelProjectProgramBuilder::with_capacity(
        text.clone(),
        owner_count,
        node_count,
        edge_count,
        external_count,
    );

    for (owner_index, owner) in input.owners.into_vec().into_iter().enumerate() {
        builder.push_compatibility_owner(terms, owner, owner_index)?;
    }

    builder.finish()
}

fn packed_span(start: usize, end: usize, label: &str) -> Result<Span32, KernelOwnerBuildError> {
    Span32::from_bounds(start, end, label)
        .map_err(|error| KernelOwnerBuildError::new(error.to_string()))
}

fn symbol(
    text: &ProjectTextSnapshot,
    value: &str,
    owner: usize,
    expression: usize,
    label: &str,
) -> Result<SymbolId, KernelOwnerBuildError> {
    text.lookup_symbol(value).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel definition {owner} expression {expression} {label} `{value}` is absent from the project text authority"
        ))
    })
}

fn path(
    text: &ProjectTextSnapshot,
    value: &[Box<str>],
    owner: usize,
    expression: usize,
    label: &str,
) -> Result<PathId, KernelOwnerBuildError> {
    text.lookup_path(value.iter().map(Box::as_ref)).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel definition {owner} expression {expression} {label} is absent from the project text authority"
        ))
    })
}

fn pack_pattern(
    text: &ProjectTextSnapshot,
    pattern: KernelPattern,
    owner: usize,
    expression: usize,
) -> Result<PackedKernelPattern, KernelOwnerBuildError> {
    Ok(match pattern {
        KernelPattern::Wildcard => PackedKernelPattern::Wildcard,
        KernelPattern::Number => PackedKernelPattern::Number,
        KernelPattern::Text => PackedKernelPattern::Text,
        KernelPattern::Bits { width } => PackedKernelPattern::Bits { width },
        KernelPattern::Tag { name, fields } => PackedKernelPattern::Tag {
            name: symbol(text, &name, owner, expression, "pattern tag")?,
            fields: path(text, &fields, owner, expression, "pattern fields")?,
        },
        KernelPattern::Binding { name } => PackedKernelPattern::Binding {
            name: symbol(text, &name, owner, expression, "pattern binding")?,
        },
        KernelPattern::Invalid => PackedKernelPattern::Invalid,
    })
}

fn pack_render_constructor(
    text: &ProjectTextSnapshot,
    kind: KernelRenderConstructorKind,
    owner: usize,
    expression: usize,
) -> Result<PackedKernelRenderConstructorKind, KernelOwnerBuildError> {
    Ok(match kind {
        KernelRenderConstructorKind::Fixed(name) => PackedKernelRenderConstructorKind::Fixed(
            symbol(text, &name, owner, expression, "render constructor")?,
        ),
        KernelRenderConstructorKind::StripeDirection => {
            PackedKernelRenderConstructorKind::StripeDirection
        }
    })
}

fn pack_kind(
    terms: &TypeTermArena,
    kind: KernelOwnerNodeKind,
    owner: usize,
    expression: usize,
) -> Result<PackedKernelOwnerNodeKind, KernelOwnerBuildError> {
    let text = terms.text_snapshot();
    Ok(match kind {
        KernelOwnerNodeKind::KnownPacked(reference) => PackedKernelOwnerNodeKind::Known(
            validate_type_ref(terms, reference, owner, expression, "known value")?,
        ),
        KernelOwnerNodeKind::SourcePacked(reference) => PackedKernelOwnerNodeKind::Source(
            validate_type_ref(terms, reference, owner, expression, "SOURCE payload")?,
        ),
        KernelOwnerNodeKind::FixedAbiCallPacked { result } => {
            PackedKernelOwnerNodeKind::FixedAbiCall {
                result: validate_type_ref(
                    terms,
                    result,
                    owner,
                    expression,
                    "fixed ABI result",
                )?,
            }
        }
        KernelOwnerNodeKind::Known(_)
        | KernelOwnerNodeKind::Source(_)
        | KernelOwnerNodeKind::FixedAbiCall { .. } => {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel definition {owner} expression {expression} retains a recursive closed type after project packing"
            )));
        }
        KernelOwnerNodeKind::Absent => PackedKernelOwnerNodeKind::Absent,
        KernelOwnerNodeKind::Text => PackedKernelOwnerNodeKind::Text,
        KernelOwnerNodeKind::TextTemplate => PackedKernelOwnerNodeKind::TextTemplate,
        KernelOwnerNodeKind::Number => PackedKernelOwnerNodeKind::Number,
        KernelOwnerNodeKind::Byte => PackedKernelOwnerNodeKind::Byte,
        KernelOwnerNodeKind::Bits(width) => PackedKernelOwnerNodeKind::Bits(width),
        KernelOwnerNodeKind::Tag(tag) => PackedKernelOwnerNodeKind::Tag(symbol(
            text,
            &tag,
            owner,
            expression,
            "tag",
        )?),
        KernelOwnerNodeKind::Record { tag } => PackedKernelOwnerNodeKind::Record {
            tag: tag
                .as_deref()
                .map(|tag| symbol(text, tag, owner, expression, "record tag"))
                .transpose()?,
        },
        KernelOwnerNodeKind::Block => PackedKernelOwnerNodeKind::Block,
        KernelOwnerNodeKind::Collection { kind, capacity } => {
            PackedKernelOwnerNodeKind::Collection {
                kind,
                capacity: capacity
                    .map(|capacity| {
                        u32::try_from(capacity).map_err(|_| {
                            KernelOwnerBuildError::new(format!(
                                "kernel definition {owner} expression {expression} collection capacity exceeds u32"
                            ))
                        })
                    })
                    .transpose()?,
            }
        }
        KernelOwnerNodeKind::MapEntry => PackedKernelOwnerNodeKind::MapEntry,
        KernelOwnerNodeKind::FormalRead { formal, fields } => {
            PackedKernelOwnerNodeKind::FormalRead {
                formal,
                fields: path(text, &fields, owner, expression, "formal path")?,
            }
        }
        KernelOwnerNodeKind::ContextRead { formal, fields } => {
            PackedKernelOwnerNodeKind::ContextRead {
                formal,
                fields: path(text, &fields, owner, expression, "context path")?,
            }
        }
        KernelOwnerNodeKind::LexicalRead { fields } => {
            PackedKernelOwnerNodeKind::LexicalRead {
                fields: path(text, &fields, owner, expression, "lexical path")?,
            }
        }
        KernelOwnerNodeKind::ValueRead {
            fields,
            mode_narrowing,
        } => PackedKernelOwnerNodeKind::ValueRead {
            fields: path(text, &fields, owner, expression, "value path")?,
            mode_narrowing,
        },
        KernelOwnerNodeKind::DerivedRead { fields } => {
            PackedKernelOwnerNodeKind::DerivedRead {
                fields: path(text, &fields, owner, expression, "derived path")?,
            }
        }
        KernelOwnerNodeKind::PatternRead { pattern, fields } => {
            PackedKernelOwnerNodeKind::PatternRead {
                pattern: pack_pattern(text, pattern, owner, expression)?,
                fields: path(text, &fields, owner, expression, "pattern-read path")?,
            }
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            PackedKernelOwnerNodeKind::CollectionItemRead
        }
        KernelOwnerNodeKind::FreshOut => PackedKernelOwnerNodeKind::FreshOut,
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => PackedKernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        },
        KernelOwnerNodeKind::RenderConstructor { kind } => {
            PackedKernelOwnerNodeKind::RenderConstructor {
                kind: pack_render_constructor(text, kind, owner, expression)?,
            }
        }
        KernelOwnerNodeKind::PureBuiltin { kind } => {
            PackedKernelOwnerNodeKind::PureBuiltin { kind }
        }
        KernelOwnerNodeKind::HostEffect { operation } => {
            PackedKernelOwnerNodeKind::HostEffect {
                operation: symbol(text, &operation, owner, expression, "host effect")?,
            }
        }
        KernelOwnerNodeKind::Latest => PackedKernelOwnerNodeKind::Latest,
        KernelOwnerNodeKind::When => PackedKernelOwnerNodeKind::When,
        KernelOwnerNodeKind::Then => PackedKernelOwnerNodeKind::Then,
        KernelOwnerNodeKind::Infix { operation } => PackedKernelOwnerNodeKind::Infix {
            operation: symbol(text, &operation, owner, expression, "infix operation")?,
        },
        KernelOwnerNodeKind::Draining => PackedKernelOwnerNodeKind::Draining,
        KernelOwnerNodeKind::Hold => PackedKernelOwnerNodeKind::Hold,
        KernelOwnerNodeKind::MatchArm { pattern } => PackedKernelOwnerNodeKind::MatchArm {
            pattern: pack_pattern(text, pattern, owner, expression)?,
        },
        KernelOwnerNodeKind::Arrow => PackedKernelOwnerNodeKind::Arrow,
        KernelOwnerNodeKind::Delimiter => PackedKernelOwnerNodeKind::Delimiter,
        KernelOwnerNodeKind::Unknown => PackedKernelOwnerNodeKind::Unknown,
        KernelOwnerNodeKind::Flush => PackedKernelOwnerNodeKind::Flush,
        KernelOwnerNodeKind::FieldProjection { field } => {
            PackedKernelOwnerNodeKind::FieldProjection {
                field: symbol(text, &field, owner, expression, "field projection")?,
            }
        }
    })
}

fn validate_type_ref(
    terms: &TypeTermArena,
    reference: KernelTypeRef,
    owner: usize,
    expression: usize,
    label: &str,
) -> Result<KernelTypeRef, KernelOwnerBuildError> {
    let Some(term) = terms.resolve_type_ref(reference) else {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel definition {owner} expression {expression} {label} belongs to a foreign type authority"
        )));
    };
    if terms.has_variable(term) {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel definition {owner} expression {expression} {label} contains solver variables"
        )));
    }
    Ok(reference)
}

fn pack_edge(
    text: &ProjectTextSnapshot,
    edge: KernelOwnerInputEdge,
    owner: usize,
    expression: usize,
) -> Result<PackedKernelOwnerInputEdge, KernelOwnerBuildError> {
    let role = match edge.role {
        KernelOwnerEdgeRole::RecordField { name, spread } => {
            PackedKernelOwnerEdgeRole::RecordField {
                name: symbol(text, &name, owner, expression, "record field")?,
                spread,
            }
        }
        KernelOwnerEdgeRole::TextDynamic => PackedKernelOwnerEdgeRole::TextDynamic,
        KernelOwnerEdgeRole::BlockResult => PackedKernelOwnerEdgeRole::BlockResult,
        KernelOwnerEdgeRole::CollectionItem => PackedKernelOwnerEdgeRole::CollectionItem,
        KernelOwnerEdgeRole::MapEntry => PackedKernelOwnerEdgeRole::MapEntry,
        KernelOwnerEdgeRole::MapKey => PackedKernelOwnerEdgeRole::MapKey,
        KernelOwnerEdgeRole::MapValue => PackedKernelOwnerEdgeRole::MapValue,
        KernelOwnerEdgeRole::ReadProvider => PackedKernelOwnerEdgeRole::ReadProvider,
        KernelOwnerEdgeRole::CallArgument { ordinal } => {
            PackedKernelOwnerEdgeRole::CallArgument { ordinal }
        }
        KernelOwnerEdgeRole::CallOutArgument { ordinal } => {
            PackedKernelOwnerEdgeRole::CallOutArgument { ordinal }
        }
        KernelOwnerEdgeRole::AbiArgument { name } => PackedKernelOwnerEdgeRole::AbiArgument {
            name: symbol(text, &name, owner, expression, "ABI argument")?,
        },
        KernelOwnerEdgeRole::LatestBranch => PackedKernelOwnerEdgeRole::LatestBranch,
        KernelOwnerEdgeRole::WhenInput => PackedKernelOwnerEdgeRole::WhenInput,
        KernelOwnerEdgeRole::WhenArm => PackedKernelOwnerEdgeRole::WhenArm,
        KernelOwnerEdgeRole::ThenInput => PackedKernelOwnerEdgeRole::ThenInput,
        KernelOwnerEdgeRole::ThenOutput => PackedKernelOwnerEdgeRole::ThenOutput,
        KernelOwnerEdgeRole::InfixLeft => PackedKernelOwnerEdgeRole::InfixLeft,
        KernelOwnerEdgeRole::InfixRight => PackedKernelOwnerEdgeRole::InfixRight,
        KernelOwnerEdgeRole::DrainingInput => PackedKernelOwnerEdgeRole::DrainingInput,
        KernelOwnerEdgeRole::HoldInitial => PackedKernelOwnerEdgeRole::HoldInitial,
        KernelOwnerEdgeRole::HoldUpdate => PackedKernelOwnerEdgeRole::HoldUpdate,
        KernelOwnerEdgeRole::MatchOutput => PackedKernelOwnerEdgeRole::MatchOutput,
        KernelOwnerEdgeRole::ArrowOutput => PackedKernelOwnerEdgeRole::ArrowOutput,
        KernelOwnerEdgeRole::FlushPayload => PackedKernelOwnerEdgeRole::FlushPayload,
    };
    Ok(PackedKernelOwnerInputEdge {
        role,
        expression: edge.expression,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KernelExpressionId, KernelOwnerNode, KernelOwnerProgramInput};
    use boon_contract::PackedTextCatalogBuilder;

    #[test]
    fn packing_flattens_owner_nodes_edges_and_paths() {
        let mut text = PackedTextCatalogBuilder::new();
        text.intern_path(["item", "name"]).unwrap();
        text.intern_symbol("field").unwrap();
        let text = text.freeze();
        let input = KernelProjectProgramInput {
            owners: vec![KernelOwnerProgramInput {
                nodes: vec![KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: vec!["item".into(), "name".into()].into_boxed_slice(),
                        mode_narrowing: None,
                    },
                    inputs: vec![KernelOwnerInputEdge {
                        role: KernelOwnerEdgeRole::RecordField {
                            name: "field".into(),
                            spread: false,
                        },
                        expression: KernelExpressionId(0),
                    }]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                }]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(0),
            }]
            .into_boxed_slice(),
        };
        let mut terms = TypeTermArena::with_text(text.clone());
        let mut input = input;
        crate::pack_project_closed_type_roots(&mut input, &mut terms).unwrap();
        let packed = pack_kernel_project_program(input, &terms).unwrap();
        assert_eq!(packed.definition_count(), 1);
        assert_eq!(packed.node_count(), 1);
        assert_eq!(packed.edge_count(), 1);
        let owner = packed.owner_at(0).unwrap();
        let [node] = owner.nodes() else { panic!() };
        let PackedKernelOwnerNodeKind::ValueRead { fields, .. } = node.kind else {
            panic!()
        };
        assert_eq!(text.path_depth(fields), Some(2));
        assert!(matches!(
            node.inputs(owner)[0].role,
            PackedKernelOwnerEdgeRole::RecordField { .. }
        ));
    }

    #[test]
    fn packing_rejects_out_of_range_graph_coordinates() {
        let text = PackedTextCatalogBuilder::new().freeze();
        let input = KernelProjectProgramInput {
            owners: vec![KernelOwnerProgramInput {
                nodes: vec![KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: vec![KernelOwnerInputEdge {
                        role: KernelOwnerEdgeRole::BlockResult,
                        expression: KernelExpressionId(1),
                    }]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                }]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(0),
            }]
            .into_boxed_slice(),
        };
        let terms = TypeTermArena::with_text(text);
        assert_eq!(
            pack_kernel_project_program(input, &terms)
                .unwrap_err()
                .to_string(),
            "kernel definition 0 expression 0 input 1 is outside its 1-value packed input namespace"
        );
    }

    #[test]
    fn packing_rejects_foreign_call_targets() {
        let text = PackedTextCatalogBuilder::new().freeze();
        let input = KernelProjectProgramInput {
            owners: vec![KernelOwnerProgramInput {
                nodes: vec![KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: crate::KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                }]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(0),
            }]
            .into_boxed_slice(),
        };
        let terms = TypeTermArena::with_text(text);
        assert_eq!(
            pack_kernel_project_program(input, &terms)
                .unwrap_err()
                .to_string(),
            "kernel definition 0 expression 0 calls missing definition 1"
        );
    }
}
