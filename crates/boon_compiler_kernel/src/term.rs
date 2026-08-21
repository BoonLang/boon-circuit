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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObjectFieldTerm {
    pub name: NameId,
    pub ty: TypeTermId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TypeTerm<'a> {
    Text,
    Number,
    Bytes(BytesTerm),
    Absent,
    VariantSet(&'a [VariantTerm]),
    Object {
        fields: ObjectFields<'a>,
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
        args: &'a [TypeTermId],
        result_mode: FlowMode,
        result: TypeTermId,
    },
    UnresolvedShape(NameId),
    Variable(TypeVariableId),
    Unknown,
    Union(&'a [TypeTermId]),
    Map {
        key: TypeTermId,
        value: TypeTermId,
    },
    Set(TypeTermId),
    Bits(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectFields<'a> {
    canonical: &'a [ObjectFieldTerm],
    semantic_order: &'a [u32],
}

impl Hash for ObjectFields<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.len().hash(state);
        for field in self.iter() {
            field.hash(state);
        }
    }
}

impl<'a> ObjectFields<'a> {
    pub fn len(self) -> usize {
        self.semantic_order.len()
    }

    pub fn is_empty(self) -> bool {
        self.semantic_order.is_empty()
    }

    pub fn iter(self) -> ObjectFieldIter<'a> {
        ObjectFieldIter {
            canonical: self.canonical,
            semantic_order: self.semantic_order.iter(),
        }
    }

    pub(crate) fn canonical_iter(self) -> std::slice::Iter<'a, ObjectFieldTerm> {
        self.canonical.iter()
    }

    pub fn into_vec(self) -> Vec<ObjectFieldTerm> {
        self.iter().copied().collect()
    }
}

impl<'a> IntoIterator for ObjectFields<'a> {
    type Item = &'a ObjectFieldTerm;
    type IntoIter = ObjectFieldIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone, Debug)]
pub struct ObjectFieldIter<'a> {
    canonical: &'a [ObjectFieldTerm],
    semantic_order: std::slice::Iter<'a, u32>,
}

impl<'a> Iterator for ObjectFieldIter<'a> {
    type Item = &'a ObjectFieldTerm;

    fn next(&mut self) -> Option<Self::Item> {
        self.semantic_order
            .next()
            .map(|index| &self.canonical[*index as usize])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.semantic_order.size_hint()
    }
}

impl ExactSizeIterator for ObjectFieldIter<'_> {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TermSpan {
    start: u32,
    len: u32,
}

impl TermSpan {
    fn new(start: usize, len: usize, description: &str) -> Self {
        Self {
            start: u32::try_from(start)
                .unwrap_or_else(|_| panic!("kernel {description} start exceeds u32")),
            len: u32::try_from(len)
                .unwrap_or_else(|_| panic!("kernel {description} length exceeds u32")),
        }
    }

    fn range(self) -> std::ops::Range<usize> {
        let start = self.start as usize;
        start..start + self.len as usize
    }

    pub(crate) const fn len(self) -> usize {
        self.len as usize
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TypeTermTag {
    Text,
    Number,
    Bytes,
    Absent,
    VariantSet,
    Object,
    OpenObjectPlaceholder,
    RenderContract,
    List,
    Function,
    UnresolvedShape,
    Variable,
    Unknown,
    Union,
    Map,
    Set,
    Bits,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TypeTermHeader {
    payload: u64,
    child_start: u32,
    child_len: u32,
    tag: TypeTermTag,
    flags: u8,
    aux: u8,
    _reserved: [u8; 5],
}

const TERM_HAS_VARIABLE: u8 = 1 << 0;
const TERM_OBJECT_OPEN: u8 = 1 << 1;

impl TypeTermHeader {
    const fn span(self) -> TermSpan {
        TermSpan {
            start: self.child_start,
            len: self.child_len,
        }
    }

    const fn has_variable(self) -> bool {
        self.flags & TERM_HAS_VARIABLE != 0
    }

    const fn object_open(self) -> bool {
        self.flags & TERM_OBJECT_OPEN != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ObjectShapeTermRow {
    canonical_fields: TermSpan,
    semantic_order: TermSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NameRow {
    bytes: TermSpan,
    fingerprint: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TypeTermHead {
    Text,
    Number,
    Bytes(BytesTerm),
    Absent,
    VariantSet(TermSpan),
    Object {
        shape: u32,
        open: bool,
    },
    OpenObjectPlaceholder,
    RenderContract,
    List(TypeTermId),
    Function {
        args: TermSpan,
        result_mode: FlowMode,
        result: TypeTermId,
    },
    UnresolvedShape(NameId),
    Variable(TypeVariableId),
    Unknown,
    Union(TermSpan),
    Map {
        key: TypeTermId,
        value: TypeTermId,
    },
    Set(TypeTermId),
    Bits(u32),
}

/// Immutable type DAG used by the inference kernel.
///
/// Hash maps are lookup-only. Canonical output order is derived from the
/// interned terms and source field order, never from hash-table iteration.
#[derive(Clone, Debug)]
pub struct TypeTermArena {
    name_bytes: Vec<u8>,
    names: Vec<NameRow>,
    name_slots: Vec<u32>,
    headers: Vec<TypeTermHeader>,
    children: Vec<TypeTermId>,
    variants: Vec<VariantTerm>,
    object_shapes: Vec<ObjectShapeTermRow>,
    object_fields: Vec<ObjectFieldTerm>,
    semantic_field_order: Vec<u32>,
    term_fingerprints: Vec<u64>,
    term_slots: Vec<u32>,
    lookup_fingerprint_mask: u64,
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

/// Immutable packed type-and-name store retained after solver quiescence.
///
/// Construction-only interning tables, variable lookup rows, and structural
/// widening caches are dropped when this store is created. All snapshots from
/// one solved project share the remaining columns through one `Arc`. `NameId`
/// remains local to this store; it is not the planned cross-phase `SymbolId`.
/// The store deliberately cannot be deep-cloned.
#[derive(Debug)]
pub struct FrozenTypeStore(TypeTermArena);

/// Retained packed-column occupancy after construction indexes are discarded.
///
/// Byte totals cover vector payloads only. They intentionally report both
/// logical length and backing capacity so measurements can distinguish useful
/// packed state from retained allocation slack before any capacity rewrite.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrozenTypeStoreLayout {
    pub term_rows: u64,
    pub term_capacity: u64,
    pub name_rows: u64,
    pub name_capacity: u64,
    pub name_bytes: u64,
    pub name_byte_capacity: u64,
    pub child_rows: u64,
    pub child_capacity: u64,
    pub variant_rows: u64,
    pub variant_capacity: u64,
    pub object_shape_rows: u64,
    pub object_shape_capacity: u64,
    pub object_field_rows: u64,
    pub object_field_capacity: u64,
    pub semantic_order_rows: u64,
    pub semantic_order_capacity: u64,
    pub payload_len_bytes: u64,
    pub payload_capacity_bytes: u64,
}

impl FrozenTypeStore {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn layout(&self) -> FrozenTypeStoreLayout {
        self.0.frozen_layout()
    }

    #[cfg(test)]
    pub(crate) fn term(&self, id: TypeTermId) -> TypeTerm<'_> {
        self.0.term(id)
    }

    #[cfg(test)]
    pub(crate) fn name(&self, id: NameId) -> &str {
        self.0.name(id)
    }

    #[cfg(test)]
    pub(crate) fn export_checked_type(&self, id: TypeTermId) -> Type {
        self.0.export_checked_type(id)
    }

    pub(crate) fn as_arena(&self) -> &TypeTermArena {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn construction_storage_entries(&self) -> usize {
        self.0.name_slots.len()
            + self.0.term_fingerprints.len()
            + self.0.term_slots.len()
            + self.0.variable_terms.len()
            + self.0.structural_widen_cache.len()
    }
}

impl PartialEq for FrozenTypeStore {
    fn eq(&self, other: &Self) -> bool {
        let left = &self.0;
        let right = &other.0;
        left.name_bytes == right.name_bytes
            && left.names.len() == right.names.len()
            && left
                .names
                .iter()
                .zip(&right.names)
                .all(|(left, right)| left.bytes == right.bytes)
            && left.headers == right.headers
            && left.children == right.children
            && left.variants == right.variants
            && left.object_shapes == right.object_shapes
            && left.object_fields == right.object_fields
            && left.semantic_field_order == right.semantic_field_order
            && left.absent == right.absent
            && left.unknown == right.unknown
            && left.text == right.text
            && left.number == right.number
            && left.render_contract == right.render_contract
            && left.open_object == right.open_object
    }
}

impl Eq for FrozenTypeStore {}

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
        Self::with_lookup_fingerprint_mask(u64::MAX)
    }

    fn with_lookup_fingerprint_mask(lookup_fingerprint_mask: u64) -> Self {
        let placeholder = TypeTermId(0);
        let mut arena = Self {
            name_bytes: Vec::new(),
            names: Vec::new(),
            name_slots: vec![0; 8],
            headers: Vec::new(),
            children: Vec::new(),
            variants: Vec::new(),
            object_shapes: Vec::new(),
            object_fields: Vec::new(),
            semantic_field_order: Vec::new(),
            term_fingerprints: Vec::new(),
            term_slots: vec![0; 8],
            lookup_fingerprint_mask,
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
        self.headers.len()
    }

    pub(crate) fn name_count(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
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

    fn frozen_layout(&self) -> FrozenTypeStoreLayout {
        let payload_len_bytes = column_bytes::<TypeTermHeader>(self.headers.len())
            + column_bytes::<NameRow>(self.names.len())
            + column_bytes::<u8>(self.name_bytes.len())
            + column_bytes::<TypeTermId>(self.children.len())
            + column_bytes::<VariantTerm>(self.variants.len())
            + column_bytes::<ObjectShapeTermRow>(self.object_shapes.len())
            + column_bytes::<ObjectFieldTerm>(self.object_fields.len())
            + column_bytes::<u32>(self.semantic_field_order.len());
        let payload_capacity_bytes = column_bytes::<TypeTermHeader>(self.headers.capacity())
            + column_bytes::<NameRow>(self.names.capacity())
            + column_bytes::<u8>(self.name_bytes.capacity())
            + column_bytes::<TypeTermId>(self.children.capacity())
            + column_bytes::<VariantTerm>(self.variants.capacity())
            + column_bytes::<ObjectShapeTermRow>(self.object_shapes.capacity())
            + column_bytes::<ObjectFieldTerm>(self.object_fields.capacity())
            + column_bytes::<u32>(self.semantic_field_order.capacity());
        FrozenTypeStoreLayout {
            term_rows: count_u64(self.headers.len()),
            term_capacity: count_u64(self.headers.capacity()),
            name_rows: count_u64(self.names.len()),
            name_capacity: count_u64(self.names.capacity()),
            name_bytes: count_u64(self.name_bytes.len()),
            name_byte_capacity: count_u64(self.name_bytes.capacity()),
            child_rows: count_u64(self.children.len()),
            child_capacity: count_u64(self.children.capacity()),
            variant_rows: count_u64(self.variants.len()),
            variant_capacity: count_u64(self.variants.capacity()),
            object_shape_rows: count_u64(self.object_shapes.len()),
            object_shape_capacity: count_u64(self.object_shapes.capacity()),
            object_field_rows: count_u64(self.object_fields.len()),
            object_field_capacity: count_u64(self.object_fields.capacity()),
            semantic_order_rows: count_u64(self.semantic_field_order.len()),
            semantic_order_capacity: count_u64(self.semantic_field_order.capacity()),
            payload_len_bytes,
            payload_capacity_bytes,
        }
    }

    /// Consume the mutable solver arena and retain only columns required to
    /// interpret packed type and symbol IDs.
    pub(crate) fn freeze(mut self) -> FrozenTypeStore {
        self.name_slots = Vec::new();
        self.term_fingerprints = Vec::new();
        self.term_slots = Vec::new();
        self.variable_terms = Vec::new();
        self.structural_widen_cache = HashMap::new();
        FrozenTypeStore(self)
    }

    pub fn term(&self, id: TypeTermId) -> TypeTerm<'_> {
        match self.term_head(id) {
            TypeTermHead::Text => TypeTerm::Text,
            TypeTermHead::Number => TypeTerm::Number,
            TypeTermHead::Bytes(bytes) => TypeTerm::Bytes(bytes),
            TypeTermHead::Absent => TypeTerm::Absent,
            TypeTermHead::VariantSet(span) => TypeTerm::VariantSet(self.variant_terms(span)),
            TypeTermHead::Object { shape, open } => TypeTerm::Object {
                fields: self.object_fields_for_shape(shape),
                open,
            },
            TypeTermHead::OpenObjectPlaceholder => TypeTerm::OpenObjectPlaceholder,
            TypeTermHead::RenderContract => TypeTerm::RenderContract,
            TypeTermHead::List(item) => TypeTerm::List(item),
            TypeTermHead::Function {
                args,
                result_mode,
                result,
            } => TypeTerm::Function {
                args: self.term_ids(args),
                result_mode,
                result,
            },
            TypeTermHead::UnresolvedShape(reason) => TypeTerm::UnresolvedShape(reason),
            TypeTermHead::Variable(variable) => TypeTerm::Variable(variable),
            TypeTermHead::Unknown => TypeTerm::Unknown,
            TypeTermHead::Union(span) => TypeTerm::Union(self.term_ids(span)),
            TypeTermHead::Map { key, value } => TypeTerm::Map { key, value },
            TypeTermHead::Set(item) => TypeTerm::Set(item),
            TypeTermHead::Bits(width) => TypeTerm::Bits(width),
        }
    }

    pub(crate) fn term_head(&self, id: TypeTermId) -> TypeTermHead {
        let header = self.headers[id.0 as usize];
        match header.tag {
            TypeTermTag::Text => TypeTermHead::Text,
            TypeTermTag::Number => TypeTermHead::Number,
            TypeTermTag::Bytes => TypeTermHead::Bytes(if header.aux == 0 {
                BytesTerm::Dynamic
            } else {
                BytesTerm::Fixed(
                    usize::try_from(header.payload)
                        .expect("kernel fixed byte-list size exceeds usize"),
                )
            }),
            TypeTermTag::Absent => TypeTermHead::Absent,
            TypeTermTag::VariantSet => TypeTermHead::VariantSet(header.span()),
            TypeTermTag::Object => TypeTermHead::Object {
                shape: header.payload as u32,
                open: header.object_open(),
            },
            TypeTermTag::OpenObjectPlaceholder => TypeTermHead::OpenObjectPlaceholder,
            TypeTermTag::RenderContract => TypeTermHead::RenderContract,
            TypeTermTag::List => TypeTermHead::List(TypeTermId(header.payload as u32)),
            TypeTermTag::Function => TypeTermHead::Function {
                args: header.span(),
                result_mode: decode_flow_mode(header.aux),
                result: TypeTermId(header.payload as u32),
            },
            TypeTermTag::UnresolvedShape => {
                TypeTermHead::UnresolvedShape(NameId(header.payload as u32))
            }
            TypeTermTag::Variable => TypeTermHead::Variable(TypeVariableId(header.payload as u32)),
            TypeTermTag::Unknown => TypeTermHead::Unknown,
            TypeTermTag::Union => TypeTermHead::Union(header.span()),
            TypeTermTag::Map => {
                let (key, value) = unpack_type_pair(header.payload);
                TypeTermHead::Map { key, value }
            }
            TypeTermTag::Set => TypeTermHead::Set(TypeTermId(header.payload as u32)),
            TypeTermTag::Bits => TypeTermHead::Bits(header.payload as u32),
        }
    }

    pub(crate) fn term_ids(&self, span: TermSpan) -> &[TypeTermId] {
        &self.children[span.range()]
    }

    pub(crate) fn variant_terms(&self, span: TermSpan) -> &[VariantTerm] {
        &self.variants[span.range()]
    }

    pub(crate) fn object_fields_for_shape(&self, shape: u32) -> ObjectFields<'_> {
        let shape = self.object_shapes[shape as usize];
        ObjectFields {
            canonical: &self.object_fields[shape.canonical_fields.range()],
            semantic_order: &self.semantic_field_order[shape.semantic_order.range()],
        }
    }

    pub(crate) fn lookup_object_field(&self, shape: u32, name: NameId) -> Option<TypeTermId> {
        let shape = self.object_shapes[shape as usize];
        let fields = &self.object_fields[shape.canonical_fields.range()];
        fields
            .binary_search_by(|field| self.name(field.name).cmp(self.name(name)))
            .ok()
            .map(|index| fields[index].ty)
    }

    /// Whether this immutable term DAG contains any occurrence-local variable.
    /// Closed terms can bypass solver resolution and dependency traversal.
    pub(crate) fn has_variable(&self, id: TypeTermId) -> bool {
        self.headers[id.0 as usize].has_variable()
    }

    pub fn name(&self, id: NameId) -> &str {
        let row = self.names[id.0 as usize];
        std::str::from_utf8(&self.name_bytes[row.bytes.range()])
            .expect("kernel names were interned from valid UTF-8")
    }

    pub fn intern_name(&mut self, name: impl AsRef<str>) -> NameId {
        let name = name.as_ref();
        let hash = lookup_hash(name) & self.lookup_fingerprint_mask;
        if let Some(id) = self.find_name(hash, name.as_bytes()) {
            return id;
        }
        let id = NameId(u32::try_from(self.names.len()).expect("kernel name count exceeds u32"));
        self.ensure_name_slot_capacity();
        let bytes = TermSpan::new(self.name_bytes.len(), name.len(), "name byte span");
        self.name_bytes.extend_from_slice(name.as_bytes());
        self.names.push(NameRow {
            bytes,
            fingerprint: hash,
        });
        self.insert_name_slot(id, hash);
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
        let args = args.into_iter().collect::<Vec<_>>();
        self.intern_raw(TypeTerm::Function {
            args: &args,
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
            let replacement = match (merged[index], incoming) {
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
                    tag,
                    fields: self.structural_widen(existing, incoming),
                }),
            };
            if let Some(replacement) = replacement {
                merged[index] = replacement;
            }
        }
        if canonicalize {
            merged.sort_by(|left, right| self.compare_variants_canonically(left, right));
        }
        self.intern_raw(TypeTerm::VariantSet(&merged))
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
            match self.term_head(candidate) {
                TypeTermHead::Absent => {}
                TypeTermHead::Union(nested) => {
                    pending.extend(self.term_ids(nested).iter().rev().copied())
                }
                TypeTermHead::VariantSet(incoming) => {
                    variants.extend_from_slice(self.variant_terms(incoming))
                }
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
            _ => self.intern_raw(TypeTerm::Union(&members)),
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
        let left_term = self.term_head(left);
        let right_term = self.term_head(right);
        if is_value_placeholder_head(left_term) {
            return right;
        }
        if is_value_placeholder_head(right_term) {
            return left;
        }
        match (left_term, right_term) {
            (TypeTermHead::Absent, _) => right,
            (_, TypeTermHead::Absent) => left,
            (TypeTermHead::Union(members), _) => self
                .term_ids(members)
                .to_vec()
                .into_iter()
                .fold(right, |widened, member| {
                    self.structural_widen(widened, member)
                }),
            (_, TypeTermHead::Union(members)) => self
                .term_ids(members)
                .to_vec()
                .into_iter()
                .fold(left, |widened, member| {
                    self.structural_widen(widened, member)
                }),
            (TypeTermHead::VariantSet(left), TypeTermHead::VariantSet(right)) => {
                let left = self.variant_terms(left).to_vec();
                let right = self.variant_terms(right).to_vec();
                self.variant_set(left.into_iter().chain(right))
            }
            (TypeTermHead::Bytes(left), TypeTermHead::Bytes(right)) => {
                let bytes = if left == right {
                    left
                } else {
                    BytesTerm::Dynamic
                };
                self.bytes(bytes)
            }
            (TypeTermHead::Bits(left), TypeTermHead::Bits(right)) if left == right => {
                self.bits(left)
            }
            (TypeTermHead::List(left), TypeTermHead::List(right)) => {
                let item = self.structural_widen(left, right);
                self.list(item)
            }
            (TypeTermHead::Set(left), TypeTermHead::Set(right)) => {
                let item = self.structural_widen(left, right);
                self.set(item)
            }
            (
                TypeTermHead::Map {
                    key: left_key,
                    value: left_value,
                },
                TypeTermHead::Map {
                    key: right_key,
                    value: right_value,
                },
            ) => {
                let key = self.structural_widen(left_key, right_key);
                let value = self.structural_widen(left_value, right_value);
                self.map(key, value)
            }
            (
                TypeTermHead::Object {
                    shape: left_shape,
                    open: left_open,
                },
                TypeTermHead::Object {
                    shape: right_shape,
                    open: right_open,
                },
            ) => {
                let mut fields = self.object_fields_for_shape(left_shape).into_vec();
                let right_fields = self.object_fields_for_shape(right_shape).into_vec();
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
        let imported = match source.term(term) {
            TypeTerm::Text => self.text(),
            TypeTerm::Number => self.number(),
            TypeTerm::Bytes(bytes) => self.bytes(bytes),
            TypeTerm::Absent => self.absent(),
            TypeTerm::VariantSet(variants) => {
                let variants = variants
                    .to_vec()
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
            TypeTerm::Bytes(BytesTerm::Fixed(size)) => Type::Bytes(BytesType::Fixed(size)),
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
                open,
            )),
            TypeTerm::OpenObjectPlaceholder => {
                Type::object(ObjectShape::new(std::collections::BTreeMap::new(), true))
            }
            TypeTerm::RenderContract => Type::RenderContract,
            TypeTerm::List(item) => Type::List(Type::shared(self.export_checked_type_inner(item))),
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
                    mode: result_mode,
                    ty: self.export_checked_type_inner(result),
                }),
            },
            TypeTerm::UnresolvedShape(reason) => Type::UnresolvedShape {
                reason: self.name(reason).to_owned(),
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
                key: Box::new(self.export_checked_type_inner(key)),
                value: Box::new(self.export_checked_type_inner(value)),
            },
            TypeTerm::Set(item) => Type::Set(Type::shared(self.export_checked_type_inner(item))),
            TypeTerm::Bits(width) => Type::Bits { width },
        }
    }

    fn intern_raw(&mut self, term: TypeTerm<'_>) -> TypeTermId {
        self.work.intern_requests = self.work.intern_requests.saturating_add(1);
        let work_kind = term_work_kind(term);
        self.work.intern_requests_by_kind[work_kind] =
            self.work.intern_requests_by_kind[work_kind].saturating_add(1);
        let hash = lookup_hash(&term) & self.lookup_fingerprint_mask;
        if let Some(id) = self.find_term(hash, term) {
            self.work.intern_hits = self.work.intern_hits.saturating_add(1);
            self.work.intern_hits_by_kind[work_kind] =
                self.work.intern_hits_by_kind[work_kind].saturating_add(1);
            return id;
        }
        let has_variable = match term {
            TypeTerm::Variable(_) => true,
            TypeTerm::VariantSet(variants) => variants.iter().any(|variant| match variant {
                VariantTerm::Tag(_) => false,
                VariantTerm::Tagged { fields, .. } => self.has_variable(*fields),
            }),
            TypeTerm::Object { fields, .. } => {
                fields.iter().any(|field| self.has_variable(field.ty))
            }
            TypeTerm::List(item) | TypeTerm::Set(item) => self.has_variable(item),
            TypeTerm::Function { args, result, .. } => {
                args.iter().any(|argument| self.has_variable(*argument))
                    || self.has_variable(result)
            }
            TypeTerm::Union(members) => members.iter().any(|member| self.has_variable(*member)),
            TypeTerm::Map { key, value } => self.has_variable(key) || self.has_variable(value),
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
        let variable_flag = u8::from(has_variable) * TERM_HAS_VARIABLE;
        let header = match term {
            TypeTerm::Text => term_header(TypeTermTag::Text, 0, 0, TermSpan { start: 0, len: 0 }),
            TypeTerm::Number => {
                term_header(TypeTermTag::Number, 0, 0, TermSpan { start: 0, len: 0 })
            }
            TypeTerm::Bytes(bytes) => match bytes {
                BytesTerm::Dynamic => {
                    term_header(TypeTermTag::Bytes, 0, 0, TermSpan { start: 0, len: 0 })
                }
                BytesTerm::Fixed(size) => TypeTermHeader {
                    payload: u64::try_from(size).expect("kernel fixed byte-list size exceeds u64"),
                    child_start: 0,
                    child_len: 0,
                    tag: TypeTermTag::Bytes,
                    flags: 0,
                    aux: 1,
                    _reserved: [0; 5],
                },
            },
            TypeTerm::Absent => {
                term_header(TypeTermTag::Absent, 0, 0, TermSpan { start: 0, len: 0 })
            }
            TypeTerm::VariantSet(variants) => {
                let span = TermSpan::new(self.variants.len(), variants.len(), "variant span");
                self.variants.extend_from_slice(variants);
                term_header(TypeTermTag::VariantSet, 0, variable_flag, span)
            }
            TypeTerm::Object { .. } => {
                unreachable!("objects use their canonical-column interner")
            }
            TypeTerm::OpenObjectPlaceholder => term_header(
                TypeTermTag::OpenObjectPlaceholder,
                0,
                0,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::RenderContract => term_header(
                TypeTermTag::RenderContract,
                0,
                0,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::List(item) => term_header(
                TypeTermTag::List,
                u64::from(item.0),
                variable_flag,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::Function {
                args,
                result_mode,
                result,
            } => {
                let span = TermSpan::new(self.children.len(), args.len(), "function argument span");
                self.children.extend_from_slice(args);
                let mut header = term_header(
                    TypeTermTag::Function,
                    u64::from(result.0),
                    variable_flag,
                    span,
                );
                header.aux = encode_flow_mode(result_mode);
                header
            }
            TypeTerm::UnresolvedShape(reason) => term_header(
                TypeTermTag::UnresolvedShape,
                u64::from(reason.0),
                0,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::Variable(variable) => term_header(
                TypeTermTag::Variable,
                u64::from(variable.0),
                TERM_HAS_VARIABLE,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::Unknown => {
                term_header(TypeTermTag::Unknown, 0, 0, TermSpan { start: 0, len: 0 })
            }
            TypeTerm::Union(members) => {
                let span = TermSpan::new(self.children.len(), members.len(), "union member span");
                self.children.extend_from_slice(members);
                term_header(TypeTermTag::Union, 0, variable_flag, span)
            }
            TypeTerm::Map { key, value } => term_header(
                TypeTermTag::Map,
                pack_type_pair(key, value),
                variable_flag,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::Set(item) => term_header(
                TypeTermTag::Set,
                u64::from(item.0),
                variable_flag,
                TermSpan { start: 0, len: 0 },
            ),
            TypeTerm::Bits(width) => term_header(
                TypeTermTag::Bits,
                u64::from(width),
                0,
                TermSpan { start: 0, len: 0 },
            ),
        };
        self.append_term(header, hash)
    }

    fn intern_object(&mut self, semantic_fields: Vec<ObjectFieldTerm>, open: bool) -> TypeTermId {
        const OBJECT_WORK_KIND: usize = 1;
        self.work.intern_requests = self.work.intern_requests.saturating_add(1);
        self.work.intern_requests_by_kind[OBJECT_WORK_KIND] =
            self.work.intern_requests_by_kind[OBJECT_WORK_KIND].saturating_add(1);
        let mut canonical_fields = semantic_fields.clone();
        canonical_fields
            .sort_unstable_by(|left, right| self.name(left.name).cmp(self.name(right.name)));
        let semantic_order = semantic_fields
            .iter()
            .map(|field| {
                u32::try_from(
                    canonical_fields
                        .iter()
                        .position(|candidate| candidate.name == field.name)
                        .expect("semantic object field exists in canonical fields"),
                )
                .expect("kernel object field count exceeds u32")
            })
            .collect::<Vec<_>>();
        let candidate_fields = ObjectFields {
            canonical: &canonical_fields,
            semantic_order: &semantic_order,
        };
        let candidate = TypeTerm::Object {
            fields: candidate_fields,
            open,
        };
        let hash = lookup_hash(&candidate) & self.lookup_fingerprint_mask;
        if let Some(id) = self.find_term(hash, candidate) {
            self.work.intern_hits = self.work.intern_hits.saturating_add(1);
            self.work.intern_hits_by_kind[OBJECT_WORK_KIND] =
                self.work.intern_hits_by_kind[OBJECT_WORK_KIND].saturating_add(1);
            return id;
        }
        let canonical_span = TermSpan::new(
            self.object_fields.len(),
            canonical_fields.len(),
            "canonical object field span",
        );
        self.object_fields.extend_from_slice(&canonical_fields);
        let order_span = TermSpan::new(
            self.semantic_field_order.len(),
            semantic_order.len(),
            "semantic object field-order span",
        );
        self.semantic_field_order.extend_from_slice(&semantic_order);
        let shape =
            u32::try_from(self.object_shapes.len()).expect("kernel object shape count exceeds u32");
        self.object_shapes.push(ObjectShapeTermRow {
            canonical_fields: canonical_span,
            semantic_order: order_span,
        });
        let has_variable = canonical_fields
            .iter()
            .any(|field| self.has_variable(field.ty));
        let flags = u8::from(has_variable) * TERM_HAS_VARIABLE | u8::from(open) * TERM_OBJECT_OPEN;
        self.append_term(
            term_header(
                TypeTermTag::Object,
                u64::from(shape),
                flags,
                TermSpan { start: 0, len: 0 },
            ),
            hash,
        )
    }

    fn find_name(&self, fingerprint: u64, bytes: &[u8]) -> Option<NameId> {
        let mut slot = fingerprint as usize & (self.name_slots.len() - 1);
        loop {
            let encoded = self.name_slots[slot];
            if encoded == 0 {
                return None;
            }
            let id = NameId(encoded - 1);
            let row = self.names[id.0 as usize];
            if row.fingerprint == fingerprint && &self.name_bytes[row.bytes.range()] == bytes {
                return Some(id);
            }
            slot = (slot + 1) & (self.name_slots.len() - 1);
        }
    }

    fn ensure_name_slot_capacity(&mut self) {
        if (self.names.len() + 1) * 10 < self.name_slots.len() * 7 {
            return;
        }
        self.name_slots = vec![0; self.name_slots.len() * 2];
        for index in 0..self.names.len() {
            let id = NameId(u32::try_from(index).expect("kernel name count exceeds u32"));
            self.insert_name_slot(id, self.names[index].fingerprint);
        }
    }

    fn insert_name_slot(&mut self, id: NameId, fingerprint: u64) {
        let mut slot = fingerprint as usize & (self.name_slots.len() - 1);
        while self.name_slots[slot] != 0 {
            slot = (slot + 1) & (self.name_slots.len() - 1);
        }
        self.name_slots[slot] =
            id.0.checked_add(1)
                .expect("kernel name slots reserve u32::MAX");
    }

    fn find_term(&self, fingerprint: u64, candidate: TypeTerm<'_>) -> Option<TypeTermId> {
        let mut slot = fingerprint as usize & (self.term_slots.len() - 1);
        loop {
            let encoded = self.term_slots[slot];
            if encoded == 0 {
                return None;
            }
            let id = TypeTermId(encoded - 1);
            if self.term_fingerprints[id.0 as usize] == fingerprint && self.term(id) == candidate {
                return Some(id);
            }
            slot = (slot + 1) & (self.term_slots.len() - 1);
        }
    }

    fn append_term(&mut self, header: TypeTermHeader, fingerprint: u64) -> TypeTermId {
        self.ensure_term_slot_capacity();
        let id =
            TypeTermId(u32::try_from(self.headers.len()).expect("kernel term count exceeds u32"));
        self.headers.push(header);
        self.term_fingerprints.push(fingerprint);
        self.insert_term_slot(id, fingerprint);
        id
    }

    fn ensure_term_slot_capacity(&mut self) {
        if (self.headers.len() + 1) * 10 < self.term_slots.len() * 7 {
            return;
        }
        self.term_slots = vec![0; self.term_slots.len() * 2];
        for index in 0..self.headers.len() {
            let id = TypeTermId(u32::try_from(index).expect("kernel term count exceeds u32"));
            self.insert_term_slot(id, self.term_fingerprints[index]);
        }
    }

    fn insert_term_slot(&mut self, id: TypeTermId, fingerprint: u64) {
        let mut slot = fingerprint as usize & (self.term_slots.len() - 1);
        while self.term_slots[slot] != 0 {
            slot = (slot + 1) & (self.term_slots.len() - 1);
        }
        self.term_slots[slot] =
            id.0.checked_add(1)
                .expect("kernel term slots reserve u32::MAX");
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
                (TypeTerm::Bytes(left), TypeTerm::Bytes(right)) => left.cmp(&right),
                (TypeTerm::Bits(left), TypeTerm::Bits(right)) => left.cmp(&right),
                (TypeTerm::Variable(left), TypeTerm::Variable(right)) => left.cmp(&right),
                (TypeTerm::UnresolvedShape(left), TypeTerm::UnresolvedShape(right)) => {
                    self.name(left).cmp(self.name(right))
                }
                (TypeTerm::List(left), TypeTerm::List(right))
                | (TypeTerm::Set(left), TypeTerm::Set(right)) => self.compare_terms(left, right),
                (TypeTerm::Map { key: lk, value: lv }, TypeTerm::Map { key: rk, value: rv }) => {
                    self.compare_terms(lk, rk)
                        .then_with(|| self.compare_terms(lv, rv))
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
                    .then_with(|| lo.cmp(&ro)),
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
                    .then_with(|| flow_mode_rank(lm).cmp(&flow_mode_rank(rm)))
                    .then_with(|| self.compare_terms(lr, rr)),
                _ => Ordering::Equal,
            })
    }

    fn term_rank(&self, term: TypeTerm<'_>) -> u8 {
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

    fn compare_field_slices(&self, left: ObjectFields<'_>, right: ObjectFields<'_>) -> Ordering {
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

fn count_u64(value: usize) -> u64 {
    u64::try_from(value).expect("kernel packed-column count exceeds u64")
}

fn column_bytes<T>(count: usize) -> u64 {
    count_u64(count)
        .checked_mul(count_u64(std::mem::size_of::<T>()))
        .expect("kernel packed-column byte count exceeds u64")
}

const fn term_header(tag: TypeTermTag, payload: u64, flags: u8, span: TermSpan) -> TypeTermHeader {
    TypeTermHeader {
        payload,
        child_start: span.start,
        child_len: span.len,
        tag,
        flags,
        aux: 0,
        _reserved: [0; 5],
    }
}

const fn pack_type_pair(left: TypeTermId, right: TypeTermId) -> u64 {
    left.0 as u64 | ((right.0 as u64) << 32)
}

const fn unpack_type_pair(value: u64) -> (TypeTermId, TypeTermId) {
    (TypeTermId(value as u32), TypeTermId((value >> 32) as u32))
}

const fn encode_flow_mode(mode: FlowMode) -> u8 {
    match mode {
        FlowMode::Continuous => 0,
        FlowMode::TickPresent => 1,
        FlowMode::PresentOrAbsent => 2,
        FlowMode::Absent => 3,
    }
}

const fn decode_flow_mode(mode: u8) -> FlowMode {
    match mode {
        0 => FlowMode::Continuous,
        1 => FlowMode::TickPresent,
        2 => FlowMode::PresentOrAbsent,
        3 => FlowMode::Absent,
        _ => panic!("invalid packed kernel flow mode"),
    }
}

const fn is_value_placeholder_head(term: TypeTermHead) -> bool {
    matches!(
        term,
        TypeTermHead::Variable(_)
            | TypeTermHead::Unknown
            | TypeTermHead::UnresolvedShape(_)
            | TypeTermHead::OpenObjectPlaceholder
    )
}

fn term_work_kind(term: TypeTerm<'_>) -> usize {
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
/// Every hash hit is still checked for exact equality while probing, so
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
    fn packed_production_rows_have_fixed_layouts() {
        assert_eq!(std::mem::size_of::<TypeTermHeader>(), 24);
        assert_eq!(std::mem::size_of::<ObjectFieldTerm>(), 8);
        assert_eq!(std::mem::size_of::<ObjectShapeTermRow>(), 16);
        assert_eq!(std::mem::size_of::<NameRow>(), 16);
    }

    #[test]
    fn freezing_drops_construction_indexes_and_preserves_packed_views() {
        let mut arena = TypeTermArena::new();
        let value = arena.intern_name("value");
        let variable = arena.variable(TypeVariableId(73));
        let record = arena.object([(value, variable)], true);
        let number = arena.number();
        let widened = arena.structural_widen(record, number);
        assert!(!arena.name_slots.is_empty());
        assert!(!arena.term_slots.is_empty());
        assert!(!arena.term_fingerprints.is_empty());
        assert!(!arena.variable_terms.is_empty());
        assert!(!arena.structural_widen_cache.is_empty());

        let frozen = arena.freeze();
        assert_eq!(frozen.construction_storage_entries(), 0);
        let layout = frozen.layout();
        assert_eq!(layout.term_rows, frozen.len() as u64);
        assert!(layout.term_capacity >= layout.term_rows);
        assert!(layout.name_capacity >= layout.name_rows);
        assert!(layout.name_byte_capacity >= layout.name_bytes);
        assert!(layout.payload_capacity_bytes >= layout.payload_len_bytes);
        assert_eq!(frozen.name(value), "value");
        assert_eq!(
            frozen.export_checked_type(record),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Var(TypeVar(73)))],
                true,
            ))
        );
        assert!(matches!(frozen.term(widened), TypeTerm::Object { .. }));
    }

    #[test]
    fn frozen_semantic_equality_ignores_construction_fingerprint_mask() {
        let build = |mask| {
            let mut arena = TypeTermArena::with_lookup_fingerprint_mask(mask);
            let alpha = arena.intern_name("alpha");
            let beta = arena.intern_name("beta");
            let text = arena.text();
            let number = arena.number();
            let _ = arena.object([(alpha, text), (beta, number)], false);
            arena.freeze()
        };

        assert_eq!(build(u64::MAX), build(0));
    }

    #[test]
    fn packed_production_indexes_are_collision_exact_and_hit_stable() {
        let mut arena = TypeTermArena::with_lookup_fingerprint_mask(0);
        assert_ne!(arena.absent(), arena.unknown());
        assert_ne!(arena.text(), arena.number());

        let alpha = arena.intern_name("alpha");
        let beta = arena.intern_name("beta");
        assert_ne!(alpha, beta);
        assert_eq!(arena.intern_name("alpha"), alpha);
        let colliding_names = (0..48)
            .map(|index| {
                let name = format!("production_collision_name_{index}");
                let id = arena.intern_name(&name);
                (name, id)
            })
            .collect::<Vec<_>>();
        for (name, id) in &colliding_names {
            assert_eq!(arena.intern_name(name), *id);
        }

        let number = arena.number();
        let text = arena.text();
        let first = arena.object([(alpha, number), (beta, text)], false);
        let reversed = arena.object([(beta, text), (alpha, number)], false);
        assert_ne!(first, reversed);
        let TypeTermHead::Object { shape, .. } = arena.term_head(first) else {
            unreachable!("production object constructor returned a non-object term")
        };
        assert_eq!(arena.lookup_object_field(shape, alpha), Some(number));
        assert_eq!(arena.lookup_object_field(shape, beta), Some(text));
        let missing = arena.intern_name("missing");
        assert_eq!(arena.lookup_object_field(shape, missing), None);
        let function = arena.function([first, reversed], FlowMode::TickPresent, text);
        let pair = arena.tagged_variant("Pair", first);
        let variants = arena.variant_set([pair]);
        let union = arena.union([function, variants]);
        let storage_lengths = (
            arena.name_bytes.len(),
            arena.names.len(),
            arena.headers.len(),
            arena.children.len(),
            arena.variants.len(),
            arena.object_shapes.len(),
            arena.object_fields.len(),
            arena.semantic_field_order.len(),
        );

        assert_eq!(arena.object([(alpha, number), (beta, text)], false), first);
        assert_eq!(
            arena.function([first, reversed], FlowMode::TickPresent, text),
            function
        );
        let repeated_pair = arena.tagged_variant("Pair", first);
        assert_eq!(arena.variant_set([repeated_pair]), variants);
        assert_eq!(arena.union([function, variants]), union);
        assert_eq!(
            (
                arena.name_bytes.len(),
                arena.names.len(),
                arena.headers.len(),
                arena.children.len(),
                arena.variants.len(),
                arena.object_shapes.len(),
                arena.object_fields.len(),
                arena.semantic_field_order.len(),
            ),
            storage_lengths
        );

        let colliding_terms = (0..96).map(|width| arena.bits(width)).collect::<Vec<_>>();
        for (width, id) in colliding_terms.into_iter().enumerate() {
            assert_eq!(arena.bits(width as u32), id);
        }
    }

    #[test]
    fn packed_objects_keep_canonical_lookup_separate_from_authored_order() {
        let mut arena = TypeTermArena::new();
        let z = arena.intern_name("z");
        let a = arena.intern_name("a");
        let number = arena.number();
        let text = arena.text();
        let za = arena.object([(z, number), (a, text)], false);
        let az = arena.object([(a, text), (z, number)], false);

        assert_ne!(za, az);
        let TypeTerm::Object { fields, .. } = arena.term(za) else {
            unreachable!()
        };
        assert_eq!(
            fields.iter().map(|field| field.name).collect::<Vec<_>>(),
            [z, a]
        );
        assert_eq!(
            fields
                .canonical_iter()
                .map(|field| field.name)
                .collect::<Vec<_>>(),
            [a, z]
        );
        assert_eq!(
            fields
                .canonical_iter()
                .find(|field| field.name == z)
                .map(|field| field.ty),
            Some(number)
        );
    }

    #[test]
    fn packed_variants_preserve_authored_order_until_a_structural_join() {
        let mut arena = TypeTermArena::new();
        let field = arena.intern_name("value");
        let text = arena.text();
        let payload = arena.object([(field, text)], false);
        let tagged = arena.tagged_variant("Zulu", payload);
        let bare = arena.variant_tag("Alpha");
        let authored = arena.variant_set_preserving_order([tagged, bare]);
        let canonical = arena.variant_set([tagged, bare]);

        let TypeTerm::VariantSet(authored_rows) = arena.term(authored) else {
            unreachable!()
        };
        let TypeTerm::VariantSet(canonical_rows) = arena.term(canonical) else {
            unreachable!()
        };
        assert_eq!(arena.name(authored_rows[0].tag()), "Zulu");
        assert_eq!(arena.name(authored_rows[1].tag()), "Alpha");
        assert_eq!(arena.name(canonical_rows[0].tag()), "Alpha");
        assert_eq!(arena.name(canonical_rows[1].tag()), "Zulu");
    }

    #[test]
    fn packed_union_order_is_deterministic_for_adversarial_kinds() {
        let mut arena = TypeTermArena::new();
        let field = arena.intern_name("value");
        let text = arena.text();
        let number = arena.number();
        let record = arena.object([(field, text)], false);
        let list = arena.list(text);
        let set = arena.set(number);
        let map = arena.map(text, number);
        let function = arena.function([record, list], FlowMode::TickPresent, map);
        let tag = arena.variant_tag("Item");
        let variants = arena.variant_set([tag]);
        let unresolved = arena.unresolved_shape("later");
        let variable = arena.variable(TypeVariableId(4));
        let dynamic_bytes = arena.bytes(BytesTerm::Dynamic);
        let bits = arena.bits(7);
        let candidates = [
            arena.absent(),
            text,
            number,
            dynamic_bytes,
            bits,
            variants,
            record,
            arena.open_object(),
            arena.render_contract(),
            list,
            set,
            map,
            function,
            unresolved,
            variable,
            arena.unknown(),
        ];
        let forward = arena.union(candidates);
        let reverse = arena.union(candidates.into_iter().rev());
        assert_eq!(forward, reverse);
        assert_eq!(
            arena.export_checked_type(forward),
            arena.export_checked_type(reverse)
        );
    }

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
