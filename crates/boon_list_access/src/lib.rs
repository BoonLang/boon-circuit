//! Deterministic ordered access over canonical row identities.
//!
//! The kernel stores structural index keys and row identities only. Canonical row
//! values remain owned by the caller.
//!
//! ```
//! use boon_list_access::{
//!     Direction, IndexPlanId, KeyComponent, KeyKind, KeySchema, OrderedIndex, RowId,
//!     SourceOrderToken, StructuralKey, StructuralValue, WorkLimits, WorkTracker,
//! };
//!
//! let schema = KeySchema::new(vec![KeyComponent::new(
//!     KeyKind::Text,
//!     Direction::Asc,
//! )])?;
//! let mut index = OrderedIndex::new(IndexPlanId::from_u128(1), schema);
//! let key = StructuralKey::new(vec![StructuralValue::text("Oslo")])?;
//! index.insert(RowId::from_u128(7), SourceOrderToken::from_u128(10), key.clone())?;
//!
//! let mut stream = index.exact(&key, None)?;
//! let mut work = WorkTracker::new(WorkLimits::default());
//! assert_eq!(stream.next(&mut work)?.unwrap().row_id(), RowId::from_u128(7));
//! assert!(stream.next(&mut work)?.is_none());
//! # Ok::<(), boon_list_access::AccessError>(())
//! ```

#![forbid(unsafe_code)]

mod index;
mod key;
mod work;

pub use index::{
    AccessError, AccessItem, AccessStream, CursorKey, IndexMetrics, IndexPlanId, IndexResource,
    IndexResourceLimits, IntegrityReport, MutationOutcome, OrderedIndex,
    OrderedIndexIntegrityPhase, OrderedIndexIntegrityPoll, OrderedIndexIntegrityProgress,
    OrderedIndexIntegrityResult, OrderedIndexIntegrityTask, RowId, SourceOrderToken,
};
pub use key::{
    ClosedTag, Direction, EncodedKey, FiniteNumber, KEY_CODEC_VERSION, KeyComponent, KeyError,
    KeyKind, KeySchema, MAX_KEY_COMPONENTS, StructuralKey, StructuralValue, TagTypeId,
};
pub use work::{AccessMetrics, LimitKind, WorkLimitExceeded, WorkLimits, WorkTracker};

#[cfg(test)]
mod tests;
