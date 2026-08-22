use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

const ROOT_PATH_DIGEST_DOMAIN: &[u8] = b"boon.compiler.path.root.v1\0";
const PATH_DIGEST_DOMAIN: &[u8] = b"boon.compiler.path.v1\0";
const EMPTY_SLOT: u32 = u32::MAX;
static NEXT_TEXT_AUTHORITY: AtomicU64 = AtomicU64::new(1);

/// Session-local coordinate for one identifier-like UTF-8 atom.
///
/// Numeric coordinates are deliberately not serializable and never own stable
/// identity. Receipts and deterministic publication use the resolved bytes.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SymbolId(u32);

impl SymbolId {
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Session-local coordinate for one parent-linked symbol path.
///
/// `PathId` is meaningful only together with the exact catalog that produced
/// it. It identifies text segments, not a resolved declaration or lexical
/// entity; those require their source/scope/link identity. The root path is
/// always row zero.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PathId(u32);

impl PathId {
    pub const ROOT: Self = Self(0);

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SymbolRow {
    byte_start: u32,
    byte_len: u32,
}

impl SymbolRow {
    fn bytes(self) -> std::ops::Range<usize> {
        let start = self.byte_start as usize;
        start..start + self.byte_len as usize
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PathRow {
    parent: PathId,
    segment: u32,
    depth: u32,
    stable_digest: [u8; 32],
}

impl PathRow {
    pub const fn parent(self) -> Option<PathId> {
        if self.segment == EMPTY_SLOT {
            None
        } else {
            Some(self.parent)
        }
    }

    pub const fn segment(self) -> Option<SymbolId> {
        if self.segment == EMPTY_SLOT {
            None
        } else {
            Some(SymbolId(self.segment))
        }
    }

    pub const fn depth(self) -> u32 {
        self.depth
    }

    pub const fn stable_digest(self) -> [u8; 32] {
        self.stable_digest
    }
}

/// One immutable, flat symbol/path catalog shared by compiler phases.
///
/// The catalog owns one UTF-8 byte slab, one fixed-width symbol column, and
/// one fixed-width parent-linked path column. Path construction indexes and
/// fingerprints are discarded on freeze. The exact symbol lookup table is
/// retained temporarily while the rich compiler-input adapter still presents
/// strings; direct packed-syntax input will make that table removable too.
#[derive(Debug, Eq, PartialEq)]
pub struct PackedTextCatalog {
    bytes: Box<[u8]>,
    symbols: Box<[SymbolRow]>,
    /// Lexical rank of each `SymbolId`, computed once when the project text
    /// authority freezes. Canonical type rows compare four-byte ranks instead
    /// of repeatedly chasing and comparing UTF-8 slices in solver hot loops.
    symbol_lexical_ranks: Box<[u32]>,
    symbol_slots: Box<[u32]>,
    paths: Box<[PathRow]>,
    /// Transitional exact lookup index for the rich-input consumer.
    ///
    /// Packed syntax writes `PathId` while parsing and will make this column
    /// unnecessary.  Until that producer lands, retaining one project-wide
    /// open-addressed index lets the consuming compatibility boundary replace
    /// every boxed path with its dense coordinate without allocating a second
    /// path map or linearly scanning the path store.
    path_slots: Box<[u32]>,
}

impl PackedTextCatalog {
    fn symbol_id(&self, ordinal: usize) -> Option<SymbolId> {
        (ordinal < self.symbols.len()).then(|| SymbolId(ordinal as u32))
    }

    fn symbol(&self, id: SymbolId) -> Option<&str> {
        let row = *self.symbols.get(id.as_usize())?;
        std::str::from_utf8(&self.bytes[row.bytes()]).ok()
    }

    fn lookup_symbol(&self, value: &str) -> Option<SymbolId> {
        let mut slot = lookup_hash(value.as_bytes()) as usize & (self.symbol_slots.len() - 1);
        loop {
            let encoded = self.symbol_slots[slot];
            if encoded == EMPTY_SLOT {
                return None;
            }
            let id = SymbolId(encoded);
            let row = self.symbols[id.as_usize()];
            if &self.bytes[row.bytes()] == value.as_bytes() {
                return Some(id);
            }
            slot = (slot + 1) & (self.symbol_slots.len() - 1);
        }
    }

    fn compare_symbols(
        &self,
        left: SymbolId,
        right: SymbolId,
    ) -> Result<Ordering, TextCatalogError> {
        let left = self.symbol_lexical_rank(left).ok_or_else(|| {
            TextCatalogError::new(format!("symbol {} is outside the catalog", left.0))
        })?;
        let right = self.symbol_lexical_rank(right).ok_or_else(|| {
            TextCatalogError::new(format!("symbol {} is outside the catalog", right.0))
        })?;
        Ok(left.cmp(&right))
    }

    fn symbol_lexical_rank(&self, id: SymbolId) -> Option<u32> {
        self.symbol_lexical_ranks.get(id.as_usize()).copied()
    }

    fn path(&self, id: PathId) -> Option<PathRow> {
        self.paths.get(id.as_usize()).copied()
    }

    fn path_digest(&self, id: PathId) -> Option<[u8; 32]> {
        self.path(id).map(PathRow::stable_digest)
    }

    fn lookup_path<'a>(&self, segments: impl IntoIterator<Item = &'a str>) -> Option<PathId> {
        let mut path = PathId::ROOT;
        for segment in segments {
            let symbol = self.lookup_symbol(segment)?;
            let fingerprint = path_lookup_hash(path, symbol);
            let mut slot = fingerprint as usize & (self.path_slots.len() - 1);
            loop {
                let encoded = self.path_slots[slot];
                if encoded == EMPTY_SLOT {
                    return None;
                }
                let candidate = PathId(encoded);
                let row = self.paths[candidate.as_usize()];
                if row.parent == path && row.segment == symbol.0 {
                    path = candidate;
                    break;
                }
                slot = (slot + 1) & (self.path_slots.len() - 1);
            }
        }
        Some(path)
    }

    fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    fn path_count(&self) -> usize {
        self.paths.len()
    }

    fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Resolves a path into caller-owned scratch in authored order.
    ///
    /// Persistent compiler rows retain `PathId`; allocation is reserved for
    /// explicit presentation/export boundaries.
    fn resolve_path(
        &self,
        path: PathId,
        output: &mut Vec<SymbolId>,
    ) -> Result<(), TextCatalogError> {
        output.clear();
        let mut current = path;
        loop {
            let row = self.path(current).ok_or_else(|| {
                TextCatalogError::new(format!("path {} is outside the catalog", current.0))
            })?;
            let Some(segment) = row.segment() else {
                break;
            };
            output.push(segment);
            current = row.parent;
        }
        output.reverse();
        Ok(())
    }

    fn format_path(&self, path: PathId, separator: &str) -> Result<String, TextCatalogError> {
        let depth = self
            .path(path)
            .ok_or_else(|| {
                TextCatalogError::new(format!("path {} is outside the catalog", path.0))
            })?
            .depth as usize;
        let mut segments = Vec::with_capacity(depth);
        self.resolve_path(path, &mut segments)?;
        let mut output = String::new();
        for (index, segment) in segments.into_iter().enumerate() {
            if index != 0 {
                output.push_str(separator);
            }
            output.push_str(self.symbol(segment).ok_or_else(|| {
                TextCatalogError::new(format!("symbol {} is outside the catalog", segment.0))
            })?);
        }
        Ok(output)
    }
}

/// Process-local brand for one project text namespace.
///
/// The brand is a misuse detector only. It is never serialized, hashed into a
/// receipt, or used as stable project identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextAuthorityId(u64);

/// Detached symbol coordinate qualified by the authority that issued it.
/// Persistent packed rows store the four-byte [`SymbolId`] under one owning
/// [`ProjectTextSnapshot`]; public cross-aggregate APIs use this qualified
/// form so an in-range coordinate from another project cannot silently bind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QualifiedSymbolId {
    authority: TextAuthorityId,
    coordinate: SymbolId,
}

impl QualifiedSymbolId {
    pub const fn coordinate(self) -> SymbolId {
        self.coordinate
    }
}

/// Detached path coordinate qualified by the authority that issued it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QualifiedPathId {
    authority: TextAuthorityId,
    coordinate: PathId,
}

impl QualifiedPathId {
    pub const fn coordinate(self) -> PathId {
        self.coordinate
    }
}

/// Immutable text namespace carried once by a packed project aggregate.
///
/// IDs in hot rows are raw four-byte coordinates. The aggregate owns this
/// snapshot, and any ID crossing an aggregate boundary must be qualified.
#[derive(Clone, Debug)]
pub struct ProjectTextSnapshot {
    authority: TextAuthorityId,
    catalog: Arc<PackedTextCatalog>,
}

impl PartialEq for ProjectTextSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.catalog == other.catalog
    }
}

impl Eq for ProjectTextSnapshot {}

impl ProjectTextSnapshot {
    pub const fn authority_id(&self) -> TextAuthorityId {
        self.authority
    }

    pub fn same_authority(&self, other: &Self) -> bool {
        self.authority == other.authority && Arc::ptr_eq(&self.catalog, &other.catalog)
    }

    pub fn symbol_id(&self, ordinal: usize) -> Option<SymbolId> {
        self.catalog.symbol_id(ordinal)
    }

    pub fn symbol(&self, id: SymbolId) -> Option<&str> {
        self.catalog.symbol(id)
    }

    pub fn lookup_symbol(&self, value: &str) -> Option<SymbolId> {
        self.catalog.lookup_symbol(value)
    }

    pub fn qualified_symbol(&self, id: SymbolId) -> Option<QualifiedSymbolId> {
        self.symbol(id).map(|_| QualifiedSymbolId {
            authority: self.authority,
            coordinate: id,
        })
    }

    pub fn resolve_qualified_symbol(
        &self,
        symbol: QualifiedSymbolId,
    ) -> Result<&str, TextCatalogError> {
        self.require_authority(symbol.authority)?;
        self.symbol(symbol.coordinate).ok_or_else(|| {
            TextCatalogError::new(format!(
                "symbol {} is outside the text authority",
                symbol.coordinate.0
            ))
        })
    }

    pub fn compare_symbols(
        &self,
        left: SymbolId,
        right: SymbolId,
    ) -> Result<Ordering, TextCatalogError> {
        self.catalog.compare_symbols(left, right)
    }

    pub fn symbol_lexical_rank(&self, id: SymbolId) -> Option<u32> {
        self.catalog.symbol_lexical_rank(id)
    }

    pub fn qualified_path(&self, id: PathId) -> Option<QualifiedPathId> {
        self.catalog.path(id).map(|_| QualifiedPathId {
            authority: self.authority,
            coordinate: id,
        })
    }

    pub fn path_parent(&self, id: PathId) -> Option<Option<PathId>> {
        self.catalog.path(id).map(PathRow::parent)
    }

    pub fn path_segment(&self, id: PathId) -> Option<Option<SymbolId>> {
        self.catalog.path(id).map(PathRow::segment)
    }

    pub fn path_depth(&self, id: PathId) -> Option<u32> {
        self.catalog.path(id).map(PathRow::depth)
    }

    /// Resolve one authored path segment without allocating a temporary path.
    /// `ordinal` is zero-based from the root, even though paths are stored as
    /// parent-linked rows from leaf to root.
    pub fn path_symbol_at(&self, id: PathId, ordinal: u32) -> Option<SymbolId> {
        let depth = self.catalog.path(id)?.depth();
        if ordinal >= depth {
            return None;
        }
        let mut current = id;
        for _ in 0..depth - ordinal - 1 {
            current = self.catalog.path(current)?.parent()?;
        }
        self.catalog.path(current)?.segment()
    }

    pub fn path_digest(&self, id: PathId) -> Option<[u8; 32]> {
        self.catalog.path_digest(id)
    }

    /// Resolve one temporary rich path to its already-interned project
    /// coordinate without allocating. New packed producers carry `PathId`
    /// directly and never call this method.
    pub fn lookup_path<'a>(&self, segments: impl IntoIterator<Item = &'a str>) -> Option<PathId> {
        self.catalog.lookup_path(segments)
    }

    pub fn resolve_qualified_path(
        &self,
        path: QualifiedPathId,
        output: &mut Vec<SymbolId>,
    ) -> Result<(), TextCatalogError> {
        self.require_authority(path.authority)?;
        self.catalog.resolve_path(path.coordinate, output)
    }

    pub fn format_qualified_path(
        &self,
        path: QualifiedPathId,
        separator: &str,
    ) -> Result<String, TextCatalogError> {
        self.require_authority(path.authority)?;
        self.catalog.format_path(path.coordinate, separator)
    }

    pub fn symbol_count(&self) -> usize {
        self.catalog.symbol_count()
    }

    pub fn path_count(&self) -> usize {
        self.catalog.path_count()
    }

    pub fn byte_len(&self) -> usize {
        self.catalog.byte_len()
    }

    fn require_authority(&self, authority: TextAuthorityId) -> Result<(), TextCatalogError> {
        if authority == self.authority {
            Ok(())
        } else {
            Err(TextCatalogError::new(
                "text coordinate belongs to a different project authority",
            ))
        }
    }
}

/// One-revision builder for a packed symbol/path namespace.
///
/// This uses exact open-addressed indexes over flat rows. A later persistent
/// project authority will retain the indexes and publish chunk-shared
/// snapshots. This builder is deliberately consuming and must not be confused
/// with that M3 session owner.
#[derive(Debug)]
pub struct PackedTextCatalogBuilder {
    authority: TextAuthorityId,
    bytes: Vec<u8>,
    symbols: Vec<SymbolRow>,
    symbol_fingerprints: Vec<u64>,
    symbol_slots: Vec<u32>,
    paths: Vec<PathRow>,
    path_fingerprints: Vec<u64>,
    path_slots: Vec<u32>,
}

impl PackedTextCatalogBuilder {
    pub fn new() -> Self {
        Self::with_capacity(0, 0, 0)
    }

    pub fn with_capacity(symbols: usize, symbol_bytes: usize, paths: usize) -> Self {
        let root_digest: [u8; 32] = Sha256::digest(ROOT_PATH_DIGEST_DOMAIN).into();
        let authority = TextAuthorityId(
            NEXT_TEXT_AUTHORITY
                .fetch_update(
                    AtomicOrdering::Relaxed,
                    AtomicOrdering::Relaxed,
                    |current| current.checked_add(1),
                )
                .expect("process-local text authority namespace exhausted"),
        );
        let symbol_slot_count = lookup_slot_capacity(symbols);
        let path_slot_count = lookup_slot_capacity(paths);
        let mut path_rows = Vec::with_capacity(paths.saturating_add(1));
        path_rows.push(PathRow {
            parent: PathId::ROOT,
            segment: EMPTY_SLOT,
            depth: 0,
            stable_digest: root_digest,
        });
        Self {
            authority,
            bytes: Vec::with_capacity(symbol_bytes),
            symbols: Vec::with_capacity(symbols),
            symbol_fingerprints: Vec::with_capacity(symbols),
            symbol_slots: vec![EMPTY_SLOT; symbol_slot_count],
            paths: path_rows,
            path_fingerprints: Vec::from([lookup_hash(ROOT_PATH_DIGEST_DOMAIN)]),
            path_slots: vec![EMPTY_SLOT; path_slot_count],
        }
    }

    pub fn intern_symbol(&mut self, value: &str) -> Result<QualifiedSymbolId, TextCatalogError> {
        let coordinate = self.intern_symbol_coordinate(value)?;
        Ok(QualifiedSymbolId {
            authority: self.authority,
            coordinate,
        })
    }

    fn intern_symbol_coordinate(&mut self, value: &str) -> Result<SymbolId, TextCatalogError> {
        let fingerprint = lookup_hash(value.as_bytes());
        if let Some(id) = self.find_symbol(fingerprint, value.as_bytes()) {
            return Ok(id);
        }
        let byte_end = self
            .bytes
            .len()
            .checked_add(value.len())
            .ok_or_else(|| TextCatalogError::new("symbol byte slab length overflow"))?;
        if byte_end > u32::MAX as usize {
            return Err(TextCatalogError::new("symbol byte slab exceeds u32"));
        }
        let byte_start = self.bytes.len() as u32;
        let byte_len = u32::try_from(value.len())
            .map_err(|_| TextCatalogError::new("one symbol exceeds u32 bytes"))?;
        let id = SymbolId(dense_id(self.symbols.len(), "symbol count")?);
        self.ensure_symbol_slots()?;
        self.bytes.extend_from_slice(value.as_bytes());
        self.symbols.push(SymbolRow {
            byte_start,
            byte_len,
        });
        self.symbol_fingerprints.push(fingerprint);
        self.insert_symbol_slot(id);
        Ok(id)
    }

    pub fn intern_path<'a>(
        &mut self,
        segments: impl IntoIterator<Item = &'a str>,
    ) -> Result<QualifiedPathId, TextCatalogError> {
        let mut path = PathId::ROOT;
        for segment in segments {
            let segment = self.intern_symbol_coordinate(segment)?;
            path = self.extend_path_coordinate(path, segment)?;
        }
        Ok(QualifiedPathId {
            authority: self.authority,
            coordinate: path,
        })
    }

    pub fn intern_symbol_path(
        &mut self,
        segments: impl IntoIterator<Item = QualifiedSymbolId>,
    ) -> Result<QualifiedPathId, TextCatalogError> {
        let mut path = PathId::ROOT;
        for segment in segments {
            self.require_authority(segment.authority)?;
            let segment = segment.coordinate;
            if segment.as_usize() >= self.symbols.len() {
                return Err(TextCatalogError::new(format!(
                    "symbol {} is outside the catalog",
                    segment.0
                )));
            }
            path = self.extend_path_coordinate(path, segment)?;
        }
        Ok(QualifiedPathId {
            authority: self.authority,
            coordinate: path,
        })
    }

    pub fn extend_path(
        &mut self,
        parent: QualifiedPathId,
        segment: QualifiedSymbolId,
    ) -> Result<QualifiedPathId, TextCatalogError> {
        self.require_authority(parent.authority)?;
        self.require_authority(segment.authority)?;
        let coordinate = self.extend_path_coordinate(parent.coordinate, segment.coordinate)?;
        Ok(QualifiedPathId {
            authority: self.authority,
            coordinate,
        })
    }

    fn extend_path_coordinate(
        &mut self,
        parent: PathId,
        segment: SymbolId,
    ) -> Result<PathId, TextCatalogError> {
        let parent_row = *self.paths.get(parent.as_usize()).ok_or_else(|| {
            TextCatalogError::new(format!("parent path {} is outside the catalog", parent.0))
        })?;
        let symbol = *self.symbols.get(segment.as_usize()).ok_or_else(|| {
            TextCatalogError::new(format!("symbol {} is outside the catalog", segment.0))
        })?;
        let fingerprint = path_lookup_hash(parent, segment);
        if let Some(id) = self.find_path(fingerprint, parent, segment) {
            return Ok(id);
        }
        let depth = parent_row
            .depth
            .checked_add(1)
            .ok_or_else(|| TextCatalogError::new("path depth exceeds u32"))?;
        let id = PathId(dense_id(self.paths.len(), "path count")?);
        self.ensure_path_slots()?;
        let mut hasher = Sha256::new();
        hasher.update(PATH_DIGEST_DOMAIN);
        hasher.update(parent_row.stable_digest);
        hasher.update(symbol.byte_len.to_be_bytes());
        hasher.update(&self.bytes[symbol.bytes()]);
        self.paths.push(PathRow {
            parent,
            segment: segment.0,
            depth,
            stable_digest: hasher.finalize().into(),
        });
        self.path_fingerprints.push(fingerprint);
        self.insert_path_slot(id);
        Ok(id)
    }

    pub fn resolve_symbol(&self, id: QualifiedSymbolId) -> Result<&str, TextCatalogError> {
        self.require_authority(id.authority)?;
        self.symbol_coordinate(id.coordinate).ok_or_else(|| {
            TextCatalogError::new(format!("symbol {} is outside the catalog", id.coordinate.0))
        })
    }

    fn symbol_coordinate(&self, id: SymbolId) -> Option<&str> {
        let row = *self.symbols.get(id.as_usize())?;
        std::str::from_utf8(&self.bytes[row.bytes()]).ok()
    }

    pub fn path_depth(&self, id: QualifiedPathId) -> Result<u32, TextCatalogError> {
        self.require_authority(id.authority)?;
        self.paths
            .get(id.coordinate.as_usize())
            .map(|row| row.depth)
            .ok_or_else(|| {
                TextCatalogError::new(format!("path {} is outside the catalog", id.coordinate.0))
            })
    }

    pub fn freeze(self) -> ProjectTextSnapshot {
        let mut lexical_order = (0..self.symbols.len()).collect::<Vec<_>>();
        lexical_order.sort_unstable_by(|left, right| {
            let left = self.symbols[*left];
            let right = self.symbols[*right];
            self.bytes[left.bytes()].cmp(&self.bytes[right.bytes()])
        });
        let mut symbol_lexical_ranks = vec![0_u32; self.symbols.len()];
        for (rank, symbol) in lexical_order.into_iter().enumerate() {
            symbol_lexical_ranks[symbol] =
                u32::try_from(rank).expect("packed symbol count already fits the u32 namespace");
        }
        ProjectTextSnapshot {
            authority: self.authority,
            catalog: Arc::new(PackedTextCatalog {
                bytes: self.bytes.into_boxed_slice(),
                symbols: self.symbols.into_boxed_slice(),
                symbol_lexical_ranks: symbol_lexical_ranks.into_boxed_slice(),
                symbol_slots: self.symbol_slots.into_boxed_slice(),
                paths: self.paths.into_boxed_slice(),
                path_slots: self.path_slots.into_boxed_slice(),
            }),
        }
    }

    pub fn root_path(&self) -> QualifiedPathId {
        QualifiedPathId {
            authority: self.authority,
            coordinate: PathId::ROOT,
        }
    }

    fn require_authority(&self, authority: TextAuthorityId) -> Result<(), TextCatalogError> {
        if authority == self.authority {
            Ok(())
        } else {
            Err(TextCatalogError::new(
                "text coordinate belongs to a different project authority",
            ))
        }
    }

    fn find_symbol(&self, fingerprint: u64, bytes: &[u8]) -> Option<SymbolId> {
        let mut slot = fingerprint as usize & (self.symbol_slots.len() - 1);
        loop {
            let encoded = self.symbol_slots[slot];
            if encoded == EMPTY_SLOT {
                return None;
            }
            let id = SymbolId(encoded);
            let row = self.symbols[id.as_usize()];
            if self.symbol_fingerprints[id.as_usize()] == fingerprint
                && &self.bytes[row.bytes()] == bytes
            {
                return Some(id);
            }
            slot = (slot + 1) & (self.symbol_slots.len() - 1);
        }
    }

    fn find_path(&self, fingerprint: u64, parent: PathId, segment: SymbolId) -> Option<PathId> {
        let mut slot = fingerprint as usize & (self.path_slots.len() - 1);
        loop {
            let encoded = self.path_slots[slot];
            if encoded == EMPTY_SLOT {
                return None;
            }
            let id = PathId(encoded);
            let row = self.paths[id.as_usize()];
            if self.path_fingerprints[id.as_usize()] == fingerprint
                && row.parent == parent
                && row.segment == segment.0
            {
                return Some(id);
            }
            slot = (slot + 1) & (self.path_slots.len() - 1);
        }
    }

    fn ensure_symbol_slots(&mut self) -> Result<(), TextCatalogError> {
        if (self.symbols.len() + 1) * 10 < self.symbol_slots.len() * 7 {
            return Ok(());
        }
        let capacity = self
            .symbol_slots
            .len()
            .checked_mul(2)
            .ok_or_else(|| TextCatalogError::new("symbol lookup capacity overflow"))?;
        self.symbol_slots = vec![EMPTY_SLOT; capacity];
        for index in 0..self.symbols.len() {
            self.insert_symbol_slot(SymbolId(index as u32));
        }
        Ok(())
    }

    fn ensure_path_slots(&mut self) -> Result<(), TextCatalogError> {
        // Row zero is the root and is not looked up through `(parent, segment)`.
        if self.paths.len() * 10 < self.path_slots.len() * 7 {
            return Ok(());
        }
        let capacity = self
            .path_slots
            .len()
            .checked_mul(2)
            .ok_or_else(|| TextCatalogError::new("path lookup capacity overflow"))?;
        self.path_slots = vec![EMPTY_SLOT; capacity];
        for index in 1..self.paths.len() {
            self.insert_path_slot(PathId(index as u32));
        }
        Ok(())
    }

    fn insert_symbol_slot(&mut self, id: SymbolId) {
        let fingerprint = self.symbol_fingerprints[id.as_usize()];
        let mut slot = fingerprint as usize & (self.symbol_slots.len() - 1);
        while self.symbol_slots[slot] != EMPTY_SLOT {
            slot = (slot + 1) & (self.symbol_slots.len() - 1);
        }
        self.symbol_slots[slot] = id.0;
    }

    fn insert_path_slot(&mut self, id: PathId) {
        let fingerprint = self.path_fingerprints[id.as_usize()];
        let mut slot = fingerprint as usize & (self.path_slots.len() - 1);
        while self.path_slots[slot] != EMPTY_SLOT {
            slot = (slot + 1) & (self.path_slots.len() - 1);
        }
        self.path_slots[slot] = id.0;
    }
}

fn lookup_slot_capacity(expected_rows: usize) -> usize {
    expected_rows
        .saturating_mul(10)
        .checked_div(7)
        .unwrap_or(usize::MAX)
        .saturating_add(1)
        .max(8)
        .checked_next_power_of_two()
        .unwrap_or(1_usize << (usize::BITS - 1))
}

fn dense_id(length: usize, description: &str) -> Result<u32, TextCatalogError> {
    let id = u32::try_from(length)
        .map_err(|_| TextCatalogError::new(format!("{description} exceeds u32")))?;
    if id == EMPTY_SLOT {
        return Err(TextCatalogError::new(format!(
            "{description} exhausts the reserved dense-ID sentinel"
        )));
    }
    Ok(id)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextCatalogError {
    message: String,
}

impl TextCatalogError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TextCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for TextCatalogError {}

fn path_lookup_hash(parent: PathId, segment: SymbolId) -> u64 {
    let mut bytes = [0_u8; 8];
    bytes[..4].copy_from_slice(&parent.0.to_le_bytes());
    bytes[4..].copy_from_slice(&segment.0.to_le_bytes());
    lookup_hash(&bytes)
}

fn lookup_hash(bytes: &[u8]) -> u64 {
    // FNV-1a is a lookup accelerator only. Stable identities use SHA-256 over
    // resolved bytes and never depend on this table layout.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_and_paths_are_exactly_interned() {
        let mut authority = PackedTextCatalogBuilder::new();
        let alpha = authority.intern_symbol("alpha").unwrap();
        assert_eq!(authority.intern_symbol("alpha").unwrap(), alpha);
        let first = authority.intern_path(["alpha", "beta"]).unwrap();
        let beta = authority.intern_symbol("beta").unwrap();
        let second = authority.intern_symbol_path([alpha, beta]).unwrap();
        assert_eq!(first, second);

        let catalog = authority.freeze();
        assert_eq!(
            catalog.symbol_id(alpha.coordinate().as_usize()),
            Some(alpha.coordinate())
        );
        assert_eq!(catalog.symbol_id(catalog.symbol_count()), None);
        assert_eq!(catalog.resolve_qualified_symbol(alpha).unwrap(), "alpha");
        assert_eq!(catalog.lookup_symbol("alpha"), Some(alpha.coordinate()));
        assert_eq!(catalog.lookup_symbol("missing"), None);
        assert_eq!(
            catalog.format_qualified_path(first, "/").unwrap(),
            "alpha/beta"
        );
        assert_eq!(
            catalog.lookup_path(["alpha", "beta"]),
            Some(first.coordinate())
        );
        assert_eq!(catalog.lookup_path(["alpha", "missing"]), None);
        assert_eq!(catalog.path_depth(first.coordinate()), Some(2));
        assert_eq!(
            catalog
                .path_symbol_at(first.coordinate(), 0)
                .and_then(|symbol| catalog.symbol(symbol)),
            Some("alpha")
        );
        assert_eq!(
            catalog
                .path_symbol_at(first.coordinate(), 1)
                .and_then(|symbol| catalog.symbol(symbol)),
            Some("beta")
        );
        assert_eq!(catalog.path_symbol_at(first.coordinate(), 2), None);
    }

    #[test]
    fn frozen_symbol_ranks_preserve_byte_lexical_order() {
        let mut authority = PackedTextCatalogBuilder::new();
        let zebra = authority.intern_symbol("zebra").unwrap();
        let alpha = authority.intern_symbol("alpha").unwrap();
        let alphabet = authority.intern_symbol("alphabet").unwrap();
        let catalog = authority.freeze();

        assert_eq!(
            catalog.compare_symbols(alpha.coordinate(), alphabet.coordinate()),
            Ok(Ordering::Less)
        );
        assert_eq!(
            catalog.compare_symbols(alphabet.coordinate(), zebra.coordinate()),
            Ok(Ordering::Less)
        );
        assert!(
            catalog.symbol_lexical_rank(alpha.coordinate()).unwrap()
                < catalog.symbol_lexical_rank(zebra.coordinate()).unwrap()
        );
    }

    #[test]
    fn stable_path_digest_depends_on_bytes_not_intern_order() {
        let mut left = PackedTextCatalogBuilder::new();
        left.intern_path(["unrelated"]).unwrap();
        let left_path = left.intern_path(["module", "value"]).unwrap();
        let left = left.freeze();

        let mut right = PackedTextCatalogBuilder::new();
        let right_path = right.intern_path(["module", "value"]).unwrap();
        right.intern_symbol("unrelated").unwrap();
        let right = right.freeze();

        assert_ne!(left_path.coordinate(), right_path.coordinate());
        assert_eq!(
            left.path_digest(left_path.coordinate()),
            right.path_digest(right_path.coordinate())
        );
    }

    #[test]
    fn invalid_coordinates_fail_closed() {
        let mut authority = PackedTextCatalogBuilder::new();
        let missing_symbol = QualifiedSymbolId {
            authority: authority.authority,
            coordinate: SymbolId(99),
        };
        let missing_path = QualifiedPathId {
            authority: authority.authority,
            coordinate: PathId(99),
        };
        assert!(
            authority
                .extend_path(authority.root_path(), missing_symbol)
                .is_err()
        );
        let x = authority.intern_symbol("x").unwrap();
        assert!(authority.extend_path(missing_path, x).is_err());
        let catalog = authority.freeze();
        assert!(catalog.resolve_qualified_symbol(missing_symbol).is_err());
        assert!(catalog.format_qualified_path(missing_path, "/").is_err());
    }

    #[test]
    fn foreign_in_range_coordinates_fail_closed() {
        let mut left = PackedTextCatalogBuilder::new();
        let left_symbol = left.intern_symbol("alpha").unwrap();
        let left_path = left.intern_path(["alpha"]).unwrap();
        let left = left.freeze();

        let mut right = PackedTextCatalogBuilder::new();
        let right_symbol = right.intern_symbol("beta").unwrap();
        let right_path = right.intern_path(["beta"]).unwrap();
        let right = right.freeze();

        assert_eq!(left_symbol.coordinate(), right_symbol.coordinate());
        assert_eq!(left_path.coordinate(), right_path.coordinate());
        assert!(right.resolve_qualified_symbol(left_symbol).is_err());
        assert!(right.format_qualified_path(left_path, "/").is_err());
        assert!(!left.same_authority(&right));
    }
}
