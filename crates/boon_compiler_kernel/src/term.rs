use boon_checked::{BytesType, FlowMode, FlowType, ObjectShape, Type, TypeVar, Variant};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NameId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeTermId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeVariableId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BytesTerm {
    Dynamic,
    Fixed(usize),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObjectFieldTerm {
    pub name: NameId,
    pub ty: TypeTermId,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum VariantTerm {
    Tag(NameId),
    Tagged { tag: NameId, fields: TypeTermId },
}

impl VariantTerm {
    pub const fn tag(&self) -> NameId {
        match self {
            Self::Tag(tag) | Self::Tagged { tag, .. } => *tag,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TypeTerm {
    Text,
    Number,
    Bytes(BytesTerm),
    Absent,
    VariantSet(Box<[VariantTerm]>),
    Object {
        fields: Box<[ObjectFieldTerm]>,
        open: bool,
    },
    /// An unconstrained object-shaped requirement.
    ///
    /// This deliberately exports as the checked model's open empty object,
    /// but remains distinct from an actual open empty object produced by
    /// structural widening. Conflating those two meanings made a real
    /// incompatible-shape result disappear when it was widened again.
    OpenObjectPlaceholder,
    RenderContract,
    List(TypeTermId),
    Function {
        args: Box<[TypeTermId]>,
        result_mode: FlowMode,
        result: TypeTermId,
    },
    UnresolvedShape(NameId),
    Variable(TypeVariableId),
    Unknown,
    Union(Box<[TypeTermId]>),
    Map {
        key: TypeTermId,
        value: TypeTermId,
    },
    Set(TypeTermId),
    Bits(u32),
}

fn is_value_placeholder_term(term: &TypeTerm) -> bool {
    matches!(
        term,
        TypeTerm::Variable(_)
            | TypeTerm::Unknown
            | TypeTerm::UnresolvedShape(_)
            | TypeTerm::OpenObjectPlaceholder
    )
}

/// Immutable type DAG used by the inference kernel.
///
/// Hash maps are lookup-only. Canonical output order is derived from the
/// interned terms and source field order, never from hash-table iteration.
#[derive(Debug)]
pub struct TypeTermArena {
    names: Vec<Box<str>>,
    name_ids: HashMap<u64, Vec<NameId>>,
    terms: Vec<TypeTerm>,
    term_has_variable: Vec<bool>,
    term_ids: HashMap<u64, Vec<TypeTermId>>,
    object_ids: HashMap<u64, Vec<TypeTermId>>,
    variable_terms: Vec<Option<TypeTermId>>,
    structural_widen_cache: HashMap<(TypeTermId, TypeTermId), TypeTermId>,
    absent: TypeTermId,
    unknown: TypeTermId,
    text: TypeTermId,
    number: TypeTermId,
    render_contract: TypeTermId,
    open_object: TypeTermId,
    work: TypeTermArenaWork,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TypeTermArenaWork {
    pub intern_requests: u64,
    pub intern_hits: u64,
    /// Variable, object, variant, union, list/set, map, function, scalar.
    pub intern_requests_by_kind: [u64; 8],
    pub intern_hits_by_kind: [u64; 8],
    pub structural_widen_requests: u64,
    pub structural_widen_hits: u64,
}

impl Default for TypeTermArena {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeTermArena {
    pub fn new() -> Self {
        let placeholder = TypeTermId(0);
        let mut arena = Self {
            names: Vec::new(),
            name_ids: HashMap::new(),
            terms: Vec::new(),
            term_has_variable: Vec::new(),
            term_ids: HashMap::new(),
            object_ids: HashMap::new(),
            variable_terms: Vec::new(),
            structural_widen_cache: HashMap::new(),
            absent: placeholder,
            unknown: placeholder,
            text: placeholder,
            number: placeholder,
            render_contract: placeholder,
            open_object: placeholder,
            work: TypeTermArenaWork::default(),
        };
        arena.absent = arena.intern_raw(TypeTerm::Absent);
        arena.unknown = arena.intern_raw(TypeTerm::Unknown);
        arena.text = arena.intern_raw(TypeTerm::Text);
        arena.number = arena.intern_raw(TypeTerm::Number);
        arena.render_contract = arena.intern_raw(TypeTerm::RenderContract);
        arena.open_object = arena.intern_raw(TypeTerm::OpenObjectPlaceholder);
        arena.work = TypeTermArenaWork::default();
        arena
    }

    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub(crate) fn name_count(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub const fn absent(&self) -> TypeTermId {
        self.absent
    }

    pub const fn unknown(&self) -> TypeTermId {
        self.unknown
    }

    pub const fn text(&self) -> TypeTermId {
        self.text
    }

    pub const fn number(&self) -> TypeTermId {
        self.number
    }

    pub const fn render_contract(&self) -> TypeTermId {
        self.render_contract
    }

    pub const fn open_object(&self) -> TypeTermId {
        self.open_object
    }

    pub(crate) fn reset_work(&mut self) {
        self.work = TypeTermArenaWork::default();
    }

    pub(crate) const fn work(&self) -> TypeTermArenaWork {
        self.work
    }

    pub fn term(&self, id: TypeTermId) -> &TypeTerm {
        &self.terms[id.0 as usize]
    }

    /// Whether this immutable term DAG contains any occurrence-local variable.
    /// Closed terms can bypass solver resolution and dependency traversal.
    pub(crate) fn has_variable(&self, id: TypeTermId) -> bool {
        self.term_has_variable[id.0 as usize]
    }

    pub fn name(&self, id: NameId) -> &str {
        &self.names[id.0 as usize]
    }

    pub fn intern_name(&mut self, name: impl AsRef<str>) -> NameId {
        let name = name.as_ref();
        let hash = lookup_hash(name);
        if let Some(id) = self.name_ids.get(&hash).and_then(|candidates| {
            candidates
                .iter()
                .find(|candidate| self.names[candidate.0 as usize].as_ref() == name)
        }) {
            return *id;
        }
        let id = NameId(u32::try_from(self.names.len()).expect("kernel name count exceeds u32"));
        self.names.push(Box::<str>::from(name));
        self.name_ids.entry(hash).or_default().push(id);
        id
    }

    pub fn variable(&mut self, variable: TypeVariableId) -> TypeTermId {
        let index = variable.0 as usize;
        if let Some(term) = self.variable_terms.get(index).copied().flatten() {
            return term;
        }
        let term = self.intern_raw(TypeTerm::Variable(variable));
        if self.variable_terms.len() <= index {
            self.variable_terms.resize(index + 1, None);
        }
        self.variable_terms[index] = Some(term);
        term
    }

    pub fn bytes(&mut self, bytes: BytesTerm) -> TypeTermId {
        self.intern_raw(TypeTerm::Bytes(bytes))
    }

    pub fn bits(&mut self, width: u32) -> TypeTermId {
        self.intern_raw(TypeTerm::Bits(width))
    }

    pub fn unresolved_shape(&mut self, reason: impl AsRef<str>) -> TypeTermId {
        let reason = self.intern_name(reason);
        self.intern_raw(TypeTerm::UnresolvedShape(reason))
    }

    pub fn list(&mut self, item: TypeTermId) -> TypeTermId {
        self.intern_raw(TypeTerm::List(item))
    }

    pub fn set(&mut self, item: TypeTermId) -> TypeTermId {
        self.intern_raw(TypeTerm::Set(item))
    }

    pub fn map(&mut self, key: TypeTermId, value: TypeTermId) -> TypeTermId {
        self.intern_raw(TypeTerm::Map { key, value })
    }

    pub fn function(
        &mut self,
        args: impl IntoIterator<Item = TypeTermId>,
        result_mode: FlowMode,
        result: TypeTermId,
    ) -> TypeTermId {
        self.intern_raw(TypeTerm::Function {
            args: args.into_iter().collect::<Vec<_>>().into_boxed_slice(),
            result_mode,
            result,
        })
    }

    pub fn object(
        &mut self,
        fields: impl IntoIterator<Item = (NameId, TypeTermId)>,
        open: bool,
    ) -> TypeTermId {
        let mut ordered = Vec::<ObjectFieldTerm>::new();
        for (name, ty) in fields {
            if let Some(index) = ordered.iter().position(|field| field.name == name) {
                ordered[index].ty = ty;
            } else {
                ordered.push(ObjectFieldTerm { name, ty });
            }
        }
        self.intern_object(ordered, open)
    }

    pub fn variant_tag(&mut self, tag: impl AsRef<str>) -> VariantTerm {
        VariantTerm::Tag(self.intern_name(tag))
    }

    pub fn tagged_variant(&mut self, tag: impl AsRef<str>, fields: TypeTermId) -> VariantTerm {
        debug_assert!(matches!(self.term(fields), TypeTerm::Object { .. }));
        VariantTerm::Tagged {
            tag: self.intern_name(tag),
            fields,
        }
    }

    pub fn variant_set(&mut self, variants: impl IntoIterator<Item = VariantTerm>) -> TypeTermId {
        self.variant_set_with_order(variants, true)
    }

    pub(crate) fn variant_set_preserving_order(
        &mut self,
        variants: impl IntoIterator<Item = VariantTerm>,
    ) -> TypeTermId {
        self.variant_set_with_order(variants, false)
    }

    fn variant_set_with_order(
        &mut self,
        variants: impl IntoIterator<Item = VariantTerm>,
        canonicalize: bool,
    ) -> TypeTermId {
        let mut merged = Vec::<VariantTerm>::new();
        for incoming in variants {
            let tag = incoming.tag();
            let Some(index) = merged.iter().position(|variant| variant.tag() == tag) else {
                merged.push(incoming);
                continue;
            };
            let replacement = match (&merged[index], incoming) {
                (VariantTerm::Tag(_), VariantTerm::Tag(_)) => None,
                (VariantTerm::Tagged { .. }, VariantTerm::Tag(_)) => None,
                (VariantTerm::Tag(_), tagged @ VariantTerm::Tagged { .. }) => Some(tagged),
                (
                    VariantTerm::Tagged {
                        tag,
                        fields: existing,
                    },
                    VariantTerm::Tagged {
                        fields: incoming, ..
                    },
                ) => Some(VariantTerm::Tagged {
                    tag: *tag,
                    fields: self.structural_widen(*existing, incoming),
                }),
            };
            if let Some(replacement) = replacement {
                merged[index] = replacement;
            }
        }
        if canonicalize {
            merged.sort_by(|left, right| self.compare_variants_canonically(left, right));
        }
        self.intern_raw(TypeTerm::VariantSet(merged.into_boxed_slice()))
    }

    fn compare_variants_canonically(&self, left: &VariantTerm, right: &VariantTerm) -> Ordering {
        match (left, right) {
            (VariantTerm::Tag(left), VariantTerm::Tag(right)) => {
                self.name(*left).cmp(self.name(*right))
            }
            (VariantTerm::Tag(_), VariantTerm::Tagged { .. }) => Ordering::Less,
            (VariantTerm::Tagged { .. }, VariantTerm::Tag(_)) => Ordering::Greater,
            (
                VariantTerm::Tagged {
                    tag: left_tag,
                    fields: left_fields,
                },
                VariantTerm::Tagged {
                    tag: right_tag,
                    fields: right_fields,
                },
            ) => {
                let left_count = match self.term(*left_fields) {
                    TypeTerm::Object { fields, .. } => fields.len(),
                    _ => unreachable!("tagged variant payload is an object"),
                };
                let right_count = match self.term(*right_fields) {
                    TypeTerm::Object { fields, .. } => fields.len(),
                    _ => unreachable!("tagged variant payload is an object"),
                };
                compare_tagged_variant_sort_suffixes(
                    self.name(*left_tag),
                    left_count,
                    self.name(*right_tag),
                    right_count,
                )
            }
        }
    }

    pub fn union(&mut self, candidates: impl IntoIterator<Item = TypeTermId>) -> TypeTermId {
        let mut pending = candidates.into_iter().collect::<Vec<_>>();
        let mut members = Vec::<TypeTermId>::new();
        let mut variants = Vec::<VariantTerm>::new();
        while let Some(candidate) = pending.pop() {
            match self.term(candidate).clone() {
                TypeTerm::Absent => {}
                TypeTerm::Union(nested) => pending.extend(nested.iter().rev().copied()),
                TypeTerm::VariantSet(incoming) => variants.extend(incoming),
                _ if !members.contains(&candidate) => members.push(candidate),
                _ => {}
            }
        }
        if !variants.is_empty() {
            let variants = self.variant_set(variants);
            members.push(variants);
        }
        members.sort_by(|left, right| self.compare_terms(*left, *right));
        members.dedup();
        match members.as_slice() {
            [] => self.absent,
            [member] => *member,
            _ => self.intern_raw(TypeTerm::Union(members.into_boxed_slice())),
        }
    }

    pub fn structural_widen(&mut self, left: TypeTermId, right: TypeTermId) -> TypeTermId {
        self.work.structural_widen_requests = self.work.structural_widen_requests.saturating_add(1);
        if let Some(widened) = self.structural_widen_cache.get(&(left, right)).copied() {
            self.work.structural_widen_hits = self.work.structural_widen_hits.saturating_add(1);
            return widened;
        }
        let widened = self.structural_widen_uncached(left, right);
        self.structural_widen_cache.insert((left, right), widened);
        widened
    }

    fn structural_widen_uncached(&mut self, left: TypeTermId, right: TypeTermId) -> TypeTermId {
        let left_term = self.term(left).clone();
        let right_term = self.term(right).clone();
        if is_value_placeholder_term(&left_term) {
            return right;
        }
        if is_value_placeholder_term(&right_term) {
            return left;
        }
        match (left_term, right_term) {
            (TypeTerm::Absent, _) => right,
            (_, TypeTerm::Absent) => left,
            (TypeTerm::Union(members), _) => members.into_iter().fold(right, |widened, member| {
                self.structural_widen(widened, member)
            }),
            (_, TypeTerm::Union(members)) => members.into_iter().fold(left, |widened, member| {
                self.structural_widen(widened, member)
            }),
            (TypeTerm::VariantSet(left), TypeTerm::VariantSet(right)) => {
                self.variant_set(left.into_iter().chain(right))
            }
            (TypeTerm::Bytes(left), TypeTerm::Bytes(right)) => {
                let bytes = if left == right {
                    left
                } else {
                    BytesTerm::Dynamic
                };
                self.bytes(bytes)
            }
            (TypeTerm::Bits(left), TypeTerm::Bits(right)) if left == right => self.bits(left),
            (TypeTerm::List(left), TypeTerm::List(right)) => {
                let item = self.structural_widen(left, right);
                self.list(item)
            }
            (TypeTerm::Set(left), TypeTerm::Set(right)) => {
                let item = self.structural_widen(left, right);
                self.set(item)
            }
            (
                TypeTerm::Map {
                    key: left_key,
                    value: left_value,
                },
                TypeTerm::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => {
                let key = self.structural_widen(left_key, right_key);
                let value = self.structural_widen(left_value, right_value);
                self.map(key, value)
            }
            (
                TypeTerm::Object {
                    fields: left_fields,
                    open: left_open,
                },
                TypeTerm::Object {
                    fields: right_fields,
                    open: right_open,
                },
            ) => {
                let mut fields = left_fields.into_vec();
                for right in right_fields {
                    if let Some(index) = fields.iter().position(|left| left.name == right.name) {
                        fields[index].ty = self.structural_widen(fields[index].ty, right.ty);
                    } else {
                        fields.push(right);
                    }
                }
                self.object(
                    fields.into_iter().map(|field| (field.name, field.ty)),
                    left_open || right_open,
                )
            }
            (left_term, right_term) if left_term == right_term => left,
            _ => self.object([], true),
        }
    }

    pub fn import_checked_type<F>(&mut self, ty: &Type, variable: &mut F) -> TypeTermId
    where
        F: FnMut(TypeVar) -> TypeVariableId,
    {
        match ty {
            Type::Text => self.text,
            Type::Number => self.number,
            Type::Bytes(BytesType::Dynamic) => self.bytes(BytesTerm::Dynamic),
            Type::Bytes(BytesType::Fixed(size)) => self.bytes(BytesTerm::Fixed(*size)),
            Type::Absent => self.absent,
            Type::VariantSet(variants) => {
                let variants = variants
                    .iter()
                    .map(|variant| match variant {
                        Variant::Tag(tag) => self.variant_tag(tag),
                        Variant::Tagged { tag, fields } => {
                            let ordered = fields
                                .ordered_fields()
                                .into_iter()
                                .map(|(name, ty)| {
                                    let name = self.intern_name(name);
                                    let ty = self.import_checked_type(ty, variable);
                                    (name, ty)
                                })
                                .collect::<Vec<_>>();
                            let fields = self.object(ordered, fields.open);
                            self.tagged_variant(tag, fields)
                        }
                    })
                    .collect::<Vec<_>>();
                self.variant_set_preserving_order(variants)
            }
            Type::Object(shape) if shape.open && shape.fields.is_empty() => self.open_object,
            Type::Object(shape) => {
                let fields = shape
                    .ordered_fields()
                    .into_iter()
                    .map(|(name, ty)| {
                        let name = self.intern_name(name);
                        let ty = self.import_checked_type(ty, variable);
                        (name, ty)
                    })
                    .collect::<Vec<_>>();
                self.object(fields, shape.open)
            }
            Type::RenderContract => self.render_contract,
            Type::List(item) => {
                let item = self.import_checked_type(item, variable);
                self.list(item)
            }
            Type::Function { args, result } => {
                let args = args
                    .iter()
                    .map(|argument| self.import_checked_type(argument, variable))
                    .collect::<Vec<_>>();
                let result_ty = self.import_checked_type(&result.ty, variable);
                self.function(args, result.mode, result_ty)
            }
            Type::UnresolvedShape { reason } => self.unresolved_shape(reason),
            Type::Var(source) => {
                let variable = variable(*source);
                self.variable(variable)
            }
            Type::Unknown => self.unknown,
            Type::Union(members) => {
                let members = members
                    .iter()
                    .map(|member| self.import_checked_type(member, variable))
                    .collect::<Vec<_>>();
                self.union(members)
            }
            Type::Map { key, value } => {
                let key = self.import_checked_type(key, variable);
                let value = self.import_checked_type(value, variable);
                self.map(key, value)
            }
            Type::Set(item) => {
                let item = self.import_checked_type(item, variable);
                self.set(item)
            }
            Type::Bits { width } => self.bits(*width),
        }
    }

    pub fn export_checked_type(&self, term: TypeTermId) -> Type {
        self.export_checked_type_inner(term)
    }

    /// Import an immutable term DAG from another kernel arena while rebasing
    /// its variable slots. This is the linker primitive for compiled residual
    /// modules: semantic operations stay shared, while each invocation owns
    /// only a compact variable-frame mapping.
    pub(crate) fn import_rebased_term(
        &mut self,
        source: &TypeTermArena,
        term: TypeTermId,
        variables: &[TypeVariableId],
        term_cache: &mut [Option<TypeTermId>],
        name_cache: &mut [Option<NameId>],
    ) -> TypeTermId {
        if let Some(imported) = term_cache[term.0 as usize] {
            return imported;
        }
        let import_name =
            |target: &mut TypeTermArena, name: NameId, cache: &mut [Option<NameId>]| {
                let slot = &mut cache[name.0 as usize];
                *slot.get_or_insert_with(|| target.intern_name(source.name(name)))
            };
        let imported = match source.term(term).clone() {
            TypeTerm::Text => self.text(),
            TypeTerm::Number => self.number(),
            TypeTerm::Bytes(bytes) => self.bytes(bytes),
            TypeTerm::Absent => self.absent(),
            TypeTerm::VariantSet(variants) => {
                let variants = variants
                    .into_vec()
                    .into_iter()
                    .map(|variant| match variant {
                        VariantTerm::Tag(tag) => {
                            VariantTerm::Tag(import_name(self, tag, name_cache))
                        }
                        VariantTerm::Tagged { tag, fields } => VariantTerm::Tagged {
                            tag: import_name(self, tag, name_cache),
                            fields: self.import_rebased_term(
                                source, fields, variables, term_cache, name_cache,
                            ),
                        },
                    })
                    .collect::<Vec<_>>();
                self.variant_set_preserving_order(variants)
            }
            TypeTerm::Object { fields, open } => {
                let fields = fields
                    .into_vec()
                    .into_iter()
                    .map(|field| {
                        (
                            import_name(self, field.name, name_cache),
                            self.import_rebased_term(
                                source, field.ty, variables, term_cache, name_cache,
                            ),
                        )
                    })
                    .collect::<Vec<_>>();
                self.object(fields, open)
            }
            TypeTerm::OpenObjectPlaceholder => self.open_object(),
            TypeTerm::RenderContract => self.render_contract(),
            TypeTerm::List(item) => {
                let item =
                    self.import_rebased_term(source, item, variables, term_cache, name_cache);
                self.list(item)
            }
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => {
                let args = args
                    .iter()
                    .map(|argument| {
                        self.import_rebased_term(
                            source, *argument, variables, term_cache, name_cache,
                        )
                    })
                    .collect::<Vec<_>>();
                let result =
                    self.import_rebased_term(source, result, variables, term_cache, name_cache);
                self.function(args, result_mode, result)
            }
            TypeTerm::UnresolvedShape(reason) => {
                let reason = source.name(reason).to_owned();
                self.unresolved_shape(reason)
            }
            TypeTerm::Variable(variable) => self.variable(
                *variables
                    .get(variable.0 as usize)
                    .expect("residual module variable belongs to its frame"),
            ),
            TypeTerm::Unknown => self.unknown(),
            TypeTerm::Union(members) => {
                let members = members
                    .iter()
                    .map(|member| {
                        self.import_rebased_term(source, *member, variables, term_cache, name_cache)
                    })
                    .collect::<Vec<_>>();
                self.union(members)
            }
            TypeTerm::Map { key, value } => {
                let key = self.import_rebased_term(source, key, variables, term_cache, name_cache);
                let value =
                    self.import_rebased_term(source, value, variables, term_cache, name_cache);
                self.map(key, value)
            }
            TypeTerm::Set(item) => {
                let item =
                    self.import_rebased_term(source, item, variables, term_cache, name_cache);
                self.set(item)
            }
            TypeTerm::Bits(width) => self.bits(width),
        };
        term_cache[term.0 as usize] = Some(imported);
        imported
    }

    fn export_checked_type_inner(&self, term: TypeTermId) -> Type {
        match self.term(term) {
            TypeTerm::Text => Type::Text,
            TypeTerm::Number => Type::Number,
            TypeTerm::Bytes(BytesTerm::Dynamic) => Type::Bytes(BytesType::Dynamic),
            TypeTerm::Bytes(BytesTerm::Fixed(size)) => Type::Bytes(BytesType::Fixed(*size)),
            TypeTerm::Absent => Type::Absent,
            TypeTerm::VariantSet(variants) => Type::VariantSet(
                variants
                    .iter()
                    .map(|variant| match variant {
                        VariantTerm::Tag(tag) => Variant::Tag(self.name(*tag).to_owned()),
                        VariantTerm::Tagged { tag, fields } => {
                            let Type::Object(fields) = self.export_checked_type_inner(*fields)
                            else {
                                unreachable!("kernel tagged payload is always an object")
                            };
                            Variant::Tagged {
                                tag: self.name(*tag).to_owned(),
                                fields,
                            }
                        }
                    })
                    .collect(),
            ),
            TypeTerm::Object { fields, open } => Type::object(ObjectShape::from_ordered_fields(
                fields.iter().map(|field| {
                    (
                        self.name(field.name).to_owned(),
                        self.export_checked_type_inner(field.ty),
                    )
                }),
                *open,
            )),
            TypeTerm::OpenObjectPlaceholder => {
                Type::object(ObjectShape::new(std::collections::BTreeMap::new(), true))
            }
            TypeTerm::RenderContract => Type::RenderContract,
            TypeTerm::List(item) => Type::List(Type::shared(self.export_checked_type_inner(*item))),
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => Type::Function {
                args: args
                    .iter()
                    .map(|argument| self.export_checked_type_inner(*argument))
                    .collect(),
                result: Box::new(FlowType {
                    mode: *result_mode,
                    ty: self.export_checked_type_inner(*result),
                }),
            },
            TypeTerm::UnresolvedShape(reason) => Type::UnresolvedShape {
                reason: self.name(*reason).to_owned(),
            },
            TypeTerm::Variable(variable) => Type::Var(TypeVar(variable.0)),
            TypeTerm::Unknown => Type::Unknown,
            TypeTerm::Union(members) => boon_checked::canonical_union_type(
                members
                    .iter()
                    .map(|member| self.export_checked_type_inner(*member))
                    .collect(),
            ),
            TypeTerm::Map { key, value } => Type::Map {
                key: Box::new(self.export_checked_type_inner(*key)),
                value: Box::new(self.export_checked_type_inner(*value)),
            },
            TypeTerm::Set(item) => Type::Set(Type::shared(self.export_checked_type_inner(*item))),
            TypeTerm::Bits(width) => Type::Bits { width: *width },
        }
    }

    fn intern_raw(&mut self, term: TypeTerm) -> TypeTermId {
        self.work.intern_requests = self.work.intern_requests.saturating_add(1);
        let work_kind = term_work_kind(&term);
        self.work.intern_requests_by_kind[work_kind] =
            self.work.intern_requests_by_kind[work_kind].saturating_add(1);
        let hash = lookup_hash(&term);
        if let Some(id) = self.term_ids.get(&hash).and_then(|candidates| {
            candidates
                .iter()
                .find(|candidate| self.terms[candidate.0 as usize] == term)
        }) {
            self.work.intern_hits = self.work.intern_hits.saturating_add(1);
            self.work.intern_hits_by_kind[work_kind] =
                self.work.intern_hits_by_kind[work_kind].saturating_add(1);
            return *id;
        }
        let id =
            TypeTermId(u32::try_from(self.terms.len()).expect("kernel term count exceeds u32"));
        let has_variable = match &term {
            TypeTerm::Variable(_) => true,
            TypeTerm::VariantSet(variants) => variants.iter().any(|variant| match variant {
                VariantTerm::Tag(_) => false,
                VariantTerm::Tagged { fields, .. } => self.has_variable(*fields),
            }),
            TypeTerm::Object { fields, .. } => {
                fields.iter().any(|field| self.has_variable(field.ty))
            }
            TypeTerm::List(item) | TypeTerm::Set(item) => self.has_variable(*item),
            TypeTerm::Function { args, result, .. } => {
                args.iter().any(|argument| self.has_variable(*argument))
                    || self.has_variable(*result)
            }
            TypeTerm::Union(members) => members.iter().any(|member| self.has_variable(*member)),
            TypeTerm::Map { key, value } => self.has_variable(*key) || self.has_variable(*value),
            TypeTerm::Text
            | TypeTerm::Number
            | TypeTerm::Bytes(_)
            | TypeTerm::Absent
            | TypeTerm::OpenObjectPlaceholder
            | TypeTerm::RenderContract
            | TypeTerm::UnresolvedShape(_)
            | TypeTerm::Unknown
            | TypeTerm::Bits(_) => false,
        };
        self.terms.push(term);
        self.term_has_variable.push(has_variable);
        self.term_ids.entry(hash).or_default().push(id);
        id
    }

    fn intern_object(&mut self, fields: Vec<ObjectFieldTerm>, open: bool) -> TypeTermId {
        const OBJECT_WORK_KIND: usize = 1;
        self.work.intern_requests = self.work.intern_requests.saturating_add(1);
        self.work.intern_requests_by_kind[OBJECT_WORK_KIND] =
            self.work.intern_requests_by_kind[OBJECT_WORK_KIND].saturating_add(1);
        let hash = lookup_hash(&(open, fields.as_slice()));
        if let Some(id) = self.object_ids.get(&hash).and_then(|candidates| {
            candidates.iter().find(|candidate| {
                matches!(
                    &self.terms[candidate.0 as usize],
                    TypeTerm::Object {
                        fields: candidate_fields,
                        open: candidate_open,
                    } if *candidate_open == open && candidate_fields.as_ref() == fields.as_slice()
                )
            })
        }) {
            self.work.intern_hits = self.work.intern_hits.saturating_add(1);
            self.work.intern_hits_by_kind[OBJECT_WORK_KIND] =
                self.work.intern_hits_by_kind[OBJECT_WORK_KIND].saturating_add(1);
            return *id;
        }
        let has_variable = fields.iter().any(|field| self.has_variable(field.ty));
        let id =
            TypeTermId(u32::try_from(self.terms.len()).expect("kernel term count exceeds u32"));
        self.terms.push(TypeTerm::Object {
            fields: fields.into_boxed_slice(),
            open,
        });
        self.term_has_variable.push(has_variable);
        self.object_ids.entry(hash).or_default().push(id);
        id
    }

    fn compare_terms(&self, left: TypeTermId, right: TypeTermId) -> Ordering {
        if left == right {
            return Ordering::Equal;
        }
        let left = self.term(left);
        let right = self.term(right);
        self.term_rank(left)
            .cmp(&self.term_rank(right))
            .then_with(|| match (left, right) {
                (TypeTerm::Bytes(left), TypeTerm::Bytes(right)) => left.cmp(right),
                (TypeTerm::Bits(left), TypeTerm::Bits(right)) => left.cmp(right),
                (TypeTerm::Variable(left), TypeTerm::Variable(right)) => left.cmp(right),
                (TypeTerm::UnresolvedShape(left), TypeTerm::UnresolvedShape(right)) => {
                    self.name(*left).cmp(self.name(*right))
                }
                (TypeTerm::List(left), TypeTerm::List(right))
                | (TypeTerm::Set(left), TypeTerm::Set(right)) => self.compare_terms(*left, *right),
                (TypeTerm::Map { key: lk, value: lv }, TypeTerm::Map { key: rk, value: rv }) => {
                    self.compare_terms(*lk, *rk)
                        .then_with(|| self.compare_terms(*lv, *rv))
                }
                (TypeTerm::Union(left), TypeTerm::Union(right)) => {
                    self.compare_term_slices(left, right)
                }
                (
                    TypeTerm::Object {
                        fields: left,
                        open: lo,
                    },
                    TypeTerm::Object {
                        fields: right,
                        open: ro,
                    },
                ) => self
                    .compare_field_slices(left, right)
                    .then_with(|| lo.cmp(ro)),
                (TypeTerm::VariantSet(left), TypeTerm::VariantSet(right)) => {
                    self.compare_variant_slices(left, right)
                }
                (
                    TypeTerm::Function {
                        args: la,
                        result_mode: lm,
                        result: lr,
                    },
                    TypeTerm::Function {
                        args: ra,
                        result_mode: rm,
                        result: rr,
                    },
                ) => self
                    .compare_term_slices(la, ra)
                    .then_with(|| flow_mode_rank(*lm).cmp(&flow_mode_rank(*rm)))
                    .then_with(|| self.compare_terms(*lr, *rr)),
                _ => Ordering::Equal,
            })
    }

    fn term_rank(&self, term: &TypeTerm) -> u8 {
        match term {
            TypeTerm::Absent => 0,
            TypeTerm::Text => 1,
            TypeTerm::Number => 2,
            TypeTerm::Bytes(_) => 3,
            TypeTerm::Bits(_) => 4,
            TypeTerm::VariantSet(_) => 5,
            TypeTerm::Object { .. } => 6,
            TypeTerm::OpenObjectPlaceholder => 7,
            TypeTerm::RenderContract => 8,
            TypeTerm::List(_) => 9,
            TypeTerm::Set(_) => 10,
            TypeTerm::Map { .. } => 11,
            TypeTerm::Function { .. } => 12,
            TypeTerm::UnresolvedShape(_) => 13,
            TypeTerm::Variable(_) => 14,
            TypeTerm::Unknown => 15,
            TypeTerm::Union(_) => 16,
        }
    }

    fn compare_term_slices(&self, left: &[TypeTermId], right: &[TypeTermId]) -> Ordering {
        left.len().cmp(&right.len()).then_with(|| {
            left.iter()
                .zip(right)
                .map(|(left, right)| self.compare_terms(*left, *right))
                .find(|order| !order.is_eq())
                .unwrap_or(Ordering::Equal)
        })
    }

    fn compare_field_slices(
        &self,
        left: &[ObjectFieldTerm],
        right: &[ObjectFieldTerm],
    ) -> Ordering {
        left.len().cmp(&right.len()).then_with(|| {
            left.iter()
                .zip(right)
                .map(|(left, right)| {
                    self.name(left.name)
                        .cmp(self.name(right.name))
                        .then_with(|| self.compare_terms(left.ty, right.ty))
                })
                .find(|order| !order.is_eq())
                .unwrap_or(Ordering::Equal)
        })
    }

    fn compare_variant_slices(&self, left: &[VariantTerm], right: &[VariantTerm]) -> Ordering {
        left.len().cmp(&right.len()).then_with(|| {
            left.iter()
                .zip(right)
                .map(|(left, right)| {
                    self.name(left.tag())
                        .cmp(self.name(right.tag()))
                        .then_with(|| match (left, right) {
                            (VariantTerm::Tag(_), VariantTerm::Tagged { .. }) => Ordering::Less,
                            (VariantTerm::Tagged { .. }, VariantTerm::Tag(_)) => Ordering::Greater,
                            (
                                VariantTerm::Tagged { fields: left, .. },
                                VariantTerm::Tagged { fields: right, .. },
                            ) => self.compare_terms(*left, *right),
                            _ => Ordering::Equal,
                        })
                })
                .find(|order| !order.is_eq())
                .unwrap_or(Ordering::Equal)
        })
    }
}

fn term_work_kind(term: &TypeTerm) -> usize {
    match term {
        TypeTerm::Variable(_) => 0,
        TypeTerm::Object { .. } | TypeTerm::OpenObjectPlaceholder => 1,
        TypeTerm::VariantSet(_) => 2,
        TypeTerm::Union(_) => 3,
        TypeTerm::List(_) | TypeTerm::Set(_) => 4,
        TypeTerm::Map { .. } => 5,
        TypeTerm::Function { .. } => 6,
        TypeTerm::Text
        | TypeTerm::Number
        | TypeTerm::Bytes(_)
        | TypeTerm::Absent
        | TypeTerm::RenderContract
        | TypeTerm::UnresolvedShape(_)
        | TypeTerm::Unknown
        | TypeTerm::Bits(_) => 7,
    }
}

/// Mirrors `boon_checked::compare_variants_canonically` without allocating
/// formatted sort keys. Keeping the term arena and public checked projection
/// in one order makes canonical equality independent of inference history.
fn compare_tagged_variant_sort_suffixes(
    left_tag: &str,
    left_field_count: usize,
    right_tag: &str,
    right_field_count: usize,
) -> Ordering {
    let (left_suffix, left_start) = decimal_sort_suffix(left_field_count);
    let (right_suffix, right_start) = decimal_sort_suffix(right_field_count);
    compare_joined_bytes(
        left_tag.as_bytes(),
        &left_suffix[left_start..],
        right_tag.as_bytes(),
        &right_suffix[right_start..],
    )
}

fn decimal_sort_suffix(mut value: usize) -> ([u8; 21], usize) {
    let mut bytes = [0; 21];
    let mut start = bytes.len();
    loop {
        start -= 1;
        bytes[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    start -= 1;
    bytes[start] = b':';
    (bytes, start)
}

fn compare_joined_bytes(
    left_head: &[u8],
    left_tail: &[u8],
    right_head: &[u8],
    right_tail: &[u8],
) -> Ordering {
    let left_len = left_head.len() + left_tail.len();
    let right_len = right_head.len() + right_tail.len();
    for index in 0..left_len.min(right_len) {
        let left = left_head
            .get(index)
            .copied()
            .unwrap_or_else(|| left_tail[index - left_head.len()]);
        let right = right_head
            .get(index)
            .copied()
            .unwrap_or_else(|| right_tail[index - right_head.len()]);
        match left.cmp(&right) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    left_len.cmp(&right_len)
}

fn lookup_hash(value: &(impl Hash + ?Sized)) -> u64 {
    let mut hasher = KernelLookupHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Fast deterministic lookup fingerprint for compiler-owned immutable keys.
///
/// Every hash hit is still checked for exact equality in the arena bucket, so
/// this affects lookup cost only, never canonical identity or output order.
#[derive(Default)]
struct KernelLookupHasher {
    hash: u64,
}

impl KernelLookupHasher {
    const MULTIPLIER: u64 = 0x517c_c1b7_2722_0a95;

    fn add(&mut self, value: u64) {
        self.hash = (self.hash.rotate_left(5) ^ value).wrapping_mul(Self::MULTIPLIER);
    }
}

impl Hasher for KernelLookupHasher {
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, mut bytes: &[u8]) {
        while let Some((chunk, remaining)) = bytes.split_first_chunk::<8>() {
            self.add(u64::from_le_bytes(*chunk));
            bytes = remaining;
        }
        if let Some((chunk, remaining)) = bytes.split_first_chunk::<4>() {
            self.add(u32::from_le_bytes(*chunk).into());
            bytes = remaining;
        }
        if let Some((chunk, remaining)) = bytes.split_first_chunk::<2>() {
            self.add(u16::from_le_bytes(*chunk).into());
            bytes = remaining;
        }
        if let Some(byte) = bytes.first() {
            self.add((*byte).into());
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.add(value.into());
    }

    fn write_u16(&mut self, value: u16) {
        self.add(value.into());
    }

    fn write_u32(&mut self, value: u32) {
        self.add(value.into());
    }

    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    fn write_u128(&mut self, value: u128) {
        self.add(value as u64);
        self.add((value >> 64) as u64);
    }

    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    fn write_i8(&mut self, value: i8) {
        self.add(value as u64);
    }

    fn write_i16(&mut self, value: i16) {
        self.add(value as u64);
    }

    fn write_i32(&mut self, value: i32) {
        self.add(value as u64);
    }

    fn write_i64(&mut self, value: i64) {
        self.add(value as u64);
    }

    fn write_i128(&mut self, value: i128) {
        self.write_u128(value as u128);
    }

    fn write_isize(&mut self, value: isize) {
        self.add(value as u64);
    }
}

const fn flow_mode_rank(mode: FlowMode) -> u8 {
    match mode {
        FlowMode::Continuous => 0,
        FlowMode::TickPresent => 1,
        FlowMode::PresentOrAbsent => 2,
        FlowMode::Absent => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn immutable_terms_record_whether_their_dag_contains_a_variable() {
        let mut arena = TypeTermArena::new();
        let field = arena.intern_name("value");
        let variable = arena.variable(TypeVariableId(7));
        let closed = arena.object([(field, arena.number())], false);
        let open = arena.object([(field, variable)], false);
        let nested = arena.list(open);

        assert!(!arena.has_variable(arena.number()));
        assert!(!arena.has_variable(closed));
        assert!(arena.has_variable(variable));
        assert!(arena.has_variable(open));
        assert!(arena.has_variable(nested));
    }

    #[test]
    fn structural_widen_reuses_one_object_shape() {
        let mut arena = TypeTermArena::new();
        let kind = arena.intern_name("kind");
        let header_variant = arena.variant_tag("Header");
        let header = arena.variant_set([header_variant]);
        let empty_variant = arena.variant_tag("Empty");
        let empty = arena.variant_set([empty_variant]);
        let left = arena.object([(kind, header)], false);
        let right = arena.object([(kind, empty)], false);
        let widened = arena.structural_widen(left, right);

        let Type::Object(shape) = arena.export_checked_type(widened) else {
            panic!("widened records must remain records")
        };
        assert_eq!(
            shape.fields["kind"],
            Type::VariantSet(
                vec![
                    Variant::Tag("Empty".to_owned()),
                    Variant::Tag("Header".to_owned())
                ]
                .into()
            )
        );
    }

    #[test]
    fn structural_widen_ignores_value_placeholders() {
        let mut arena = TypeTermArena::new();
        let kind = arena.intern_name("kind");
        let label_variant = arena.variant_tag("Label");
        let label = arena.variant_set([label_variant]);
        let label = arena.object([(kind, label)], false);
        let unknown = arena.unknown();
        let open = arena.open_object();

        assert_eq!(arena.structural_widen(label, unknown), label);
        assert_eq!(arena.structural_widen(open, label), label);
    }

    #[test]
    fn structural_top_is_not_reused_as_an_open_object_placeholder() {
        let mut arena = TypeTermArena::new();
        let list_item = arena.text();
        let list = arena.list(list_item);
        let tag = arena.variant_tag("NoElement");
        let tag = arena.variant_set([tag]);

        let structural_top = arena.structural_widen(list, tag);
        assert_eq!(
            arena.export_checked_type(structural_top),
            Type::object(ObjectShape::new(BTreeMap::new(), true))
        );

        let record_name = arena.intern_name("value");
        let number = arena.number();
        let record = arena.object([(record_name, number)], false);
        let widened_again = arena.structural_widen(structural_top, record);
        let Type::Object(shape) = arena.export_checked_type(widened_again) else {
            panic!("widening an object top with a record must remain an object")
        };
        assert!(shape.open);
        assert_eq!(shape.fields["value"], Type::Number);
    }

    #[test]
    fn structural_widen_reduces_internal_union_members_before_joining_records() {
        let mut arena = TypeTermArena::new();
        let kind = arena.intern_name("kind");
        let label = arena.intern_name("label");
        let row_variant = arena.variant_tag("Row");
        let row = arena.variant_set([row_variant]);
        let stack_variant = arena.variant_tag("Stack");
        let stack = arena.variant_set([stack_variant]);
        let text = arena.text();
        let row = arena.object([(kind, row), (label, text)], false);
        let stack = arena.object([(kind, stack)], false);
        let alternatives = arena.union([row, stack]);
        let widened = arena.structural_widen(alternatives, row);

        assert_eq!(
            arena.export_checked_type(widened),
            Type::object(ObjectShape::from_ordered_fields(
                [
                    (
                        "kind".to_owned(),
                        Type::VariantSet(
                            vec![
                                Variant::Tag("Row".to_owned()),
                                Variant::Tag("Stack".to_owned()),
                            ]
                            .into(),
                        ),
                    ),
                    ("label".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn checked_types_round_trip_through_interned_terms() {
        let original = Type::List(Type::shared(Type::object(
            ObjectShape::from_ordered_fields(
                [
                    ("name".to_owned(), Type::Text),
                    (
                        "kind".to_owned(),
                        Type::VariantSet(vec![Variant::Tag("Item".to_owned())].into()),
                    ),
                ],
                false,
            ),
        )));
        let mut arena = TypeTermArena::new();
        let mut variables = HashMap::<TypeVar, TypeVariableId>::new();
        let term = arena.import_checked_type(&original, &mut |source| {
            let next = TypeVariableId(
                u32::try_from(variables.len()).expect("test variable count exceeds u32"),
            );
            *variables.entry(source).or_insert(next)
        });
        assert_eq!(arena.export_checked_type(term), original);
    }

    #[test]
    fn imported_abi_variants_keep_authored_order_until_a_join_canonicalizes_them() {
        let original = Type::VariantSet(
            vec![
                Variant::Tagged {
                    tag: "Opened".to_owned(),
                    fields: ObjectShape::from_ordered_fields(
                        [("size".to_owned(), Type::Number)],
                        false,
                    ),
                },
                Variant::Tag("Cancelled".to_owned()),
            ]
            .into(),
        );
        let mut arena = TypeTermArena::new();
        let imported = arena.import_checked_type(&original, &mut |_| {
            unreachable!("fixture has no type variables")
        });
        assert_eq!(arena.export_checked_type(imported), original);

        let pending = arena.variant_tag("NotStarted");
        let pending = arena.variant_set([pending]);
        let joined = arena.union([imported, pending]);
        assert_eq!(
            arena.export_checked_type(joined),
            boon_checked::canonical_union_type(vec![
                original,
                Type::VariantSet(vec![Variant::Tag("NotStarted".to_owned())].into()),
            ])
        );
    }

    #[test]
    fn structural_join_canonicalizes_nested_abi_variants_once() {
        let original = Type::object(ObjectShape::from_ordered_fields(
            [(
                "value".to_owned(),
                Type::VariantSet(
                    vec![
                        Variant::tagged(
                            "Opened".to_owned(),
                            ObjectShape::from_ordered_fields(
                                [("size".to_owned(), Type::Number)],
                                false,
                            ),
                        ),
                        Variant::Tag("Cancelled".to_owned()),
                    ]
                    .into(),
                ),
            )],
            false,
        ));
        let mut arena = TypeTermArena::new();
        let imported = arena.import_checked_type(&original, &mut |_| {
            unreachable!("fixture has no type variables")
        });
        let widened = arena.structural_widen(imported, imported);
        assert_eq!(
            arena.export_checked_type(widened),
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "value".to_owned(),
                    Type::VariantSet(
                        vec![
                            Variant::Tag("Cancelled".to_owned()),
                            Variant::tagged(
                                "Opened".to_owned(),
                                ObjectShape::from_ordered_fields(
                                    [("size".to_owned(), Type::Number)],
                                    false,
                                ),
                            ),
                        ]
                        .into(),
                    ),
                )],
                false,
            ))
        );
        assert_eq!(
            arena.structural_widen(imported, imported),
            widened,
            "repeated structural joins must reuse the cached canonical term"
        );
    }
}
